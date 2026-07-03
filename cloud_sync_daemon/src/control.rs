//! Control socket server command processing.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
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

use crate::DaemonState;
#[allow(unused_imports)]
use crate::config::{
    is_enabled, is_webdav_enabled, is_s3_enabled, is_sftp_enabled, is_nextcloud_enabled, is_mega_enabled, is_azure_blob_enabled, is_gcs_enabled, load_or_create_config
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
                    #[cfg(feature = "google_drive")]
                    {
                        if is_enabled(&config.google_credentials) {
                            let sync = config.google_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.google_credentials.clone().map(GoogleDriveProvider::new);
                            let local_sim = LocalSimulation::new(config.google_drive_root.clone(), "Google Drive".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "Google Drive", sync)));
                        }
                    }
                    #[cfg(feature = "dropbox")]
                    {
                        if is_enabled(&config.dropbox_credentials) {
                            let sync = config.dropbox_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.dropbox_credentials.clone().map(DropboxProvider::new);
                            let local_sim = LocalSimulation::new(config.dropbox_root.clone(), "Dropbox".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "Dropbox", sync)));
                        }
                    }
                    #[cfg(feature = "onedrive")]
                    {
                        if is_enabled(&config.onedrive_credentials) {
                            let sync = config.onedrive_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.onedrive_credentials.clone().map(OneDriveProvider::new);
                            let local_sim = LocalSimulation::new(config.onedrive_root.clone(), "OneDrive".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "OneDrive", sync)));
                        }
                    }
                    #[cfg(feature = "webdav")]
                    {
                        if is_webdav_enabled(&config.webdav_credentials) {
                            let sync = config.webdav_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.webdav_credentials.clone().map(WebDAVProvider::new);
                            let local_sim = LocalSimulation::new(config.webdav_root.clone(), "WebDAV".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "WebDAV", sync)));
                        }
                    }
                    #[cfg(feature = "s3")]
                    {
                        if is_s3_enabled(&config.s3_credentials) {
                            let sync = config.s3_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.s3_credentials.clone().map(S3Provider::new);
                            let local_sim = LocalSimulation::new(config.s3_root.clone(), "S3".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "S3", sync)));
                        }
                    }
                    #[cfg(feature = "sftp")]
                    {
                        if is_sftp_enabled(&config.sftp_credentials) {
                            let sync = config.sftp_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.sftp_credentials.clone().map(SFTPProvider::new);
                            let local_sim = LocalSimulation::new(config.sftp_root.clone(), "SFTP".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "SFTP", sync)));
                        }
                    }
                    #[cfg(feature = "nextcloud")]
                    {
                        if is_nextcloud_enabled(&config.nextcloud_credentials) {
                            let sync = config.nextcloud_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.nextcloud_credentials.clone().map(NextcloudProvider::new);
                            let local_sim = LocalSimulation::new(config.nextcloud_root.clone(), "Nextcloud".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "Nextcloud", sync)));
                        }
                    }
                    #[cfg(feature = "box")]
                    {
                        if is_enabled(&config.box_credentials) {
                            let sync = config.box_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.box_credentials.clone().map(BoxProvider::new);
                            let box_root = config.box_root.clone().unwrap_or_else(|| std::path::PathBuf::from(crate::config::DEFAULT_BOX_ROOT));
                            let local_sim = LocalSimulation::new(box_root, "Box".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "Box", sync)));
                        }
                    }
                    #[cfg(feature = "mega")]
                    {
                        if is_mega_enabled(&config.mega_credentials) {
                            let sync = config.mega_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.mega_credentials.clone().map(MegaProvider::new);
                            let mega_root = config.mega_root.clone().unwrap_or_else(|| std::path::PathBuf::from(crate::config::DEFAULT_MEGA_ROOT));
                            let local_sim = LocalSimulation::new(mega_root, "MEGA".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "MEGA", sync)));
                        }
                    }
                    #[cfg(feature = "azure_blob")]
                    {
                        if is_azure_blob_enabled(&config.azure_blob_credentials) {
                            let sync = config.azure_blob_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.azure_blob_credentials.clone().map(AzureBlobProvider::new);
                            let azure_blob_root = config.azure_blob_root.clone().unwrap_or_else(|| std::path::PathBuf::from(crate::config::DEFAULT_AZURE_BLOB_ROOT));
                            let local_sim = LocalSimulation::new(azure_blob_root, "Azure Blob".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "Azure Blob", sync)));
                        }
                    }
                    #[cfg(feature = "gcs")]
                    {
                        if is_gcs_enabled(&config.gcs_credentials) {
                            let sync = config.gcs_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
                            let inner = config.gcs_credentials.clone().map(GCSProvider::new);
                            let gcs_root = config.gcs_root.clone().unwrap_or_else(|| std::path::PathBuf::from(crate::config::DEFAULT_GCS_ROOT));
                            let local_sim = LocalSimulation::new(gcs_root, "GCS".to_string());
                            backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "GCS", sync)));
                        }
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
