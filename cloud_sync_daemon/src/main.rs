//! # Cloud Sync Daemon
//!
//! A background daemon that monitors a local watched directory for file modifications,
//! creations, and deletions using the `notify` crate, and synchronizes those changes
//! to all configured and enabled cloud storage providers.

pub mod config;
pub mod control;
pub mod watcher;
pub mod sync_engine;
pub mod utils;

use cloud_sync_lib::{StorageBackend, SimulatedFallback, LocalSimulation, ProviderConfig};
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
use config::{is_provider_enabled, load_or_create_config, DEFAULT_CONFIG_FILE};
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
    /// Gitignore pattern matcher for exclusions.
    pub gitignore: ignore::gitignore::Gitignore,
    /// Copy of the current exclude configurations.
    pub exclude: Option<Vec<String>>,
    /// Optional rate limiter for uploads.
    pub upload_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    /// Optional rate limiter for downloads.
    pub download_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
}

fn try_add_backend<C, P, F>(
    backends: &mut Vec<Arc<dyn StorageBackend>>,
    creds_option: &Option<C>,
    sim_root: PathBuf,
    provider_name: &str,
    upload_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    download_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    builder: F,
) where
    C: ProviderConfig + Clone + 'static,
    P: StorageBackend + 'static,
    F: FnOnce(C) -> P,
{
    if is_provider_enabled(creds_option) {
        let sync = creds_option.as_ref().map(|c| c.sync_deletions()).unwrap_or(true);
        let inner = creds_option.clone().map(builder);
        let local_sim = LocalSimulation::new(sim_root, provider_name.to_string())
            .with_limiters(upload_limiter, download_limiter);
        let fallback = SimulatedFallback::new(inner, local_sim, provider_name, sync);

        if let Some(password) = creds_option.as_ref().and_then(|c| c.encryption_password()) {
            backends.push(Arc::new(cloud_sync_lib::EncryptedBackend::new(fallback, password)));
        } else {
            backends.push(Arc::new(fallback));
        }
    } else {
        info!("{} provider is disabled in configuration.", provider_name);
    }
}

