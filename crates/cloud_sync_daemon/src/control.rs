//! Control socket server command processing.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};

use crate::DaemonState;
use crate::config::load_or_create_config;

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
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return "Error: Empty command\n".to_string();
    }
    match parts[0] {
        "status" => {
            let s = state.lock().await;
            let backend_names: Vec<String> = s.backends.iter().map(|ab| ab.backend.name().to_string()).collect();
            
            let mut failed_backends: Vec<String> = s.connection_errors.keys().map(|k| {
                if k.starts_with("Encrypted(") && k.ends_with(')') {
                    k["Encrypted(".len()..k.len() - 1].to_string()
                } else {
                    k.clone()
                }
            }).collect();

            if let Ok(config) = load_or_create_config(&s.config_file).await {
                let is_placeholder = |val: &str| -> bool {
                    val.is_empty() || val.contains("PLACEHOLDER") || val.contains("your_") || val.contains("your-")
                };

                macro_rules! add_if_placeholder_enabled {
                    ($name:expr, $creds:expr, $token:expr) => {
                        if crate::config::is_provider_enabled($creds) && is_placeholder($token) && !failed_backends.contains(&$name.to_string()) {
                            failed_backends.push($name.to_string());
                        }
                    };
                }

                if let Some(ref creds) = config.google_credentials { add_if_placeholder_enabled!("Google Drive", &config.google_credentials, &creds.refresh_token); }
                if let Some(ref creds) = config.dropbox_credentials { add_if_placeholder_enabled!("Dropbox", &config.dropbox_credentials, &creds.refresh_token); }
                if let Some(ref creds) = config.onedrive_credentials { add_if_placeholder_enabled!("OneDrive", &config.onedrive_credentials, &creds.refresh_token); }
                if let Some(ref creds) = config.box_credentials { add_if_placeholder_enabled!("Box", &config.box_credentials, &creds.refresh_token); }
                if let Some(ref creds) = config.mega_credentials { add_if_placeholder_enabled!("MEGA", &config.mega_credentials, &creds.password); }
                if let Some(ref creds) = config.webdav_credentials { add_if_placeholder_enabled!("WebDAV", &config.webdav_credentials, &creds.password); }
                if let Some(ref creds) = config.s3_credentials { add_if_placeholder_enabled!("S3", &config.s3_credentials, &creds.secret_access_key); }
                if let Some(ref creds) = config.sftp_credentials {
                    if crate::config::is_provider_enabled(&config.sftp_credentials) {
                        let pw_placeholder = creds.password.as_ref().is_some_and(|p| is_placeholder(p));
                        let key_placeholder = creds.private_key_path.as_ref().is_none_or(|k| is_placeholder(k));
                        if pw_placeholder && key_placeholder && !failed_backends.contains(&"SFTP".to_string()) {
                            failed_backends.push("SFTP".to_string());
                        }
                    }
                }
                if let Some(ref creds) = config.nextcloud_credentials { add_if_placeholder_enabled!("Nextcloud", &config.nextcloud_credentials, &creds.app_password); }
                if let Some(ref creds) = config.azure_blob_credentials {
                    if crate::config::is_provider_enabled(&config.azure_blob_credentials) && (is_placeholder(&creds.account_key) || creds.account_key == "devstoreaccount1") && !failed_backends.contains(&"Azure Blob".to_string()) {
                        failed_backends.push("Azure Blob".to_string());
                    }
                }
                if let Some(ref creds) = config.gcs_credentials { add_if_placeholder_enabled!("Google Cloud Storage", &config.gcs_credentials, &creds.service_account_key_path); }
                if let Some(ref creds) = config.b2_credentials { add_if_placeholder_enabled!("Backblaze B2", &config.b2_credentials, &creds.application_key); }
                if let Some(ref creds) = config.pcloud_credentials { add_if_placeholder_enabled!("pCloud", &config.pcloud_credentials, &creds.access_token); }
                if let Some(ref creds) = config.ipfs_credentials { add_if_placeholder_enabled!("IPFS", &config.ipfs_credentials, &creds.jwt_token); }
            }

            let watch_dir_str = s.watch_dir.to_string_lossy().to_string();
            let clean_watch_dir = if let Some(stripped) = watch_dir_str.strip_prefix(r"\\?\") {
                stripped.to_string()
            } else {
                watch_dir_str
            };

            format!(
                "Status: OK\nPaused: {}\nWatch Directory: \"{}\"\nConfig File: {}\nActive Backends: {:?}\nFailed Backends: {:?}\nSyncing: {}\nWeb UI Address: {}\n",
                s.paused, clean_watch_dir, s.config_file, backend_names, failed_backends, s.syncing, s.ui_addr.as_deref().unwrap_or("-")
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
                    s.conflict_policy = config.conflict_policy.unwrap_or_default();
                    s.dry_run = config.dry_run.unwrap_or(false);
                    info!("Configuration reloaded successfully. Active backends, conflict policy, dry-run, and rate limits updated.");
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
                let max_concurrency = s.max_concurrency;
                let conflict_policy = s.conflict_policy;
                let dry_run = s.dry_run;
                let state_clone = state.clone();
                tokio::spawn(async move {
                    info!("Manual sync triggered via control command. Starting bidirectional sync...");
                    for active_backend in &backends {
                        let safe_name = active_backend.backend.name().to_lowercase().replace(" ", "_");
                        let state_filename = format!(".sync_state_{}.bin", safe_name);
                        let state_file_path = watch_dir.join(state_filename);
                        if let Err(e) = crate::sync_engine::sync_bidirectional(
                            &watch_dir,
                            active_backend.backend.clone(),
                            active_backend.policy,
                            &state_file_path,
                            &gitignore,
                            max_concurrency,
                            conflict_policy,
                            dry_run,
                            active_backend.selective_sync.clone(),
                        ).await {
                            error!("Bidirectional sync failed for backend '{}': {}", active_backend.backend.name(), e);
                        }
                    }
                    info!("Manual sync completed!");
                    state_clone.lock().await.syncing = false;
                });
                "Status: Sync triggered in background\n".to_string()
            }
        }
        "clear" => {
            if parts.len() < 2 {
                return "Error: clear command requires a provider name argument (e.g. 'clear MEGA')\n".to_string();
            }
            let target_provider = parts[1..].join(" ");
            let s = state.lock().await;
            let matching_backend = s.backends.iter().find(|ab| ab.backend.name().eq_ignore_ascii_case(&target_provider));
            match matching_backend {
                Some(active_backend) => {
                    let backend = active_backend.backend.clone();
                    info!("Clearing all files on remote provider: {}", backend.name());
                    match backend.list("").await {
                        Ok(items) => {
                            for item in items {
                                let path_str = item.path.to_string_lossy();
                                info!("Deleting remote item: {}", path_str);
                                if let Err(e) = backend.delete(&path_str).await {
                                    error!("Failed to delete remote item '{}': {}", path_str, e);
                                }
                            }
                            info!("Successfully cleared remote provider: {}", backend.name());
                            format!("Status: Successfully cleared remote provider: {}\n", backend.name())
                        }
                        Err(e) => {
                            error!("Failed to list files on provider '{}': {}", backend.name(), e);
                            format!("Error: Failed to list files on provider: {}\n", e)
                        }
                    }
                }
                None => {
                    error!("No enabled provider found matching name: '{}'", target_provider);
                    format!("Error: No enabled provider found matching name: '{}'\n", target_provider)
                }
            }
        }
        "stop" => {
            let _ = shutdown_tx.send(()).await;
            "Status: Stopping daemon...\n".to_string()
        }
        _ => "Error: Unknown command. Supported: status, pause, resume, reload, sync, clear, stop\n".to_string(),
    }
}
