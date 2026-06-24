//! # Cloud Sync Daemon
//!
//! A background daemon that monitors a local watched directory for file modifications,
//! creations, and deletions using the `notify` crate, and synchronizes those changes
//! to all configured and enabled cloud storage providers.

use cloud_sync_lib::{DropboxProvider, GoogleDriveProvider, OneDriveProvider, StorageBackend, OAuthCredentials, SimulatedFallback, LocalSimulation};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Global configuration parsed from the configuration TOML file.
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

async fn load_or_create_config(path: &str) -> Result<AppConfig, Box<dyn std::error::Error>> {
    let config_path = Path::new(path);
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

fn is_enabled(credentials: &Option<OAuthCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

fn get_remote_path(path: &Path, watch_dir: &Path) -> Option<String> {
    let relative_path = match path.strip_prefix(watch_dir) {
        Ok(p) => p.to_path_buf(),
        Err(_) => {
            let path_str = path.to_string_lossy();
            let watch_dir_str = watch_dir.to_string_lossy();
            if path_str.starts_with(&*watch_dir_str) {
                Path::new(&path_str[watch_dir_str.len()..]).to_path_buf()
            } else {
                return None;
            }
        }
    };
    Some(relative_path.to_string_lossy().replace('\\', "/"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let args: Vec<String> = std::env::args().collect();
    let config_file = if args.len() > 1 {
        args[1].clone()
    } else {
        "config.toml".to_string()
    };

    info!("Starting Cloud Sync Daemon using config: {}...", config_file);

    // Load configuration
    let config = load_or_create_config(&config_file).await?;

    // Ensure the directories exist
    fs::create_dir_all(&config.watch_directory).await?;
    let watch_dir = fs::canonicalize(&config.watch_directory).await?;
    info!("Watching directory: {:?}", watch_dir);

    // Initialize Providers
    let mut backends: Vec<Arc<dyn StorageBackend>> = Vec::new();

    if is_enabled(&config.google_credentials) {
        let inner = config.google_credentials.clone().map(GoogleDriveProvider::new);
        let local_sim = LocalSimulation::new(config.google_drive_root.clone(), "Google Drive".to_string());
        let drive = Arc::new(SimulatedFallback::new(inner, local_sim, "Google Drive"));
        backends.push(drive);
    } else {
        info!("Google Drive provider is disabled in configuration.");
    }

    if is_enabled(&config.dropbox_credentials) {
        let inner = config.dropbox_credentials.clone().map(DropboxProvider::new);
        let local_sim = LocalSimulation::new(config.dropbox_root.clone(), "Dropbox".to_string());
        let dropbox = Arc::new(SimulatedFallback::new(inner, local_sim, "Dropbox"));
        backends.push(dropbox);
    } else {
        info!("Dropbox provider is disabled in configuration.");
    }

    if is_enabled(&config.onedrive_credentials) {
        let inner = config.onedrive_credentials.clone().map(OneDriveProvider::new);
        let local_sim = LocalSimulation::new(config.onedrive_root.clone(), "OneDrive".to_string());
        let onedrive = Arc::new(SimulatedFallback::new(inner, local_sim, "OneDrive"));
        backends.push(onedrive);
    } else {
        info!("OneDrive provider is disabled in configuration.");
    }

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

    let active_locks = Arc::new(Mutex::new(HashMap::new()));

    // Process events
    while let Some(res) = rx.recv().await {
        match res {
            Ok(event) => {
                handle_event(event, &watch_dir, &backends, active_locks.clone()).await;
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
    active_locks: Arc<Mutex<HashMap<(String, PathBuf), Arc<tokio::sync::Mutex<()>>>>>,
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

                let remote_path_str = match get_remote_path(&abs_path, watch_dir) {
                    Some(p) => p,
                    None => {
                        error!("Failed to strip prefix for {:?} (absolute: {:?})", path, abs_path);
                        continue;
                    }
                };
                info!("File change detected: '{}'. Syncing to all cloud backends...", remote_path_str);

                for backend in backends {
                    let backend = backend.clone();
                    let local_path = path.clone();
                    let remote_path = remote_path_str.clone();

                    let key = (backend.name().to_string(), local_path.clone());
                    let file_mutex = {
                        let mut locks = active_locks.lock().await;
                        locks.entry(key).or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))).clone()
                    };

                    tokio::spawn(async move {
                        // Sequential lock to prevent concurrent uploads for the same file/backend
                        let _guard = file_mutex.lock().await;

                        // Debounce: wait briefly for concurrent writes/events to settle
                        tokio::time::sleep(Duration::from_millis(150)).await;

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
                let remote_path_str = match get_remote_path(&path, watch_dir) {
                    Some(p) => p,
                    None => {
                        error!("Failed to strip prefix for deleted path {:?}", path);
                        continue;
                    }
                };
                info!("File deletion detected: '{}'. Deleting from all cloud backends...", remote_path_str);

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
