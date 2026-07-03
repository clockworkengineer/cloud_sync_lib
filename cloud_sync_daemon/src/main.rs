//! # Cloud Sync Daemon
//!
//! A background daemon that monitors a local watched directory for file modifications,
//! creations, and deletions using the `notify` crate, and synchronizes those changes
//! to all configured and enabled cloud storage providers.

pub mod config;
pub mod control;
pub mod watcher;

use cloud_sync_lib::{StorageBackend, SimulatedFallback, LocalSimulation};
#[cfg(feature = "google_drive")]
use cloud_sync_lib::GoogleDriveProvider;
#[cfg(feature = "dropbox")]
use cloud_sync_lib::DropboxProvider;
#[cfg(feature = "onedrive")]
use cloud_sync_lib::OneDriveProvider;
#[cfg(feature = "webdav")]
use cloud_sync_lib::WebDAVProvider;
#[cfg(feature = "s3")]
use cloud_sync_lib::S3Provider;
#[cfg(feature = "sftp")]
use cloud_sync_lib::SFTPProvider;
#[cfg(feature = "nextcloud")]
use cloud_sync_lib::NextcloudProvider;
#[cfg(feature = "box")]
use cloud_sync_lib::BoxProvider;
#[cfg(feature = "mega")]
use cloud_sync_lib::MegaProvider;
#[cfg(feature = "azure_blob")]
use cloud_sync_lib::AzureBlobProvider;
#[cfg(feature = "gcs")]
use cloud_sync_lib::GCSProvider;
#[cfg(feature = "b2")]
use cloud_sync_lib::B2Provider;
#[cfg(feature = "pcloud")]
use cloud_sync_lib::PCloudProvider;
#[cfg(feature = "ipfs")]
use cloud_sync_lib::IPFSProvider;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[allow(unused_imports)]
use config::{
    is_enabled, is_webdav_enabled, is_s3_enabled, is_sftp_enabled, is_nextcloud_enabled, is_mega_enabled, is_azure_blob_enabled, is_gcs_enabled, is_b2_enabled, is_pcloud_enabled, is_ipfs_enabled, load_or_create_config,
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

    #[cfg(feature = "google_drive")]
    {
        if is_enabled(&config.google_credentials) {
            let sync = config.google_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.google_credentials.clone().map(GoogleDriveProvider::new);
            let local_sim = LocalSimulation::new(config.google_drive_root.clone(), "Google Drive".to_string());
            let drive = Arc::new(SimulatedFallback::new(inner, local_sim, "Google Drive", sync));
            backends.push(drive);
        } else {
            info!("Google Drive provider is disabled in configuration.");
        }
    }

    #[cfg(feature = "dropbox")]
    {
        if is_enabled(&config.dropbox_credentials) {
            let sync = config.dropbox_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.dropbox_credentials.clone().map(DropboxProvider::new);
            let local_sim = LocalSimulation::new(config.dropbox_root.clone(), "Dropbox".to_string());
            let dropbox = Arc::new(SimulatedFallback::new(inner, local_sim, "Dropbox", sync));
            backends.push(dropbox);
        } else {
            info!("Dropbox provider is disabled in configuration.");
        }
    }

    #[cfg(feature = "onedrive")]
    {
        if is_enabled(&config.onedrive_credentials) {
            let sync = config.onedrive_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.onedrive_credentials.clone().map(OneDriveProvider::new);
            let local_sim = LocalSimulation::new(config.onedrive_root.clone(), "OneDrive".to_string());
            let onedrive = Arc::new(SimulatedFallback::new(inner, local_sim, "OneDrive", sync));
            backends.push(onedrive);
        } else {
            info!("OneDrive provider is disabled in configuration.");
        }
    }

    #[cfg(feature = "webdav")]
    {
        if is_webdav_enabled(&config.webdav_credentials) {
            let sync = config.webdav_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.webdav_credentials.clone().map(WebDAVProvider::new);
            let local_sim = LocalSimulation::new(config.webdav_root.clone(), "WebDAV".to_string());
            let webdav = Arc::new(SimulatedFallback::new(inner, local_sim, "WebDAV", sync));
            backends.push(webdav);
        } else {
            info!("WebDAV provider is disabled in configuration.");
        }
    }

    #[cfg(feature = "s3")]
    {
        if is_s3_enabled(&config.s3_credentials) {
            let sync = config.s3_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.s3_credentials.clone().map(S3Provider::new);
            let local_sim = LocalSimulation::new(config.s3_root.clone(), "S3".to_string());
            let s3_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "S3", sync));
            backends.push(s3_backend);
        } else {
            info!("S3 provider is disabled in configuration.");
        }
    }

    #[cfg(feature = "sftp")]
    {
        if is_sftp_enabled(&config.sftp_credentials) {
            let sync = config.sftp_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.sftp_credentials.clone().map(SFTPProvider::new);
            let local_sim = LocalSimulation::new(config.sftp_root.clone(), "SFTP".to_string());
            let sftp_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "SFTP", sync));
            backends.push(sftp_backend);
        } else {
            info!("SFTP provider is disabled in configuration.");
        }
    }

    #[cfg(feature = "nextcloud")]
    {
        if is_nextcloud_enabled(&config.nextcloud_credentials) {
            let sync = config.nextcloud_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.nextcloud_credentials.clone().map(NextcloudProvider::new);
            let local_sim = LocalSimulation::new(config.nextcloud_root.clone(), "Nextcloud".to_string());
            let nextcloud_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "Nextcloud", sync));
            backends.push(nextcloud_backend);
        } else {
            info!("Nextcloud provider is disabled in configuration.");
        }
    }

    #[cfg(feature = "box")]
    {
        if is_enabled(&config.box_credentials) {
            let sync = config.box_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.box_credentials.clone().map(BoxProvider::new);
            let box_root = config.box_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_BOX_ROOT));
            let local_sim = LocalSimulation::new(box_root, "Box".to_string());
            let box_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "Box", sync));
            backends.push(box_backend);
        } else {
            info!("Box provider is disabled in configuration.");
        }
    }

    #[cfg(feature = "mega")]
    {
        if is_mega_enabled(&config.mega_credentials) {
            let sync = config.mega_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.mega_credentials.clone().map(MegaProvider::new);
            let mega_root = config.mega_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_MEGA_ROOT));
            let local_sim = LocalSimulation::new(mega_root, "MEGA".to_string());
            let mega_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "MEGA", sync));
            backends.push(mega_backend);
        } else {
            info!("MEGA provider is disabled in configuration.");
        }
    }
    #[cfg(feature = "azure_blob")]
    {
        if is_azure_blob_enabled(&config.azure_blob_credentials) {
            let sync = config.azure_blob_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.azure_blob_credentials.clone().map(AzureBlobProvider::new);
            let azure_blob_root = config.azure_blob_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_AZURE_BLOB_ROOT));
            let local_sim = LocalSimulation::new(azure_blob_root, "Azure Blob".to_string());
            let azure_blob_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "Azure Blob", sync));
            backends.push(azure_blob_backend);
        } else {
            info!("Azure Blob provider is disabled in configuration.");
        }
    }
    #[cfg(feature = "gcs")]
    {
        if is_gcs_enabled(&config.gcs_credentials) {
            let sync = config.gcs_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.gcs_credentials.clone().map(GCSProvider::new);
            let gcs_root = config.gcs_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_GCS_ROOT));
            let local_sim = LocalSimulation::new(gcs_root, "GCS".to_string());
            let gcs_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "GCS", sync));
            backends.push(gcs_backend);
        } else {
            info!("GCS provider is disabled in configuration.");
        }
    }
    #[cfg(feature = "b2")]
    {
        if is_b2_enabled(&config.b2_credentials) {
            let sync = config.b2_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.b2_credentials.clone().map(B2Provider::new);
            let b2_root = config.b2_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_B2_ROOT));
            let local_sim = LocalSimulation::new(b2_root, "B2".to_string());
            let b2_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "B2", sync));
            backends.push(b2_backend);
        } else {
            info!("B2 provider is disabled in configuration.");
        }
    }
    #[cfg(feature = "pcloud")]
    {
        if is_pcloud_enabled(&config.pcloud_credentials) {
            let sync = config.pcloud_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.pcloud_credentials.clone().map(PCloudProvider::new);
            let pcloud_root = config.pcloud_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_PCLOUD_ROOT));
            let local_sim = LocalSimulation::new(pcloud_root, "pCloud".to_string());
            let pcloud_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "pCloud", sync));
            backends.push(pcloud_backend);
        } else {
            info!("pCloud provider is disabled in configuration.");
        }
    }
    #[cfg(feature = "ipfs")]
    {
        if is_ipfs_enabled(&config.ipfs_credentials) {
            let sync = config.ipfs_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
            let inner = config.ipfs_credentials.clone().map(IPFSProvider::new);
            let ipfs_root = config.ipfs_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_IPFS_ROOT));
            let local_sim = LocalSimulation::new(ipfs_root, "IPFS".to_string());
            let ipfs_backend = Arc::new(SimulatedFallback::new(inner, local_sim, "IPFS", sync));
            backends.push(ipfs_backend);
        } else {
            info!("IPFS provider is disabled in configuration.");
        }
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
