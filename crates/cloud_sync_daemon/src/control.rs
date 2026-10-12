//! Control socket server command processing.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};

use crate::config::load_or_create_config;
use crate::DaemonState;

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
            let backend_names: Vec<String> = s
                .backends
                .iter()
                .map(|ab| ab.backend.name().to_string())
                .collect();

            let mut failed_backends: Vec<String> = s
                .connection_errors
                .keys()
                .map(|k| {
                    if k.starts_with("Encrypted(") && k.ends_with(')') {
                        k["Encrypted(".len()..k.len() - 1].to_string()
                    } else {
                        k.clone()
                    }
                })
                .collect();

            if let Ok(config) = load_or_create_config(&s.config_file).await {
                let is_placeholder = |val: &str| -> bool {
                    val.is_empty()
                        || val.contains("PLACEHOLDER")
                        || val.contains("your_")
                        || val.contains("your-")
                };

                macro_rules! add_if_placeholder_enabled {
                    ($name:expr, $creds:expr, $token:expr) => {
                        if crate::config::is_provider_enabled($creds)
                            && is_placeholder($token)
                            && !failed_backends.contains(&$name.to_string())
                        {
                            failed_backends.push($name.to_string());
                        }
                    };
                }

                if let Some(ref creds) = config.google_credentials {
                    add_if_placeholder_enabled!(
                        "Google Drive",
                        &config.google_credentials,
                        &creds.refresh_token
                    );
                }
                if let Some(ref creds) = config.dropbox_credentials {
                    add_if_placeholder_enabled!(
                        "Dropbox",
                        &config.dropbox_credentials,
                        &creds.refresh_token
                    );
                }
                if let Some(ref creds) = config.onedrive_credentials {
                    add_if_placeholder_enabled!(
                        "OneDrive",
                        &config.onedrive_credentials,
                        &creds.refresh_token
                    );
                }
                if let Some(ref creds) = config.box_credentials {
                    add_if_placeholder_enabled!(
                        "Box",
                        &config.box_credentials,
                        &creds.refresh_token
                    );
                }
                if let Some(ref creds) = config.mega_credentials {
                    add_if_placeholder_enabled!("MEGA", &config.mega_credentials, &creds.password);
                }
                if let Some(ref creds) = config.webdav_credentials {
                    add_if_placeholder_enabled!(
                        "WebDAV",
                        &config.webdav_credentials,
                        &creds.password
                    );
                }
                if let Some(ref creds) = config.s3_credentials {
                    add_if_placeholder_enabled!(
                        "S3",
                        &config.s3_credentials,
                        &creds.secret_access_key
                    );
                }
                if let Some(ref creds) = config.sftp_credentials {
                    if crate::config::is_provider_enabled(&config.sftp_credentials) {
                        let pw_placeholder =
                            creds.password.as_ref().is_some_and(|p| is_placeholder(p));
                        let key_placeholder = creds
                            .private_key_path
                            .as_ref()
                            .map_or(true, |k| is_placeholder(k));
                        if pw_placeholder
                            && key_placeholder
                            && !failed_backends.contains(&"SFTP".to_string())
                        {
                            failed_backends.push("SFTP".to_string());
                        }
                    }
                }
                if let Some(ref creds) = config.nextcloud_credentials {
                    add_if_placeholder_enabled!(
                        "Nextcloud",
                        &config.nextcloud_credentials,
                        &creds.app_password
                    );
                }
                if let Some(ref creds) = config.azure_blob_credentials {
                    if crate::config::is_provider_enabled(&config.azure_blob_credentials)
                        && (is_placeholder(&creds.account_key)
                            || creds.account_key == "devstoreaccount1")
                        && !failed_backends.contains(&"Azure Blob".to_string())
                    {
                        failed_backends.push("Azure Blob".to_string());
                    }
                }
                if let Some(ref creds) = config.gcs_credentials {
                    add_if_placeholder_enabled!(
                        "Google Cloud Storage",
                        &config.gcs_credentials,
                        &creds.service_account_key_path
                    );
                }
                if let Some(ref creds) = config.b2_credentials {
                    add_if_placeholder_enabled!(
                        "Backblaze B2",
                        &config.b2_credentials,
                        &creds.application_key
                    );
                }
                if let Some(ref creds) = config.pcloud_credentials {
                    add_if_placeholder_enabled!(
                        "pCloud",
                        &config.pcloud_credentials,
                        &creds.access_token
                    );
                }
                if let Some(ref creds) = config.ipfs_credentials {
                    add_if_placeholder_enabled!("IPFS", &config.ipfs_credentials, &creds.jwt_token);
                }
            }

            let watch_dir_str = s.watch_dir.to_string_lossy().to_string();
            let clean_watch_dir = if let Some(stripped) = watch_dir_str.strip_prefix(r"\\?\") {
                stripped.to_string()
            } else {
                watch_dir_str
            };

            let mut simulated_backends: Vec<String> = Vec::new();
            let mut backend_details: Vec<serde_json::Value> = Vec::new();

            for ab in &s.backends {
                let raw_name = ab.backend.name().to_string();
                let clean_name = if raw_name.starts_with("Encrypted(") && raw_name.ends_with(')') {
                    raw_name["Encrypted(".len()..raw_name.len() - 1].to_string()
                } else {
                    raw_name.clone()
                };

                let is_failed = failed_backends.contains(&clean_name);
                let is_sim = ab.backend.is_simulated();
                let sim_reason = ab.backend.simulation_reason();

                if is_sim && !simulated_backends.contains(&clean_name) {
                    simulated_backends.push(clean_name.clone());
                }

                let mode = if is_failed {
                    "failed"
                } else if is_sim {
                    "simulated"
                } else {
                    "cloud"
                };

                backend_details.push(serde_json::json!({
                    "name": clean_name,
                    "mode": mode,
                    "is_simulated": is_sim,
                    "simulation_reason": sim_reason,
                    "is_failed": is_failed,
                }));
            }

            let backend_details_json = serde_json::to_string(&backend_details).unwrap_or_else(|_| "[]".to_string());

            format!(
                "Status: OK\nPaused: {}\nWatch Directory: \"{}\"\nConfig File: {}\nActive Backends: {:?}\nFailed Backends: {:?}\nSimulated Backends: {:?}\nBackend Details: {}\nSyncing: {}\nWeb UI Address: {}\n",
                s.paused, clean_watch_dir, s.config_file, backend_names, failed_backends, simulated_backends, backend_details_json, s.syncing, s.ui_addr.as_deref().unwrap_or("-")
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
                    let upload_limiter = config
                        .max_upload_rate
                        .map(|rate| cloud_sync_lib::rate_limit::TokenBucket::new(rate * 1024));
                    let download_limiter = config
                        .max_download_rate
                        .map(|rate| cloud_sync_lib::rate_limit::TokenBucket::new(rate * 1024));
                    let backends = crate::build_backends(
                        &config,
                        upload_limiter.clone(),
                        download_limiter.clone(),
                    );
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
                let event_tx = s.event_tx.clone();
                let state_clone = state.clone();
                tokio::spawn(async move {
                    info!(
                        "Manual sync triggered via control command. Starting bidirectional sync..."
                    );
                    for active_backend in &backends {
                        let safe_name = active_backend
                            .backend
                            .name()
                            .to_lowercase()
                            .replace(" ", "_");
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
                            Some(event_tx.clone()),
                        )
                        .await
                        {
                            error!(
                                "Bidirectional sync failed for backend '{}': {}",
                                active_backend.backend.name(),
                                e
                            );
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
            let matching_backend = s
                .backends
                .iter()
                .find(|ab| ab.backend.name().eq_ignore_ascii_case(&target_provider));
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
                            format!(
                                "Status: Successfully cleared remote provider: {}\n",
                                backend.name()
                            )
                        }
                        Err(e) => {
                            error!(
                                "Failed to list files on provider '{}': {}",
                                backend.name(),
                                e
                            );
                            format!("Error: Failed to list files on provider: {}\n", e)
                        }
                    }
                }
                None => {
                    error!(
                        "No enabled provider found matching name: '{}'",
                        target_provider
                    );
                    format!(
                        "Error: No enabled provider found matching name: '{}'\n",
                        target_provider
                    )
                }
            }
        }
        "conflicts" => {
            let s = state.lock().await;
            let list = s.conflict_manager.list_conflicts().await;
            match serde_json::to_string(&list) {
                Ok(json) => format!("{}\n", json),
                Err(e) => format!("Error: Failed to serialize conflicts: {}\n", e),
            }
        }
        "resolve_conflict" => {
            if parts.len() < 3 {
                return "Error: resolve_conflict requires <path> and <resolution> (use_local, use_remote, keep_both, skip)\n".to_string();
            }
            let rel_path = parts[1];
            let resolution = parts[2].to_lowercase();

            let s = state.lock().await;
            let watch_dir = s.watch_dir.clone();
            let backends = s.backends.clone();
            let conflict_manager = s.conflict_manager.clone();
            let event_tx = s.event_tx.clone();
            drop(s);

            let local_path = watch_dir.join(rel_path);

            match resolution.as_str() {
                "use_local" => {
                    info!("Resolving conflict on '{}' with 'use_local'", rel_path);
                    for active in &backends {
                        if let Err(e) = active.backend.upload(&local_path, rel_path).await {
                            error!("Failed to upload local file during conflict resolution: {}", e);
                            return format!("Error: Failed to upload file: {}\n", e);
                        }
                    }
                    conflict_manager.remove_conflict(rel_path).await;
                    let _ = event_tx.send(
                        serde_json::json!({
                            "type": "conflict_resolved",
                            "rel_path": rel_path,
                            "resolution": "use_local"
                        })
                        .to_string(),
                    );
                    "Status: Conflict resolved (use_local)\n".to_string()
                }
                "use_remote" => {
                    info!("Resolving conflict on '{}' with 'use_remote'", rel_path);
                    if let Some(active) = backends.first() {
                        if let Err(e) = active.backend.download(rel_path, &local_path).await {
                            error!("Failed to download remote file during conflict resolution: {}", e);
                            return format!("Error: Failed to download file: {}\n", e);
                        }
                    }
                    conflict_manager.remove_conflict(rel_path).await;
                    let _ = event_tx.send(
                        serde_json::json!({
                            "type": "conflict_resolved",
                            "rel_path": rel_path,
                            "resolution": "use_remote"
                        })
                        .to_string(),
                    );
                    "Status: Conflict resolved (use_remote)\n".to_string()
                }
                "keep_both" => {
                    info!("Resolving conflict on '{}' with 'keep_both'", rel_path);
                    let conflict_rel = format!("{}.local-conflict", rel_path);
                    let conflict_local = watch_dir.join(&conflict_rel);

                    if let Err(e) = tokio::fs::rename(&local_path, &conflict_local).await {
                        error!("Failed to rename local file during conflict resolution: {}", e);
                        return format!("Error: Failed to rename local file: {}\n", e);
                    }

                    if let Some(active) = backends.first() {
                        if let Err(e) = active.backend.download(rel_path, &local_path).await {
                            error!("Failed to download remote file during keep_both: {}", e);
                        }
                        if let Err(e) = active.backend.upload(&conflict_local, &conflict_rel).await {
                            error!("Failed to upload .local-conflict to remote: {}", e);
                        }
                    }

                    conflict_manager.remove_conflict(rel_path).await;
                    let _ = event_tx.send(
                        serde_json::json!({
                            "type": "conflict_resolved",
                            "rel_path": rel_path,
                            "resolution": "keep_both"
                        })
                        .to_string(),
                    );
                    "Status: Conflict resolved (keep_both)\n".to_string()
                }
                "skip" => {
                    info!("Dismissing conflict on '{}'", rel_path);
                    conflict_manager.remove_conflict(rel_path).await;
                    let _ = event_tx.send(
                        serde_json::json!({
                            "type": "conflict_resolved",
                            "rel_path": rel_path,
                            "resolution": "skip"
                        })
                        .to_string(),
                    );
                    "Status: Conflict dismissed\n".to_string()
                }
                _ => {
                    format!("Error: Unknown resolution '{}'. Supported: use_local, use_remote, keep_both, skip\n", resolution)
                }
            }
        }
        "stop" => {
            let _ = shutdown_tx.send(()).await;
            "Status: Stopping daemon...\n".to_string()
        }
        _ => {
            "Error: Unknown command. Supported: status, pause, resume, reload, sync, clear, conflicts, resolve_conflict, stop\n"
                .to_string()
        }
    }
}