/// Builds the active storage backends and configures their rate limiters.
pub fn build_backends(
    config: &config::AppConfig,
    upload_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    download_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
) -> Vec<Arc<dyn StorageBackend>> {
    let mut backends: Vec<Arc<dyn StorageBackend>> = Vec::new();

    #[cfg(feature = "google_drive")]
    try_add_backend(
        &mut backends,
        &config.google_credentials,
        config.google_drive_root.clone(),
        "Google Drive",
        upload_limiter.clone(),
        download_limiter.clone(),
        GoogleDriveProvider::new,
    );

    #[cfg(feature = "dropbox")]
    try_add_backend(
        &mut backends,
        &config.dropbox_credentials,
        config.dropbox_root.clone(),
        "Dropbox",
        upload_limiter.clone(),
        download_limiter.clone(),
        DropboxProvider::new,
    );

    #[cfg(feature = "onedrive")]
    try_add_backend(
        &mut backends,
        &config.onedrive_credentials,
        config.onedrive_root.clone(),
        "OneDrive",
        upload_limiter.clone(),
        download_limiter.clone(),
        OneDriveProvider::new,
    );

    #[cfg(feature = "webdav")]
    try_add_backend(
        &mut backends,
        &config.webdav_credentials,
        config.webdav_root.clone(),
        "WebDAV",
        upload_limiter.clone(),
        download_limiter.clone(),
        WebDAVProvider::new,
    );

    #[cfg(feature = "s3")]
    try_add_backend(
        &mut backends,
        &config.s3_credentials,
        config.s3_root.clone(),
        "S3",
        upload_limiter.clone(),
        download_limiter.clone(),
        S3Provider::new,
    );

    #[cfg(feature = "sftp")]
    try_add_backend(
        &mut backends,
        &config.sftp_credentials,
        config.sftp_root.clone(),
        "SFTP",
        upload_limiter.clone(),
        download_limiter.clone(),
        SFTPProvider::new,
    );

    #[cfg(feature = "nextcloud")]
    try_add_backend(
        &mut backends,
        &config.nextcloud_credentials,
        config.nextcloud_root.clone(),
        "Nextcloud",
        upload_limiter.clone(),
        download_limiter.clone(),
        NextcloudProvider::new,
    );

    #[cfg(feature = "box")]
    try_add_backend(
        &mut backends,
        &config.box_credentials,
        config.box_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_BOX_ROOT)),
        "Box",
        upload_limiter.clone(),
        download_limiter.clone(),
        BoxProvider::new,
    );

    #[cfg(feature = "mega")]
    try_add_backend(
        &mut backends,
        &config.mega_credentials,
        config.mega_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_MEGA_ROOT)),
        "MEGA",
        upload_limiter.clone(),
        download_limiter.clone(),
        MegaProvider::new,
    );

    #[cfg(feature = "azure_blob")]
    try_add_backend(
        &mut backends,
        &config.azure_blob_credentials,
        config.azure_blob_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_AZURE_BLOB_ROOT)),
        "Azure Blob",
        upload_limiter.clone(),
        download_limiter.clone(),
        AzureBlobProvider::new,
    );

    #[cfg(feature = "gcs")]
    try_add_backend(
        &mut backends,
        &config.gcs_credentials,
        config.gcs_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_GCS_ROOT)),
        "GCS",
        upload_limiter.clone(),
        download_limiter.clone(),
        GCSProvider::new,
    );

    #[cfg(feature = "b2")]
    try_add_backend(
        &mut backends,
        &config.b2_credentials,
        config.b2_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_B2_ROOT)),
        "B2",
        upload_limiter.clone(),
        download_limiter.clone(),
        B2Provider::new,
    );

    #[cfg(feature = "pcloud")]
    try_add_backend(
        &mut backends,
        &config.pcloud_credentials,
        config.pcloud_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_PCLOUD_ROOT)),
        "pCloud",
        upload_limiter.clone(),
        download_limiter.clone(),
        PCloudProvider::new,
    );

    #[cfg(feature = "ipfs")]
    try_add_backend(
        &mut backends,
        &config.ipfs_credentials,
        config.ipfs_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_IPFS_ROOT)),
        "IPFS",
        upload_limiter.clone(),
        download_limiter.clone(),
        IPFSProvider::new,
    );

    backends
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

    // Initialize gitignore matcher
    let gitignore = watcher::build_gitignore(&watch_dir, &config.exclude);
    let exclude = config.exclude.clone();

    // Initialize rate limiters
    let upload_limiter = config.max_upload_rate.map(|rate| {
        cloud_sync_lib::rate_limit::TokenBucket::new(rate * 1024)
    });
    let download_limiter = config.max_download_rate.map(|rate| {
        cloud_sync_lib::rate_limit::TokenBucket::new(rate * 1024)
    });

    // Initialize Providers
    let backends = build_backends(&config, upload_limiter.clone(), download_limiter.clone());

    info!("Active cloud storage backends:");
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
        gitignore,
        exclude,
        upload_limiter,
        download_limiter,
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

    // Spawn periodic remote pull synchronization loop
    let pull_interval = config.pull_interval_secs.unwrap_or(30);
    let state_pull = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(pull_interval));
        interval.tick().await; // skip immediate first tick

        loop {
            interval.tick().await;

            let (paused, backends, watch_dir, gitignore) = {
                let s = state_pull.lock().await;
                (s.paused, s.backends.clone(), s.watch_dir.clone(), s.gitignore.clone())
            };

            if paused {
                continue;
            }

            info!("Periodic bidirectional sync started...");
            let state_file_path = watch_dir.join(".sync_state.json");
            for backend in &backends {
                if let Err(e) = sync_engine::sync_bidirectional(
                    &watch_dir,
                    backend.as_ref(),
                    &state_file_path,
                    &gitignore,
                ).await {
                    error!("Bidirectional sync failed for backend '{}': {}", backend.name(), e);
                }
            }
            info!("Periodic bidirectional sync completed.");
        }
    });

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
