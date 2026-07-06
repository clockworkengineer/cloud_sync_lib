//! Control socket server command processing.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};

use crate::DaemonState;
use crate::config::load_or_create_config;
use crate::watcher::trigger_full_sync;

/// Handles a string command sent from the daemon controller via TCP.
///
/// # Arguments
/// * `cmd` - Command string (e.g., "status", "pause", "resume", "reload", "sync", "stop").
/// * `state` - The daemon's internal state.
/// * `shutdown_tx` - Send channel for shutdown signals.
///
/// # Returns
/// A response string to be sent back to the TCP client.
pub async fn handle_control_command(
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
                    let upload_limiter = config.max_upload_rate.map(|rate| {
                        cloud_sync_lib::rate_limit::TokenBucket::new(rate * 1024)
                    });
                    let download_limiter = config.max_download_rate.map(|rate| {
                        cloud_sync_lib::rate_limit::TokenBucket::new(rate * 1024)
                    });
                    let backends = crate::build_backends(&config, upload_limiter.clone(), download_limiter.clone());
                    s.backends = backends;
                    s.upload_limiter = upload_limiter;
                    s.download_limiter = download_limiter;
                    s.exclude = config.exclude.clone();
                    s.gitignore = crate::watcher::build_gitignore(&s.watch_dir, &config.exclude);
                    info!("Configuration reloaded successfully. Active backends and rate limits updated.");
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
                let gitignore = s.gitignore.clone();
                let state_clone = state.clone();
                tokio::spawn(async move {
                    info!("Manual sync triggered via control command. Scanning watch directory...");
                    if let Err(e) = trigger_full_sync(&watch_dir, &backends, &gitignore).await {
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
