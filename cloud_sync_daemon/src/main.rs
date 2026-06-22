use cloud_sync_lib::{DropboxProvider, GoogleDriveProvider, OneDriveProvider, StorageBackend, OAuthCredentials};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AppConfig {
    watch_directory: PathBuf,
    google_drive_root: PathBuf,
    dropbox_root: PathBuf,
    onedrive_root: PathBuf,
    google_credentials: Option<OAuthCredentials>,
    dropbox_credentials: Option<OAuthCredentials>,
    onedrive_credentials: Option<OAuthCredentials>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            watch_directory: PathBuf::from("./watched_folder"),
            google_drive_root: PathBuf::from("./cloud_simulation/google_drive"),
            dropbox_root: PathBuf::from("./cloud_simulation/dropbox"),
            onedrive_root: PathBuf::from("./cloud_simulation/onedrive"),
            google_credentials: None,
            dropbox_credentials: None,
            onedrive_credentials: None,
        }
    }
}

async fn load_or_create_config() -> Result<AppConfig, Box<dyn std::error::Error>> {
    let config_path = Path::new("config.toml");
    if config_path.exists() {
        let content = fs::read_to_string(config_path).await?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    } else {
        let config = AppConfig::default();
        let content = toml::to_string_pretty(&config)?;
        fs::write(config_path, content).await?;
        info!("Created default configuration file at {:?}", config_path);
        Ok(config)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    info!("Starting Cloud Sync Daemon...");

    // Load configuration
    let config = load_or_create_config().await?;

    // Ensure the directories exist
    fs::create_dir_all(&config.watch_directory).await?;
    let watch_dir = fs::canonicalize(&config.watch_directory).await?;
    info!("Watching directory: {:?}", watch_dir);

    // Initialize Providers
    let drive = Arc::new(GoogleDriveProvider::new(&config.google_drive_root, config.google_credentials.clone()).await?);
    let dropbox = Arc::new(DropboxProvider::new(&config.dropbox_root, config.dropbox_credentials.clone()).await?);
    let onedrive = Arc::new(OneDriveProvider::new(&config.onedrive_root, config.onedrive_credentials.clone()).await?);

    let backends: Vec<Arc<dyn StorageBackend>> = vec![drive, dropbox, onedrive];
    info!("Initialized cloud storage providers:");
    for backend in &backends {
        info!(" - {}", backend.name());
    }

    // Set up mpsc channel for events
    let (tx, mut rx) = mpsc::channel::<notify::Result<Event>>(100);

    // Set up file watcher
    let rt = tokio::runtime::Handle::current();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = rt.block_on(async {
                tx.send(res).await.ok();
            });
        },
        Config::default().with_compare_contents(true),
    )?;

    watcher.watch(&watch_dir, RecursiveMode::Recursive)?;

    info!("Daemon is listening for changes... Press Ctrl+C to exit.");

    // Process events
    while let Some(res) = rx.recv().await {
        match res {
            Ok(event) => {
                handle_event(event, &watch_dir, &backends).await;
            }
            Err(e) => error!("Watcher error: {:?}", e),
        }
    }

    Ok(())
}

async fn handle_event(
    event: Event,
    watch_dir: &Path,
    backends: &[Arc<dyn StorageBackend>],
) {
    // Only respond to creation, modification (writes), and deletions
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(notify::event::ModifyKind::Data(_)) | EventKind::Modify(notify::event::ModifyKind::Any) => {
            for path in event.paths {
                if !path.exists() {
                    continue; // Skip if file was deleted before we could process it
                }

                // Make sure it is a file (we don't sync empty directories in this simple logic, but can be extended)
                let metadata = match fs::metadata(&path).await {
                    Ok(m) => m,
                    Err(e) => {
                        error!("Failed to read metadata for {:?}: {}", path, e);
                        continue;
                    }
                };

                if !metadata.is_file() {
                    continue;
                }

                // Canonicalize event path
                let abs_path = fs::canonicalize(&path).await.unwrap_or(path.clone());

                // Compute relative path to map to remote path
                let relative_path = match abs_path.strip_prefix(watch_dir) {
                    Ok(p) => p,
                    Err(e) => {
                        error!("Failed to strip prefix for {:?} (absolute: {:?}): {}", path, abs_path, e);
                        continue;
                    }
                };

                let remote_path_str = relative_path.to_string_lossy().replace('\\', "/");
                info!("File change detected: {:?}. Syncing to all cloud backends...", relative_path);

                for backend in backends {
                    let backend = backend.clone();
                    let local_path = path.clone();
                    let remote_path = remote_path_str.clone();

                    tokio::spawn(async move {
                        // Add minor delay/retry logic in case the file is still being written to by the OS/editor
                        let mut attempts = 3;
                        while attempts > 0 {
                            match backend.upload(&local_path, &remote_path).await {
                                Ok(_) => {
                                    info!("[{}] Successfully synced '{}'", backend.name(), remote_path);
                                    break;
                                }
                                Err(e) => {
                                    warn!(
                                        "[{}] Attempt failed to sync '{}': {}. Retrying in 500ms...",
                                        backend.name(),
                                        remote_path,
                                        e
                                    );
                                    tokio::time::sleep(Duration::from_millis(500)).await;
                                    attempts -= 1;
                                }
                            }
                        }
                        if attempts == 0 {
                            error!(
                                "[{}] Failed to sync '{}' after multiple attempts.",
                                backend.name(),
                                remote_path
                            );
                        }
                    });
                }
            }
        }
        EventKind::Remove(_) => {
            for path in event.paths {
                // Determine remote path based on watch directory
                // Since path is already deleted, we might not be able to resolve strip_prefix if the event contains a relative or resolved path.
                // Normally notify returns the absolute path that was deleted.
                // Since path is already deleted, canonicalization of path won't work.
                // We'll canonicalize the input path directly (which should match watch_dir formatting)
                // or fall back to resolving relative paths manually if prefix stripping fails.
                let relative_path = match path.strip_prefix(watch_dir) {
                    Ok(p) => p.to_path_buf(),
                    Err(_) => {
                        // Attempt to strip using standard canonicalized watch directory string matches
                        let path_str = path.to_string_lossy();
                        let watch_dir_str = watch_dir.to_string_lossy();
                        if path_str.starts_with(&*watch_dir_str) {
                            Path::new(&path_str[watch_dir_str.len()..]).to_path_buf()
                        } else {
                            error!("Failed to strip prefix for deleted path {:?}", path);
                            continue;
                        }
                    }
                };

                let remote_path_str = relative_path.to_string_lossy().replace('\\', "/");
                info!("File deletion detected: {:?}. Deleting from all cloud backends...", relative_path);

                for backend in backends {
                    let backend = backend.clone();
                    let remote_path = remote_path_str.clone();

                    tokio::spawn(async move {
                        match backend.delete(&remote_path).await {
                            Ok(_) => info!("[{}] Successfully deleted remote file '{}'", backend.name(), remote_path),
                            Err(cloud_sync_lib::StorageError::NotFound(_)) => {
                                // Already deleted or doesn't exist
                            }
                            Err(e) => error!("[{}] Failed to delete '{}': {}", backend.name(), remote_path, e),
                        }
                    });
                }
            }
        }
        _ => {}
    }
}
