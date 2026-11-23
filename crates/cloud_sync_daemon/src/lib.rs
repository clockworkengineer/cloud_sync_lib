//! # Cloud Sync Core Engine & Daemon Library
//!
//! Exposes synchronization engines, file watchers, conflict resolvers,
//! configuration management, and storage backend orchestration for both
//! in-process execution (e.g. mobile apps, embedded runtimes) and external
//! standalone daemon processes.

pub mod config;
pub mod control;
pub mod engine;
pub mod utils;
pub mod watcher;

use cloud_sync_lib::{BackendCredentials, BackendRegistry, StorageBackend};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info};

#[allow(unused_imports)]
use config::{is_provider_enabled, DEFAULT_CONFIG_FILE};

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

/// Internal state of the synchronization runtime.
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
    /// Manager for tracking and resolving pending conflicts interactively.
    pub conflict_manager: crate::engine::conflict::ConflictManager,
    /// Path of the active log file.
    pub log_file: String,
    /// Directory where synchronization state databases are stored.
    pub state_dir: PathBuf,
}

#[allow(dead_code)]
pub fn try_add_backend(
    backends: &mut Vec<ActiveBackend>,
    creds_option: Option<BackendCredentials>,
    sim_root: PathBuf,
    upload_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    download_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
) {
    if let Some(creds) = creds_option {
        let sync_mode = creds.sync_mode();
        let selective_sync = creds.selective_sync();

        let backend =
            BackendRegistry::build_wrapped(creds, sim_root, upload_limiter, download_limiter);

        backends.push(ActiveBackend {
            backend,
            policy: cloud_sync_lib::SyncPolicy::new(sync_mode),
            selective_sync,
        });
    }
}

/// Builds the active storage backends and configures their rate limiters using `AppConfig::enabled_credentials` (OCP).
pub fn build_backends(
    config: &config::AppConfig,
    upload_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    download_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
) -> Vec<ActiveBackend> {
    let mut backends: Vec<ActiveBackend> = Vec::new();
    for (creds, sim_root) in config.enabled_credentials() {
        try_add_backend(
            &mut backends,
            Some(creds),
            sim_root,
            upload_limiter.clone(),
            download_limiter.clone(),
        );
    }
    backends
}

/// Configuration options for executing synchronization cycles in-process.
#[derive(Clone, Copy, Debug)]
pub struct InProcessSyncOptions {
    pub max_concurrency: usize,
    pub conflict_policy: cloud_sync_lib::ConflictPolicy,
    pub dry_run: bool,
}

impl Default for InProcessSyncOptions {
    fn default() -> Self {
        Self {
            max_concurrency: 4,
            conflict_policy: cloud_sync_lib::ConflictPolicy::RenameLocal,
            dry_run: false,
        }
    }
}

/// Runs a single complete synchronization cycle across all active backends in-process.
pub async fn run_sync_all_backends(
    watch_dir: &Path,
    state_dir: &Path,
    backends: &[ActiveBackend],
    gitignore: &cloud_sync_lib::SyncIgnore,
    options: InProcessSyncOptions,
    event_tx: Option<tokio::sync::broadcast::Sender<String>>,
) {
    for active_backend in backends {
        let safe_name = active_backend
            .backend
            .name()
            .to_lowercase()
            .replace(' ', "_");
        let state_file_path =
            engine::resolve_state_file_path(state_dir, watch_dir, &safe_name).await;
        info!(
            "In-process sync: Starting backend {}",
            active_backend.backend.name()
        );

        if let Err(e) = engine::sync_bidirectional(
            watch_dir,
            active_backend.backend.clone(),
            active_backend.policy,
            &state_file_path,
            gitignore,
            options.max_concurrency,
            options.conflict_policy,
            options.dry_run,
            active_backend.selective_sync.clone(),
            event_tx.clone(),
        )
        .await
        {
            error!(
                "In-process sync failed for backend '{}': {}",
                active_backend.backend.name(),
                e
            );
        }
    }
}