/// Encapsulates the TCP server socket loop for processing control protocol messages (SRP).
pub struct DaemonControlServer {
    bind_addr: String,
}

impl DaemonControlServer {
    pub fn new(bind_addr: impl Into<String>) -> Self {
        Self {
            bind_addr: bind_addr.into(),
        }
    }

    pub fn bind_addr(&self) -> &str {
        &self.bind_addr
    }

    pub async fn run(
        &self,
        state: Arc<Mutex<DaemonState>>,
        shutdown_tx: mpsc::Sender<()>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind(&self.bind_addr).await?;
        info!("Daemon TCP control server listening on {}", self.bind_addr);

        loop {
            tokio::select! {
                res = listener.accept() => {
                    match res {
                        Ok((socket, _addr)) => {
                            let state_clone = state.clone();
                            let shutdown_tx_clone = shutdown_tx.clone();
                            tokio::spawn(async move {
                                let (reader, mut writer) = socket.into_split();
                                let mut buf_reader = BufReader::new(reader);
                                let mut line = String::new();
                                if buf_reader.read_line(&mut line).await.is_ok() {
                                    let response = handle_control_command(line.trim(), &state_clone, &shutdown_tx_clone).await;
                                    let _ = writer.write_all(response.as_bytes()).await;
                                }
                            });
                        }
                        Err(e) => {
                            error!("Error accepting TCP control connection: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("TCP control server shutting down.");
                    break;
                }
            }
        }
        Ok(())
    }
}
