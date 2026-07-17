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

#[derive(Clone)]
pub struct ActiveBackend {
    pub backend: Arc<dyn StorageBackend>,
    pub policy: cloud_sync_lib::SyncPolicy,
    pub selective_sync: Option<Vec<String>>,
}

/// Internal state of the daemon.
pub struct DaemonState {
    /// True if the sync operations are temporarily paused.
    pub paused: bool,
    /// Loaded list of active cloud storage backends.
    pub backends: Vec<ActiveBackend>,
    /// Path of the watched directory.
    pub watch_dir: PathBuf,
    /// Path of the configuration TOML file.
    pub config_file: String,
    /// True if a manual or automatic synchronization is actively running.
    pub syncing: bool,
    /// The address of the Web UI server, if provided.
    pub ui_addr: Option<String>,
    /// Gitignore pattern matcher for exclusions.
    pub gitignore: cloud_sync_lib::SyncIgnore,
    /// Copy of the current exclude configurations.
    pub exclude: Option<Vec<String>>,
    /// Optional rate limiter for uploads.
    pub upload_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    /// Optional rate limiter for downloads.
    pub download_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    /// Maximum concurrent workers for synchronization.
    pub max_concurrency: usize,
    /// Map of backend name to its last connection error message (if any).
    pub connection_errors: HashMap<String, String>,
    /// Selected conflict resolution strategy.
    pub conflict_policy: cloud_sync_lib::ConflictPolicy,
    /// If true, runs synchronization in dry-run mode.
    pub dry_run: bool,
    /// Event transmitter for real-time SSE updates.
    pub event_tx: tokio::sync::broadcast::Sender<String>,
}

#[allow(dead_code)]
fn try_add_backend<C, F>(
    backends: &mut Vec<ActiveBackend>,
    creds_option: &Option<C>,
    sim_root: PathBuf,
    provider_name: &str,
    upload_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    download_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    build_provider: F,
) where
    C: ProviderConfig + Clone + 'static,
    F: FnOnce(C, Option<cloud_sync_lib::rate_limit::TokenBucket>, Option<cloud_sync_lib::rate_limit::TokenBucket>) -> Arc<dyn StorageBackend>,
{
    if is_provider_enabled(creds_option) {
        let sync_mode = creds_option.as_ref().map(|c| c.sync_mode()).unwrap_or(cloud_sync_lib::SyncMode::OneWay);
        
        let provider_upload_limiter = creds_option.as_ref()
            .and_then(|c| c.max_upload_rate())
            .map(|rate| cloud_sync_lib::rate_limit::TokenBucket::new(rate * 1024))
            .or_else(|| upload_limiter.clone());

        let provider_download_limiter = creds_option.as_ref()
            .and_then(|c| c.max_download_rate())
            .map(|rate| cloud_sync_lib::rate_limit::TokenBucket::new(rate * 1024))
            .or_else(|| download_limiter.clone());

        let inner = creds_option.clone()
            .map(|creds| build_provider(creds, provider_upload_limiter.clone(), provider_download_limiter.clone()));

        let local_sim = LocalSimulation::new(sim_root, provider_name.to_string())
            .with_limiters(provider_upload_limiter, provider_download_limiter);
        let fallback = SimulatedFallback::new(inner, local_sim, provider_name, sync_mode);

        let selective_sync = creds_option.as_ref().and_then(|c| c.selective_sync());
        let backend: Arc<dyn StorageBackend> = if let Some(password) = creds_option.as_ref().and_then(|c| c.encryption_password()) {
            Arc::new(cloud_sync_lib::EncryptedBackend::new(fallback, password))
        } else {
            Arc::new(fallback)
        };
        backends.push(ActiveBackend {
            backend,
            policy: cloud_sync_lib::SyncPolicy::new(sync_mode),
            selective_sync,
        });
    } else {
        info!("{} provider is disabled in configuration.", provider_name);
    }
}

