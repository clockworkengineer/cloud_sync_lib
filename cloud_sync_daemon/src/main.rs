//! # Cloud Sync Daemon
//!
//! A background daemon that monitors a local watched directory for file modifications,
//! creations, and deletions using the `notify` crate, and synchronizes those changes
//! to all configured and enabled cloud storage providers.

use cloud_sync_lib::{DropboxProvider, GoogleDriveProvider, OneDriveProvider, WebDAVProvider, S3Provider, StorageBackend, OAuthCredentials, WebDAVCredentials, S3Credentials, SimulatedFallback, LocalSimulation};
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
    webdav_root: PathBuf,
    s3_root: PathBuf,
    google_credentials: Option<OAuthCredentials>,
    dropbox_credentials: Option<OAuthCredentials>,
    onedrive_credentials: Option<OAuthCredentials>,
    webdav_credentials: Option<WebDAVCredentials>,
    s3_credentials: Option<S3Credentials>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            watch_directory: PathBuf::from("./watched_folder"),
            google_drive_root: PathBuf::from("./cloud_simulation/google_drive"),
            dropbox_root: PathBuf::from("./cloud_simulation/dropbox"),
            onedrive_root: PathBuf::from("./cloud_simulation/onedrive"),
            webdav_root: PathBuf::from("./cloud_simulation/webdav"),
            s3_root: PathBuf::from("./cloud_simulation/s3"),
            google_credentials: None,
            dropbox_credentials: None,
            onedrive_credentials: None,
            webdav_credentials: None,
            s3_credentials: None,
        }
    }
}

/// Load configuration from a TOML file. If the file doesn't exist, a default config is created and saved.
///
/// # Arguments
/// * `path` - The path to the config file.
///
/// # Returns
/// The loaded config or an error if file I/O or parsing fails.
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

/// Helper function to check if a provider is enabled based on its credentials config.
///
/// # Arguments
/// * `credentials` - OAuth credentials configuration options.
///
/// # Returns
/// True if the provider is enabled or no enabled flag is explicitly set to false, false otherwise.
fn is_enabled(credentials: &Option<OAuthCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to check if WebDAV provider is enabled.
///
/// # Arguments
/// * `credentials` - WebDAV credentials configuration options.
///
/// # Returns
/// True if the provider is enabled, false otherwise.
fn is_webdav_enabled(credentials: &Option<WebDAVCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to check if S3 provider is enabled.
///
/// # Arguments
/// * `credentials` - S3 credentials configuration options.
///
/// # Returns
/// True if the provider is enabled, false otherwise.
fn is_s3_enabled(credentials: &Option<S3Credentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to strip prefix from the watch directory path to get the relative remote path.
///
/// # Arguments
/// * `path` - Path of the file being synced.
/// * `watch_dir` - The watched root directory path.
///
/// # Returns
/// The normalized remote path string, or None if prefix stripping fails.
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

/// Internal state of the daemon.
struct DaemonState {
    /// True if the sync operations are temporarily paused.
    paused: bool,
    /// Loaded list of active cloud storage backends.
    backends: Vec<Arc<dyn StorageBackend>>,
    /// Path of the watched directory.
    watch_dir: PathBuf,
    /// Path of the configuration TOML file.
    config_file: String,
    /// True if a manual or automatic synchronization is actively running.
    syncing: bool,
    /// The address of the Web UI server, if provided.
    ui_addr: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mut config_file = "config.toml".to_string();
    let mut ui_addr = None;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--ui-addr" && i + 1 < args.len() {
            ui_addr = Some(args[i + 1].clone());
            i += 2;
        } else {
            config_file = args[i].clone();
            i += 1;
        }
    }

    info!("Starting Cloud Sync Daemon using config: {}...", config_file);
    if let Some(ref addr) = ui_addr {
        info!("Web UI server address: {}", addr);
    }

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

    if is_webdav_enabled(&config.webdav_credentials) {
        let inner = config.webdav_credentials.clone().map(WebDAVProvider::new);
        let local_sim = LocalSimulation::new(config.webdav_root.clone(), "WebDAV".to_string());
        let webdav = Arc::new(SimulatedFallback::new(inner, local_sim, "WebDAV"));
        backends.push(webdav);
    } else {
        info!("WebDAV provider is disabled in configuration.");
    }

    if is_s3_enabled(&config.s3_credentials) {
        let inner = config.s3_credentials.clone().map(S3Provider::new);
        let local_sim = LocalSimulation::new(config.s3_root.clone(), "S3".to_string());
        let s3_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "S3"));
        backends.push(s3_backend);
    } else {
        info!("S3 provider is disabled in configuration.");
    }

    info!("Initialized cloud storage providers:");
    for backend in &backends {
        info!(" - {}", backend.name());
    }

    // Wrap state in Mutex/Arc
    let state = Arc::new(Mutex::new(DaemonState {
        paused: false,
        backends,
        watch_dir: watch_dir.clone(),
        config_file: config_file.clone(),
        syncing: false,
        ui_addr,
    }));

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

    // Setup channel for shutdown command control
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // Spawn TCP control command listener
    let state_clone = state.clone();
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind("127.0.0.1:8081").await {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to bind TCP control socket: {}", e);
                return;
            }
        };
        info!("Control command TCP socket listening on 127.0.0.1:8081");

        loop {
            tokio::select! {
                conn = listener.accept() => {
                    if let Ok((mut socket, _)) = conn {
                        let state = state_clone.clone();
                        let shutdown_tx = shutdown_tx_clone.clone();
                        tokio::spawn(async move {
                            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                            let (reader, mut writer) = socket.split();
                            let mut reader = BufReader::new(reader);
                            let mut line = String::new();
                            if reader.read_line(&mut line).await.is_ok() {
                                let cmd = line.trim();
                                let response = handle_control_command(cmd, &state, &shutdown_tx).await;
                                let _ = writer.write_all(response.as_bytes()).await;
                                let _ = writer.flush().await;
                            }
                        });
                    }
                }
            }
        }
    });

    let active_locks = Arc::new(Mutex::new(HashMap::new()));

    // Process events or handle shutdown signal
    loop {
        tokio::select! {
            Some(res) = rx.recv() => {
                match res {
                    Ok(event) => {
                        handle_event(event, state.clone(), active_locks.clone()).await;
                    }
                    Err(e) => error!("Watcher error: {:?}", e),
                }
            }
            _ = shutdown_rx.recv() => {
                info!("Shutdown command received. Stopping daemon gracefully...");
                break;
            }
            else => break,
        }
    }

    Ok(())
}

