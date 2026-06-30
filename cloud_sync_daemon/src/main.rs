//! # Cloud Sync Daemon
//!
//! A background daemon that monitors a local watched directory for file modifications,
//! creations, and deletions using the `notify` crate, and synchronizes those changes
//! to all configured and enabled cloud storage providers.

pub mod config;
pub mod control;
pub mod watcher;

use cloud_sync_lib::{
    DropboxProvider, GoogleDriveProvider, OneDriveProvider, WebDAVProvider, S3Provider, SFTPProvider, NextcloudProvider,
    StorageBackend, SimulatedFallback, LocalSimulation
};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use config::{
    is_enabled, is_webdav_enabled, is_s3_enabled, is_sftp_enabled, is_nextcloud_enabled, load_or_create_config,
    DEFAULT_CONFIG_FILE
};
use watcher::handle_event;
use control::handle_control_command;

pub const DAEMON_BIND_ADDR: &str = "127.0.0.1:8081";
pub const DEBOUNCE_DELAY_MS: u64 = 150;
pub const RETRY_DELAY_MS: u64 = 500;
pub const MAX_SYNC_ATTEMPTS: u32 = 3;

/// Internal state of the daemon.
pub struct DaemonState {
    /// True if the sync operations are temporarily paused.
    pub paused: bool,
    /// Loaded list of active cloud storage backends.
    pub backends: Vec<Arc<dyn StorageBackend>>,
    /// Path of the watched directory.
    pub watch_dir: PathBuf,
    /// Path of the configuration TOML file.
    pub config_file: String,
    /// True if a manual or automatic synchronization is actively running.
    pub syncing: bool,
    /// The address of the Web UI server, if provided.
    pub ui_addr: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mut config_file = DEFAULT_CONFIG_FILE.to_string();
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
        let sync = config.google_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
        let inner = config.google_credentials.clone().map(GoogleDriveProvider::new);
        let local_sim = LocalSimulation::new(config.google_drive_root.clone(), "Google Drive".to_string());
        let drive = Arc::new(SimulatedFallback::new(inner, local_sim, "Google Drive", sync));
        backends.push(drive);
    } else {
        info!("Google Drive provider is disabled in configuration.");
    }

    if is_enabled(&config.dropbox_credentials) {
        let sync = config.dropbox_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
        let inner = config.dropbox_credentials.clone().map(DropboxProvider::new);
        let local_sim = LocalSimulation::new(config.dropbox_root.clone(), "Dropbox".to_string());
        let dropbox = Arc::new(SimulatedFallback::new(inner, local_sim, "Dropbox", sync));
        backends.push(dropbox);
    } else {
        info!("Dropbox provider is disabled in configuration.");
    }

    if is_enabled(&config.onedrive_credentials) {
        let sync = config.onedrive_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
        let inner = config.onedrive_credentials.clone().map(OneDriveProvider::new);
        let local_sim = LocalSimulation::new(config.onedrive_root.clone(), "OneDrive".to_string());
        let onedrive = Arc::new(SimulatedFallback::new(inner, local_sim, "OneDrive", sync));
        backends.push(onedrive);
    } else {
        info!("OneDrive provider is disabled in configuration.");
    }

    if is_webdav_enabled(&config.webdav_credentials) {
        let sync = config.webdav_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
        let inner = config.webdav_credentials.clone().map(WebDAVProvider::new);
        let local_sim = LocalSimulation::new(config.webdav_root.clone(), "WebDAV".to_string());
        let webdav = Arc::new(SimulatedFallback::new(inner, local_sim, "WebDAV", sync));
        backends.push(webdav);
    } else {
        info!("WebDAV provider is disabled in configuration.");
    }

    if is_s3_enabled(&config.s3_credentials) {
        let sync = config.s3_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
        let inner = config.s3_credentials.clone().map(S3Provider::new);
        let local_sim = LocalSimulation::new(config.s3_root.clone(), "S3".to_string());
        let s3_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "S3", sync));
        backends.push(s3_backend);
    } else {
        info!("S3 provider is disabled in configuration.");
    }

    if is_sftp_enabled(&config.sftp_credentials) {
        let sync = config.sftp_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
        let inner = config.sftp_credentials.clone().map(SFTPProvider::new);
        let local_sim = LocalSimulation::new(config.sftp_root.clone(), "SFTP".to_string());
        let sftp_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "SFTP", sync));
        backends.push(sftp_backend);
    } else {
        info!("SFTP provider is disabled in configuration.");
    }

    if is_nextcloud_enabled(&config.nextcloud_credentials) {
        let sync = config.nextcloud_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
        let inner = config.nextcloud_credentials.clone().map(NextcloudProvider::new);
        let local_sim = LocalSimulation::new(config.nextcloud_root.clone(), "Nextcloud".to_string());
        let nextcloud_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "Nextcloud", sync));
        backends.push(nextcloud_backend);
    } else {
        info!("Nextcloud provider is disabled in configuration.");
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
        let listener = match tokio::net::TcpListener::bind(DAEMON_BIND_ADDR).await {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to bind TCP control socket on {}: {}", DAEMON_BIND_ADDR, e);
                return;
            }
        };
        info!("Control command TCP socket listening on {}", DAEMON_BIND_ADDR);

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