/// Builds the active storage backends and configures their rate limiters.
#[allow(unused_variables, unused_mut)]
pub fn build_backends(
    config: &config::AppConfig,
    upload_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    download_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
) -> Vec<ActiveBackend> {
    let mut backends: Vec<ActiveBackend> = Vec::new();

    #[cfg(feature = "google_drive")]
    try_add_backend(
        &mut backends,
        &config.google_credentials,
        config.google_drive_root.clone(),
        "Google Drive",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, ul, dl| Arc::new(GoogleDriveProvider::new(creds).with_limiters(ul, dl)),
    );

    #[cfg(feature = "dropbox")]
    try_add_backend(
        &mut backends,
        &config.dropbox_credentials,
        config.dropbox_root.clone(),
        "Dropbox",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, ul, dl| Arc::new(DropboxProvider::new(creds).with_limiters(ul, dl)),
    );

    #[cfg(feature = "onedrive")]
    try_add_backend(
        &mut backends,
        &config.onedrive_credentials,
        config.onedrive_root.clone(),
        "OneDrive",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, ul, dl| Arc::new(OneDriveProvider::new(creds).with_limiters(ul, dl)),
    );

    #[cfg(feature = "webdav")]
    try_add_backend(
        &mut backends,
        &config.webdav_credentials,
        config.webdav_root.clone(),
        "WebDAV",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, ul, dl| Arc::new(WebDAVProvider::new(creds).with_limiters(ul, dl)),
    );

    #[cfg(feature = "s3")]
    try_add_backend(
        &mut backends,
        &config.s3_credentials,
        config.s3_root.clone(),
        "S3",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(S3Provider::new(creds)),
    );

    #[cfg(feature = "sftp")]
    try_add_backend(
        &mut backends,
        &config.sftp_credentials,
        config.sftp_root.clone(),
        "SFTP",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(SFTPProvider::new(creds)),
    );

    #[cfg(feature = "nextcloud")]
    try_add_backend(
        &mut backends,
        &config.nextcloud_credentials,
        config.nextcloud_root.clone(),
        "Nextcloud",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(NextcloudProvider::new(creds)),
    );

    #[cfg(feature = "box")]
    try_add_backend(
        &mut backends,
        &config.box_credentials,
        config.box_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_BOX_ROOT)),
        "Box",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(BoxProvider::new(creds)),
    );

    #[cfg(feature = "mega")]
    try_add_backend(
        &mut backends,
        &config.mega_credentials,
        config.mega_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_MEGA_ROOT)),
        "MEGA",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(MegaProvider::new(creds)),
    );

    #[cfg(feature = "azure_blob")]
    try_add_backend(
        &mut backends,
        &config.azure_blob_credentials,
        config.azure_blob_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_AZURE_BLOB_ROOT)),
        "Azure Blob",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(AzureBlobProvider::new(creds)),
    );

    #[cfg(feature = "gcs")]
    try_add_backend(
        &mut backends,
        &config.gcs_credentials,
        config.gcs_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_GCS_ROOT)),
        "GCS",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(GCSProvider::new(creds)),
    );

    #[cfg(feature = "b2")]
    try_add_backend(
        &mut backends,
        &config.b2_credentials,
        config.b2_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_B2_ROOT)),
        "B2",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(B2Provider::new(creds)),
    );

    #[cfg(feature = "pcloud")]
    try_add_backend(
        &mut backends,
        &config.pcloud_credentials,
        config.pcloud_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_PCLOUD_ROOT)),
        "pCloud",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(PCloudProvider::new(creds)),
    );

    #[cfg(feature = "ipfs")]
    try_add_backend(
        &mut backends,
        &config.ipfs_credentials,
        config.ipfs_root.clone().unwrap_or_else(|| PathBuf::from(config::DEFAULT_IPFS_ROOT)),
        "IPFS",
        upload_limiter.clone(),
        download_limiter.clone(),
        |creds, _ul, _dl| Arc::new(IPFSProvider::new(creds)),
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
    let mut clear_remote = None;
    let mut single_shot = false;
    let mut pmu_hook = None;
    let mut cli_dry_run = false;
    let mut cli_conflict_policy = None;
    let mut cli_excludes = Vec::new();

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--ui-addr" && i + 1 < args.len() {
            ui_addr = Some(args[i + 1].clone());
            i += 2;
        } else if args[i] == "--clear-remote" && i + 1 < args.len() {
            clear_remote = Some(args[i + 1].clone());
            i += 2;
        } else if args[i] == "--single-shot" || args[i] == "-s" {
            single_shot = true;
            i += 1;
        } else if args[i] == "--pmu-hook" && i + 1 < args.len() {
            pmu_hook = Some(args[i + 1].clone());
            i += 2;
        } else if args[i] == "--dry-run" {
            cli_dry_run = true;
            i += 1;
        } else if args[i] == "--conflict-policy" && i + 1 < args.len() {
            let policy_str = args[i + 1].clone();
            let policy = match policy_str.as_str() {
                "rename-local" => cloud_sync_lib::ConflictPolicy::RenameLocal,
                "rename-remote" => cloud_sync_lib::ConflictPolicy::RenameRemote,
                "keep-newer" => cloud_sync_lib::ConflictPolicy::KeepNewer,
                "keep-local" => cloud_sync_lib::ConflictPolicy::KeepLocal,
                "keep-remote" => cloud_sync_lib::ConflictPolicy::KeepRemote,
                _ => {
                    error!("Unknown conflict policy: '{}'. Using RenameLocal.", policy_str);
                    cloud_sync_lib::ConflictPolicy::RenameLocal
                }
            };
            cli_conflict_policy = Some(policy);
            i += 2;
        } else if args[i] == "--exclude" && i + 1 < args.len() {
            cli_excludes.push(args[i + 1].clone());
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

    // Apply error recovery configuration globally
    if let Some(ref recovery) = config.error_recovery {
        let mut retry_config = cloud_sync_lib::get_global_retry_config();
        if let Some(max_retries) = recovery.max_retries {
            retry_config.max_attempts = max_retries + 1;
        }
        if let Some(delay_ms) = recovery.initial_delay_ms {
            retry_config.initial_delay = std::time::Duration::from_millis(delay_ms);
        }
        if let Some(mult) = recovery.multiplier {
            retry_config.multiplier = mult;
        }
        cloud_sync_lib::set_global_retry_config(retry_config);
        info!("Applied global error recovery configuration: {:?}", retry_config);
    }

    // Ensure the directories exist
    fs::create_dir_all(&config.watch_directory).await?;
    let watch_dir = fs::canonicalize(&config.watch_directory).await?;
    info!("Watching directory: {:?}", watch_dir);

    // Initialize gitignore matcher
    let mut final_excludes = config.exclude.clone().unwrap_or_default();
    final_excludes.extend(cli_excludes);
    let gitignore = watcher::build_gitignore(&watch_dir, &Some(final_excludes.clone()));
    let exclude = Some(final_excludes);

    let conflict_policy = cli_conflict_policy
        .or(config.conflict_policy)
        .unwrap_or(cloud_sync_lib::ConflictPolicy::RenameLocal);

    let dry_run = if cli_dry_run {
        true
    } else {
        config.dry_run.unwrap_or(false)
    };

    // Initialize rate limiters
    let upload_limiter = if config.max_upload_rate.is_some() || config.bandwidth_schedule.is_some() {
        let initial_rate = config.max_upload_rate.unwrap_or(0);
        Some(cloud_sync_lib::rate_limit::TokenBucket::new(initial_rate * 1024))
    } else {
        None
    };
    let download_limiter = if config.max_download_rate.is_some() || config.bandwidth_schedule.is_some() {
        let initial_rate = config.max_download_rate.unwrap_or(0);
        Some(cloud_sync_lib::rate_limit::TokenBucket::new(initial_rate * 1024))
    } else {
        None
    };

    // Initialize Providers
    let backends = build_backends(&config, upload_limiter.clone(), download_limiter.clone());

    let (event_tx, _) = tokio::sync::broadcast::channel::<String>(100);

    if single_shot {
        info!("Running single-shot bidirectional synchronization...");
        for active_backend in &backends {
            let safe_name = active_backend.backend.name().to_lowercase().replace(" ", "_");
            let state_filename = format!(".sync_state_{}.bin", safe_name);
            let state_file_path = watch_dir.join(state_filename);
            info!("Syncing backend: {}", active_backend.backend.name());
            if let Err(e) = sync_engine::sync_bidirectional(
                &watch_dir,
                active_backend.backend.clone(),
                active_backend.policy,
                &state_file_path,
                &gitignore,
                config.max_concurrency.unwrap_or(4),
                conflict_policy,
                dry_run,
                active_backend.selective_sync.clone(),
                Some(event_tx.clone()),
            ).await {
                error!("Bidirectional sync failed for backend '{}': {}", active_backend.backend.name(), e);
            }
        }
        info!("Single-shot synchronization completed.");

        let active_pmu_hook = pmu_hook.or(config.pmu_hook.clone());
        if let Some(hook) = active_pmu_hook {
            info!("Executing PMU completion hook: {}", hook);
            #[cfg(target_os = "windows")]
            let mut cmd = std::process::Command::new("cmd");
            #[cfg(target_os = "windows")]
            cmd.args(&["/C", &hook]);

            #[cfg(not(target_os = "windows"))]
            let mut cmd = std::process::Command::new("sh");
            #[cfg(not(target_os = "windows"))]
            cmd.args(&["-c", &hook]);

            match cmd.status() {
                Ok(status) => info!("PMU hook exited with status: {}", status),
                Err(e) => error!("Failed to execute PMU hook: {}", e),
            }
        }
        return Ok(());
    }

    if let Some(target_provider) = clear_remote {
        let matching_backend = backends.iter().find(|ab| ab.backend.name().eq_ignore_ascii_case(&target_provider));
        match matching_backend {
            Some(active_backend) => {
                let backend = active_backend.backend.clone();
                info!("Clearing all files on remote provider: {}", backend.name());
                let items = backend.list("").await?;
                for item in items {
                    let path_str = item.path.to_string_lossy();
                    info!("Deleting remote item: {}", path_str);
                    if let Err(e) = backend.delete(&path_str).await {
                        error!("Failed to delete remote item '{}': {}", path_str, e);
                    }
                }
                info!("Successfully cleared remote provider: {}", backend.name());
                return Ok(());
            }
            None => {
                error!("No enabled provider found matching name: '{}'", target_provider);
                return Err("Provider not found or disabled".into());
            }
        }
    }

    info!("Active cloud storage backends:");
    for active_backend in &backends {
        info!(" - {}", active_backend.backend.name());
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
        max_concurrency: config.max_concurrency.unwrap_or(4),
        connection_errors: HashMap::new(),
        conflict_policy,
        dry_run,
        event_tx,
    }));

    // Spawn background dynamic rate limiter task if a schedule is defined
    if let Some(ref schedule) = config.bandwidth_schedule {
        let schedule_clone = schedule.clone();
        let state_limit = state.clone();
        let default_upload = config.max_upload_rate.unwrap_or(0);
        let default_download = config.max_download_rate.unwrap_or(0);

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                let now = chrono::Local::now().time();
                
                // Find matching schedule
                let mut active_upload = None;
                let mut active_download = None;

                for window in &schedule_clone {
                    let start_parts: Vec<&str> = window.start_time.split(':').collect();
                    let end_parts: Vec<&str> = window.end_time.split(':').collect();

                    if start_parts.len() == 2 && end_parts.len() == 2 {
                        if let (Ok(sh), Ok(sm), Ok(eh), Ok(em)) = (
                            start_parts[0].parse::<u32>(),
                            start_parts[1].parse::<u32>(),
                            end_parts[0].parse::<u32>(),
                            end_parts[1].parse::<u32>(),
                        ) {
                            if let (Some(start_time), Some(end_time)) = (
                                chrono::NaiveTime::from_hms_opt(sh, sm, 0),
                                chrono::NaiveTime::from_hms_opt(eh, em, 0),
                            ) {
                                let in_window = if start_time <= end_time {
                                    now >= start_time && now <= end_time
                                } else {
                                    now >= start_time || now <= end_time
                                };

                                if in_window {
                                    active_upload = window.max_upload_rate;
                                    active_download = window.max_download_rate;
                                    break;
                                }
                            }
                        }
                    }
                }

                let target_upload = active_upload.unwrap_or(default_upload);
                let target_download = active_download.unwrap_or(default_download);

                let s = state_limit.lock().await;
                if let Some(ref limiter) = s.upload_limiter {
                    let cur_rate = limiter.rate() / 1024;
                    if cur_rate != target_upload {
                        info!("Dynamic Scheduler: Adjusting upload limit to {} KB/s", target_upload);
                        limiter.set_rate(target_upload * 1024);
                    }
                }
                if let Some(ref limiter) = s.download_limiter {
                    let cur_rate = limiter.rate() / 1024;
                    if cur_rate != target_download {
                        info!("Dynamic Scheduler: Adjusting download limit to {} KB/s", target_download);
                        limiter.set_rate(target_download * 1024);
                    }
                }
            }
        });
    }

    // Spawn periodic backend health check task
    let state_health = state.clone();
    tokio::spawn(async move {
        loop {
            info!("Backend health check started.");
            let backends = {
                let s = state_health.lock().await;
                s.backends.clone()
            };

            for active_backend in backends {
                let name = active_backend.backend.name().to_string();
                info!("Checking health of provider: {}", name);
                match active_backend.backend.list("").await {
                    Ok(_) => {
                        let mut s = state_health.lock().await;
                        s.connection_errors.remove(&name);
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        error!("Health check failed for provider '{}': {}", name, err_str);
                        let mut s = state_health.lock().await;
                        s.connection_errors.insert(name, err_str);
                    }
                }
            }
            info!("Backend health check finished.");

            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    });

    // Set up mpsc channel for events
    let (tx, mut rx) = mpsc::channel::<notify::Result<Event>>(100);

    // Set up file watcher
    let rt = tokio::runtime::Handle::current();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            rt.block_on(async {
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
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(pull_interval)).await;

            let (paused, backends, watch_dir, gitignore, max_concurrency, conflict_policy, dry_run, event_tx_pull) = {
                let s = state_pull.lock().await;
                (s.paused, s.backends.clone(), s.watch_dir.clone(), s.gitignore.clone(), s.max_concurrency, s.conflict_policy, s.dry_run, s.event_tx.clone())
            };

            if paused {
                continue;
            }

            info!("Periodic bidirectional sync started...");
            for active_backend in &backends {
                let safe_name = active_backend.backend.name().to_lowercase().replace(" ", "_");
                let state_filename = format!(".sync_state_{}.bin", safe_name);
                let state_file_path = watch_dir.join(state_filename);
                if let Err(e) = sync_engine::sync_bidirectional(
                    &watch_dir,
                    active_backend.backend.clone(),
                    active_backend.policy,
                    &state_file_path,
                    &gitignore,
                    max_concurrency,
                    conflict_policy,
                    dry_run,
                    active_backend.selective_sync.clone(),
                    Some(event_tx_pull.clone()),
                ).await {
                    error!("Bidirectional sync failed for backend '{}': {}", active_backend.backend.name(), e);
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
                                 if cmd == "subscribe" {
                                     let mut rx = {
                                         let s = state.lock().await;
                                         s.event_tx.subscribe()
                                     };
                                     let _ = writer.write_all(b"Status: Subscribed\n").await;
                                     let _ = writer.flush().await;
                                     while let Ok(msg) = rx.recv().await {
                                         if writer.write_all(format!("{}\n", msg).as_bytes()).await.is_err() {
                                             break;
                                         }
                                         if writer.flush().await.is_err() {
                                             break;
                                         }
                                     }
                                 } else {
                                     let response = handle_control_command(cmd, &state, &shutdown_tx).await;
                                     let _ = writer.write_all(response.as_bytes()).await;
                                     let _ = writer.flush().await;
                                 }
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