/// Handles a string command sent from the daemon controller via TCP.
///
/// # Arguments
/// * `cmd` - Command string (e.g., "status", "pause", "resume", "reload", "sync", "stop").
/// * `state` - The daemon's internal state.
/// * `shutdown_tx` - Send channel for shutdown signals.
///
/// # Returns
/// A response string to be sent back to the TCP client.
async fn handle_control_command(
    cmd: &str,
    state: &Arc<Mutex<DaemonState>>,
    shutdown_tx: &mpsc::Sender<()>,
) -> String {
    match cmd {
        "status" => {
            let s = state.lock().await;
            let backend_names: Vec<String> = s.backends.iter().map(|b| b.name().to_string()).collect();
            format!(
                "Status: OK\nPaused: {}\nWatch Directory: {:?}\nConfig File: {}\nActive Backends: {:?}\nSyncing: {}\nWeb UI Address: {}\n",
                s.paused, s.watch_dir, s.config_file, backend_names, s.syncing, s.ui_addr.as_deref().unwrap_or("-")
            )
        }
        "pause" => {
            let mut s = state.lock().await;
            s.paused = true;
            info!("Daemon synchronization paused.");
            "Status: Paused\n".to_string()
        }
        "resume" => {
            let mut s = state.lock().await;
            s.paused = false;
            info!("Daemon synchronization resumed.");
            "Status: Resumed\n".to_string()
        }
        "reload" => {
            let mut s = state.lock().await;
            info!("Reloading configuration file: {}...", s.config_file);
            match load_or_create_config(&s.config_file).await {
                Ok(config) => {
                    let mut backends: Vec<Arc<dyn StorageBackend>> = Vec::new();
                    if is_enabled(&config.google_credentials) {
                        let inner = config.google_credentials.clone().map(GoogleDriveProvider::new);
                        let local_sim = LocalSimulation::new(config.google_drive_root.clone(), "Google Drive".to_string());
                        backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "Google Drive")));
                    }
                    if is_enabled(&config.dropbox_credentials) {
                        let inner = config.dropbox_credentials.clone().map(DropboxProvider::new);
                        let local_sim = LocalSimulation::new(config.dropbox_root.clone(), "Dropbox".to_string());
                        backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "Dropbox")));
                    }
                    if is_enabled(&config.onedrive_credentials) {
                        let inner = config.onedrive_credentials.clone().map(OneDriveProvider::new);
                        let local_sim = LocalSimulation::new(config.onedrive_root.clone(), "OneDrive".to_string());
                        backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "OneDrive")));
                    }
                    if is_webdav_enabled(&config.webdav_credentials) {
                        let inner = config.webdav_credentials.clone().map(WebDAVProvider::new);
                        let local_sim = LocalSimulation::new(config.webdav_root.clone(), "WebDAV".to_string());
                        backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "WebDAV")));
                    }
                    if is_s3_enabled(&config.s3_credentials) {
                        let inner = config.s3_credentials.clone().map(S3Provider::new);
                        let local_sim = LocalSimulation::new(config.s3_root.clone(), "S3".to_string());
                        backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "S3")));
                    }
                    s.backends = backends;
                    info!("Configuration reloaded successfully. Active backends updated.");
                    "Status: Config reloaded successfully\n".to_string()
                }
                Err(e) => {
                    error!("Failed to reload config: {}", e);
                    format!("Error: Failed to reload config: {}\n", e)
                }
            }
        }
        "sync" => {
            let mut s = state.lock().await;
            if s.syncing {
                "Error: Sync already in progress\n".to_string()
            } else {
                s.syncing = true;
                let watch_dir = s.watch_dir.clone();
                let backends = s.backends.clone();
                let state_clone = state.clone();
                tokio::spawn(async move {
                    info!("Manual sync triggered via control command. Scanning watch directory...");
                    if let Err(e) = trigger_full_sync(&watch_dir, &backends).await {
                        error!("Manual sync failed: {}", e);
                    } else {
                        info!("Manual sync completed successfully!");
                    }
                    state_clone.lock().await.syncing = false;
                });
                "Status: Sync triggered in background\n".to_string()
            }
        }
        "stop" => {
            let _ = shutdown_tx.send(()).await;
            "Status: Stopping daemon...\n".to_string()
        }
        _ => "Error: Unknown command. Supported: status, pause, resume, reload, sync, stop\n".to_string(),
    }
}

