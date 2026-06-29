//! Control socket server command processing.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
use cloud_sync_lib::{
    DropboxProvider, GoogleDriveProvider, OneDriveProvider, WebDAVProvider, S3Provider,
    StorageBackend, SimulatedFallback, LocalSimulation
};

use crate::DaemonState;
use crate::config::{
    is_enabled, is_webdav_enabled, is_s3_enabled, load_or_create_config
};
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