/// Scans the watch directory recursively and uploads all files to active backends.
///
/// # Arguments
/// * `watch_dir` - The local directory root to scan.
/// * `backends` - Slice of active storage backends.
///
/// # Returns
/// `std::io::Result` indicating scanning success/failure.
async fn trigger_full_sync(watch_dir: &Path, backends: &[Arc<dyn StorageBackend>]) -> std::io::Result<()> {
    let mut dir_entries = vec![watch_dir.to_path_buf()];
    while let Some(current_dir) = dir_entries.pop() {
        let mut entries = fs::read_dir(current_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = fs::metadata(&path).await?;
            if metadata.is_dir() {
                dir_entries.push(path);
            } else if metadata.is_file() {
                if let Some(remote_path_str) = get_remote_path(&path, watch_dir) {
                    for backend in backends {
                        let backend = backend.clone();
                        let local_path = path.clone();
                        let remote_path = remote_path_str.clone();
                        tokio::spawn(async move {
                            info!("[{}] Syncing '{}' via manual trigger", backend.name(), remote_path);
                            if let Err(e) = backend.upload(&local_path, &remote_path).await {
                                error!("[{}] Failed to sync '{}': {}", backend.name(), remote_path, e);
                            } else {
                                info!("[{}] Successfully synced '{}'", backend.name(), remote_path);
                            }
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// Processes a filesystem notification event from `notify`.
///
/// Automatically creates, updates, or deletes files on remote backends based on local events.
///
/// # Arguments
/// * `event` - The filesystem event detail.
/// * `state` - The daemon's internal state.
/// * `active_locks` - Concurrent sync locking map for active files/backends.
async fn handle_event(
    event: Event,
    state: Arc<Mutex<DaemonState>>,
    active_locks: Arc<Mutex<HashMap<(String, PathBuf), Arc<tokio::sync::Mutex<()>>>>>,
) {
    // Read current state
    let (paused, backends, watch_dir) = {
        let s = state.lock().await;
        (s.paused, s.backends.clone(), s.watch_dir.clone())
    };

    if paused {
        info!("Daemon is paused. Skipping file change event.");
        return;
    }

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

                let remote_path_str = match get_remote_path(&abs_path, &watch_dir) {
                    Some(p) => p,
                    None => {
                        error!("Failed to strip prefix for {:?} (absolute: {:?})", path, abs_path);
                        continue;
                    }
                };
                info!("File change detected: '{}'. Syncing to all cloud backends...", remote_path_str);

                for backend in &backends {
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
                let remote_path_str = match get_remote_path(&path, &watch_dir) {
                    Some(p) => p,
                    None => {
                        error!("Failed to strip prefix for deleted path {:?}", path);
                        continue;
                    }
                };
                info!("File deletion detected: '{}'. Deleting from all cloud backends...", remote_path_str);

                for backend in &backends {
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
