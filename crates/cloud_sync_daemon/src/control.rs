//! Control socket server command processing.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};

use crate::config::load_or_create_config;
use crate::DaemonState;

/// Handles a string command sent from the daemon controller via TCP,
/// delegating to single-responsibility command handlers (SRP).
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
        "status" => handle_status_command(state).await,
        "get_log" | "log" | "pause" | "resume" | "reload" | "sync" | "clear" | "stop" => {
            handle_lifecycle_command(&parts, state, shutdown_tx).await
        }
        "conflicts" | "resolve_conflict" | "get_plan" | "plan" | "preview" => {
            handle_conflict_and_plan_command(&parts, state).await
        }
        "verify" | "check" | "dedupe" | "stats" | "size" | "tree" => {
            handle_diagnostics_command(&parts, state).await
        }
        "trash" | "trash_list" | "trash_restore" | "transfer" | "snapshot_create" | "snapshot"
        | "snapshot_list" | "snapshots" | "snapshot_restore" => {
            handle_storage_ops_command(&parts, state).await
        }
        _ => {
            "Error: Unknown command. Supported: status, pause, resume, reload, sync, clear, conflicts, resolve_conflict, verify, dedupe, size, tree, preview, trash, trash_restore, transfer, snapshot_create, snapshot_list, snapshot_restore, stop\n"
                .to_string()
        }
    }
}

/// Handles the `status` command, reporting active, simulated, and placeholder-configured providers (SRP & OCP).
async fn handle_status_command(state: &Arc<Mutex<DaemonState>>) -> String {
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
        for (creds, _) in config.enabled_credentials() {
            let display_name = creds.display_name().to_string();
            if creds.has_placeholder_secret() && !failed_backends.contains(&display_name) {
                failed_backends.push(display_name);
            }
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

    let backend_details_json =
        serde_json::to_string(&backend_details).unwrap_or_else(|_| "[]".to_string());

    format!(
        "Status: OK\nPaused: {}\nWatch Directory: \"{}\"\nConfig File: {}\nActive Backends: {:?}\nFailed Backends: {:?}\nSimulated Backends: {:?}\nBackend Details: {}\nSyncing: {}\nWeb UI Address: {}\nLog File: \"{}\"\nState Directory: \"{}\"\n",
        s.paused,
        clean_watch_dir,
        s.config_file,
        backend_names,
        failed_backends,
        simulated_backends,
        backend_details_json,
        s.syncing,
        s.ui_addr.as_deref().unwrap_or("-"),
        s.log_file,
        s.state_dir.display()
    )
}

/// Handles daemon lifecycle and runtime control commands (`pause`, `resume`, `reload`, `sync`, `clear`, `get_log`, `stop`) (SRP).
async fn handle_lifecycle_command(
    parts: &[&str],
    state: &Arc<Mutex<DaemonState>>,
    shutdown_tx: &mpsc::Sender<()>,
) -> String {
    match parts[0] {
        "get_log" | "log" => {
            let log_file = {
                let s = state.lock().await;
                s.log_file.clone()
            };
            match tokio::fs::read_to_string(&log_file).await {
                Ok(content) => format!("Status: OK\nLog File: \"{}\"\n\n{}", log_file, content),
                Err(e) => format!(
                    "Status: Error\nCould not read log file '{}': {}\n",
                    log_file, e
                ),
            }
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
                let state_dir = s.state_dir.clone();
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
                        let state_file_path = crate::engine::resolve_state_file_path(
                            &state_dir, &watch_dir, &safe_name,
                        )
                        .await;
                        if let Err(e) = crate::engine::sync_bidirectional(
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
                return "Error: clear command requires a provider name argument (e.g. 'clear MEGA')\n"
                    .to_string();
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
        "stop" => {
            let _ = shutdown_tx.send(()).await;
            "Status: Stopping daemon...\n".to_string()
        }
        _ => unreachable!(),
    }
}

/// Handles conflict inspection, resolution, and dry-run plan preview commands (SRP).
async fn handle_conflict_and_plan_command(
    parts: &[&str],
    state: &Arc<Mutex<DaemonState>>,
) -> String {
    match parts[0] {
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
                            error!(
                                "Failed to upload local file during conflict resolution: {}",
                                e
                            );
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
                            error!(
                                "Failed to download remote file during conflict resolution: {}",
                                e
                            );
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
                        error!(
                            "Failed to rename local file during conflict resolution: {}",
                            e
                        );
                        return format!("Error: Failed to rename local file: {}\n", e);
                    }

                    if let Some(active) = backends.first() {
                        if let Err(e) = active.backend.download(rel_path, &local_path).await {
                            error!("Failed to download remote file during keep_both: {}", e);
                        }
                        if let Err(e) = active.backend.upload(&conflict_local, &conflict_rel).await
                        {
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
        "get_plan" | "plan" | "preview" => {
            let s = state.lock().await;
            let mut plans = Vec::new();
            for active_backend in &s.backends {
                let safe_name = active_backend
                    .backend
                    .name()
                    .to_lowercase()
                    .replace(' ', "_");
                let state_file_path =
                    crate::engine::resolve_state_file_path(&s.state_dir, &s.watch_dir, &safe_name)
                        .await;
                let filter = crate::engine::FileFilter::new(
                    &s.gitignore,
                    active_backend.selective_sync.as_deref(),
                );
                let orchestrator = crate::engine::SyncOrchestrator::default_real();
                if let Ok(plan) = orchestrator
                    .plan_sync(
                        active_backend.backend.as_ref(),
                        &s.watch_dir,
                        &state_file_path,
                        &filter,
                        active_backend.policy,
                        s.conflict_policy,
                    )
                    .await
                {
                    plans.push(plan);
                }
            }
            serde_json::to_string(&plans).unwrap_or_else(|_| "[]".to_string()) + "\n"
        }
        _ => unreachable!(),
    }
}

/// Handles storage diagnostic commands (`verify`, `dedupe`, `stats`, `tree`) (SRP).
async fn handle_diagnostics_command(parts: &[&str], state: &Arc<Mutex<DaemonState>>) -> String {
    match parts[0] {
        "verify" | "check" => {
            let s = state.lock().await;
            let watch_dir = s.watch_dir.clone();
            let backends = s.backends.clone();
            drop(s);

            let remote_subpath = parts.get(1).copied().unwrap_or("");
            let compare_checksums = parts.iter().any(|&p| p == "--checksum" || p == "-c");

            let mut results = Vec::new();
            for active in &backends {
                match cloud_sync_lib::verifier::verify_integrity(
                    active.backend.as_ref(),
                    &watch_dir,
                    remote_subpath,
                    compare_checksums,
                )
                .await
                {
                    Ok(report) => {
                        results.push(serde_json::json!({
                            "backend": active.backend.name(),
                            "report": report
                        }));
                    }
                    Err(e) => {
                        results.push(serde_json::json!({
                            "backend": active.backend.name(),
                            "error": e.to_string()
                        }));
                    }
                }
            }
            serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()) + "\n"
        }
        "dedupe" => {
            let s = state.lock().await;
            let backends = s.backends.clone();
            drop(s);

            let remote_subpath = parts.get(1).copied().unwrap_or("");
            let mut results = Vec::new();

            for active in &backends {
                match cloud_sync_lib::dedupe::scan_duplicates(
                    active.backend.as_ref(),
                    remote_subpath,
                )
                .await
                {
                    Ok(groups) => {
                        results.push(serde_json::json!({
                            "backend": active.backend.name(),
                            "duplicate_groups": groups
                        }));
                    }
                    Err(e) => {
                        results.push(serde_json::json!({
                            "backend": active.backend.name(),
                            "error": e.to_string()
                        }));
                    }
                }
            }
            serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()) + "\n"
        }
        "stats" | "size" => {
            let s = state.lock().await;
            let backends = s.backends.clone();
            drop(s);

            let remote_subpath = parts.get(1).copied().unwrap_or("");
            let mut results = Vec::new();

            for active in &backends {
                match cloud_sync_lib::analytics::compute_storage_analytics(
                    active.backend.as_ref(),
                    remote_subpath,
                )
                .await
                {
                    Ok(analytics) => {
                        results.push(serde_json::json!({
                            "backend": active.backend.name(),
                            "analytics": analytics
                        }));
                    }
                    Err(e) => {
                        results.push(serde_json::json!({
                            "backend": active.backend.name(),
                            "error": e.to_string()
                        }));
                    }
                }
            }
            serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()) + "\n"
        }
        "tree" => {
            let s = state.lock().await;
            let backends = s.backends.clone();
            drop(s);

            let remote_subpath = parts.get(1).copied().unwrap_or("");
            let max_depth: Option<usize> = parts.get(2).and_then(|d| d.parse().ok());
            let mut results = Vec::new();

            for active in &backends {
                match cloud_sync_lib::analytics::build_remote_tree(
                    active.backend.as_ref(),
                    remote_subpath,
                    max_depth,
                )
                .await
                {
                    Ok(tree) => {
                        let ascii = cloud_sync_lib::analytics::format_ascii_tree(&tree);
                        results.push(serde_json::json!({
                            "backend": active.backend.name(),
                            "ascii": ascii,
                            "tree": tree
                        }));
                    }
                    Err(e) => {
                        results.push(serde_json::json!({
                            "backend": active.backend.name(),
                            "error": e.to_string()
                        }));
                    }
                }
            }
            serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()) + "\n"
        }
        _ => unreachable!(),
    }
}

/// Handles versioning, cloud-to-cloud transfer, and snapshot commands (SRP).
async fn handle_storage_ops_command(parts: &[&str], state: &Arc<Mutex<DaemonState>>) -> String {
    match parts[0] {
        "trash" | "trash_list" => {
            let s = state.lock().await;
            let trash_dir = s.state_dir.join("trash");
            drop(s);

            let mgr = crate::engine::TrashManager::new(crate::engine::VersioningConfig {
                enabled: true,
                trash_dir,
                retention_days: 30,
                max_versions_per_file: 5,
            });
            match mgr.list_versions().await {
                Ok(list) => {
                    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string()) + "\n"
                }
                Err(e) => format!("Error: Failed to list trash versions: {}\n", e),
            }
        }
        "trash_restore" => {
            if parts.len() < 2 {
                return "Error: trash_restore requires <archive_id>\n".to_string();
            }
            let archive_id = parts[1];
            let custom_rel = parts.get(2).copied();

            let s = state.lock().await;
            let trash_dir = s.state_dir.join("trash");
            let watch_dir = s.watch_dir.clone();
            drop(s);

            let mgr = crate::engine::TrashManager::new(crate::engine::VersioningConfig {
                enabled: true,
                trash_dir,
                retention_days: 30,
                max_versions_per_file: 5,
            });
            match mgr
                .restore_version(archive_id, &watch_dir, custom_rel)
                .await
            {
                Ok(restored) => {
                    serde_json::json!({
                        "status": "restored",
                        "archive_id": archive_id,
                        "restored_path": restored.to_string_lossy()
                    })
                    .to_string()
                        + "\n"
                }
                Err(e) => format!("Error: Failed to restore version '{}': {}\n", archive_id, e),
            }
        }
        "transfer" => {
            if parts.len() < 3 {
                return "Error: transfer requires <source_backend> <dest_backend> [--sync]\n"
                    .to_string();
            }
            let src_name = parts[1].to_lowercase();
            let dst_name = parts[2].to_lowercase();
            let delete_extra = parts.contains(&"--sync");

            let s = state.lock().await;
            let backends = s.backends.clone();
            drop(s);

            let src_opt = backends.iter().find(|b| {
                b.backend.name().to_lowercase().replace(' ', "_") == src_name
                    || b.backend.name().to_lowercase() == src_name
            });
            let dst_opt = backends.iter().find(|b| {
                b.backend.name().to_lowercase().replace(' ', "_") == dst_name
                    || b.backend.name().to_lowercase() == dst_name
            });

            match (src_opt, dst_opt) {
                (Some(src), Some(dst)) => {
                    match cloud_sync_lib::transfer_between_backends(
                        src.backend.as_ref(),
                        dst.backend.as_ref(),
                        "",
                        "",
                        cloud_sync_lib::CloudTransferOptions {
                            delete_extraneous: delete_extra,
                            skip_matching: true,
                            dry_run: false,
                        },
                    )
                    .await
                    {
                        Ok(report) => {
                            serde_json::to_string(&report).unwrap_or_else(|_| "{}".to_string())
                                + "\n"
                        }
                        Err(e) => format!("Error: Cloud transfer failed: {}\n", e),
                    }
                }
                _ => format!(
                    "Error: Could not find active source '{}' and/or destination '{}' backend\n",
                    src_name, dst_name
                ),
            }
        }
        "snapshot_create" | "snapshot" => {
            let label = parts.get(1).copied().unwrap_or("manual");
            let s = state.lock().await;
            let watch_dir = s.watch_dir.clone();
            let backends = s.backends.clone();
            drop(s);

            let mut results = Vec::new();
            for active in &backends {
                match cloud_sync_lib::create_snapshot(active.backend.as_ref(), &watch_dir, label)
                    .await
                {
                    Ok(manifest) => results.push(serde_json::json!({
                        "backend": active.backend.name(),
                        "snapshot": manifest
                    })),
                    Err(e) => results.push(serde_json::json!({
                        "backend": active.backend.name(),
                        "error": e.to_string()
                    })),
                }
            }
            serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()) + "\n"
        }
        "snapshot_list" | "snapshots" => {
            let s = state.lock().await;
            let backends = s.backends.clone();
            drop(s);

            let mut results = Vec::new();
            for active in &backends {
                match cloud_sync_lib::list_snapshots(active.backend.as_ref()).await {
                    Ok(list) => results.push(serde_json::json!({
                        "backend": active.backend.name(),
                        "snapshots": list
                    })),
                    Err(e) => results.push(serde_json::json!({
                        "backend": active.backend.name(),
                        "error": e.to_string()
                    })),
                }
            }
            serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()) + "\n"
        }
        "snapshot_restore" => {
            if parts.len() < 2 {
                return "Error: snapshot_restore requires <snapshot_id>\n".to_string();
            }
            let snap_id = parts[1];
            let s = state.lock().await;
            let watch_dir = s.watch_dir.clone();
            let backends = s.backends.clone();
            drop(s);

            for active in &backends {
                if let Ok(rep) =
                    cloud_sync_lib::restore_snapshot(active.backend.as_ref(), snap_id, &watch_dir)
                        .await
                {
                    return serde_json::to_string(&rep).unwrap_or_else(|_| "{}".to_string()) + "\n";
                }
            }
            format!(
                "Error: Snapshot '{}' not found on any active backend\n",
                snap_id
            )
        }
        _ => unreachable!(),
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
                        Ok((socket, addr)) => {
                            if !addr.ip().is_loopback() {
                                warn!("Rejected non-loopback TCP control connection from {}", addr);
                                continue;
                            }
                            let state_clone = state.clone();
                            let shutdown_tx_clone = shutdown_tx.clone();
                            tokio::spawn(async move {
                                use tokio::io::AsyncReadExt;
                                let (reader, mut writer) = socket.into_split();
                                // Bounded line reader: at most 16 KiB per command to prevent memory exhaustion DOS
                                let mut limited_reader = BufReader::new(reader.take(16 * 1024));
                                let mut line = String::new();
                                if limited_reader.read_line(&mut line).await.is_ok() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ActiveBackend;
    use cloud_sync_lib::{
        ConflictPolicy, StorageBackend, StorageCapabilities, StorageError, StorageItem, SyncIgnore,
        SyncPolicy,
    };
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::tempdir;

    struct MockControlBackend {
        name: String,
        simulated: bool,
    }

    #[async_trait::async_trait]
    impl StorageBackend for MockControlBackend {
        fn name(&self) -> &str {
            &self.name
        }
        fn capabilities(&self) -> StorageCapabilities {
            StorageCapabilities::NONE
        }
        fn is_simulated(&self) -> bool {
            self.simulated
        }
        fn simulation_reason(&self) -> Option<String> {
            if self.simulated {
                Some("Test simulation".to_string())
            } else {
                None
            }
        }
        async fn upload(&self, _local: &Path, _remote: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn download(&self, _remote: &str, local: &Path) -> Result<(), StorageError> {
            if let Some(p) = local.parent() {
                let _ = tokio::fs::create_dir_all(p).await;
            }
            tokio::fs::write(local, b"mock content")
                .await
                .map_err(StorageError::Io)?;
            Ok(())
        }
        async fn delete(&self, _remote: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list(&self, _remote: &str) -> Result<Vec<StorageItem>, StorageError> {
            Ok(vec![])
        }
    }

    fn create_test_state(
        temp_path: &Path,
    ) -> (
        Arc<Mutex<DaemonState>>,
        mpsc::Sender<()>,
        mpsc::Receiver<()>,
    ) {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(2);

        let watch_dir = temp_path.join("watch");
        let state_dir = temp_path.join("state");
        let log_file = temp_path.join("daemon.log");
        let config_file = temp_path.join("config.toml");

        let backend1 = Arc::new(MockControlBackend {
            name: "MockCloud".to_string(),
            simulated: false,
        });
        let backend2 = Arc::new(MockControlBackend {
            name: "MockSim".to_string(),
            simulated: true,
        });

        let mut connection_errors = HashMap::new();
        connection_errors.insert("FailedCloud".to_string(), "Auth failed".to_string());

        let state = Arc::new(Mutex::new(DaemonState {
            paused: false,
            backends: vec![
                ActiveBackend {
                    backend: backend1,
                    policy: SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
                    selective_sync: None,
                },
                ActiveBackend {
                    backend: backend2,
                    policy: SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
                    selective_sync: None,
                },
            ],
            watch_dir: watch_dir.clone(),
            config_file: config_file.to_string_lossy().to_string(),
            syncing: false,
            ui_addr: Some("127.0.0.1:8080".to_string()),
            gitignore: SyncIgnore::new(&watch_dir, &[]),
            exclude: None,
            upload_limiter: None,
            download_limiter: None,
            max_concurrency: 4,
            connection_errors,
            conflict_policy: ConflictPolicy::RenameLocal,
            dry_run: false,
            event_tx,
            conflict_manager: crate::engine::conflict::ConflictManager::new(),
            log_file: log_file.to_string_lossy().to_string(),
            state_dir,
        }));

        (state, shutdown_tx, shutdown_rx)
    }

    #[tokio::test]
    async fn test_empty_and_unknown_commands() {
        let temp = tempdir().unwrap();
        let (state, shutdown_tx, _) = create_test_state(temp.path());

        let resp_empty = handle_control_command("", &state, &shutdown_tx).await;
        assert!(resp_empty.contains("Error: Empty command"));

        let resp_unknown =
            handle_control_command("invalid_command_xyz", &state, &shutdown_tx).await;
        assert!(resp_unknown.contains("Error: Unknown command"));
    }

    #[tokio::test]
    async fn test_lifecycle_commands() {
        let temp = tempdir().unwrap();
        let (state, shutdown_tx, mut shutdown_rx) = create_test_state(temp.path());

        // 1. Status command
        let resp_status = handle_control_command("status", &state, &shutdown_tx).await;
        assert!(resp_status.contains("Status: OK"));
        assert!(resp_status.contains("Active Backends: [\"MockCloud\", \"MockSim\"]"));
        assert!(resp_status.contains("Simulated Backends: [\"MockSim\"]"));
        assert!(resp_status.contains("Failed Backends: [\"FailedCloud\"]"));

        // 2. Pause and resume
        let resp_pause = handle_control_command("pause", &state, &shutdown_tx).await;
        assert!(resp_pause.contains("Status: Paused"));
        assert!(state.lock().await.paused);

        let resp_resume = handle_control_command("resume", &state, &shutdown_tx).await;
        assert!(resp_resume.contains("Status: Resumed"));
        assert!(!state.lock().await.paused);

        // 3. Log commands
        let resp_log_missing = handle_control_command("get_log", &state, &shutdown_tx).await;
        assert!(resp_log_missing.contains("Status: Error"));

        let log_file_path = state.lock().await.log_file.clone();
        tokio::fs::write(&log_file_path, "Log line 1\nLog line 2")
            .await
            .unwrap();
        let resp_log = handle_control_command("log", &state, &shutdown_tx).await;
        assert!(resp_log.contains("Status: OK"));
        assert!(resp_log.contains("Log line 1"));

        // 4. Sync command
        let resp_sync = handle_control_command("sync", &state, &shutdown_tx).await;
        assert!(resp_sync.contains("Status: Sync triggered"));

        // 5. Clear command
        let resp_clear_missing = handle_control_command("clear", &state, &shutdown_tx).await;
        assert!(resp_clear_missing.contains("Error: clear command requires a provider name"));

        let resp_clear_unknown =
            handle_control_command("clear UnknownProvider", &state, &shutdown_tx).await;
        assert!(resp_clear_unknown.contains("Error: No enabled provider found"));

        let resp_clear_ok = handle_control_command("clear MockCloud", &state, &shutdown_tx).await;
        assert!(resp_clear_ok.contains("Status: Successfully cleared remote provider"));

        // 6. Stop command
        let resp_stop = handle_control_command("stop", &state, &shutdown_tx).await;
        assert!(resp_stop.contains("Status: Stopping daemon"));
        assert!(shutdown_rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn test_conflict_and_plan_commands() {
        let temp = tempdir().unwrap();
        let (state, shutdown_tx, _) = create_test_state(temp.path());

        // 1. Conflicts empty
        let resp_conflicts = handle_control_command("conflicts", &state, &shutdown_tx).await;
        assert_eq!(resp_conflicts.trim(), "[]");

        // 2. Resolve conflict with missing args
        let resp_res_missing =
            handle_control_command("resolve_conflict", &state, &shutdown_tx).await;
        assert!(
            resp_res_missing.contains("Error: resolve_conflict requires <path> and <resolution>")
        );

        let resp_res_bad_res =
            handle_control_command("resolve_conflict test.txt bad_mode", &state, &shutdown_tx)
                .await;
        assert!(resp_res_bad_res.contains("Error: Unknown resolution"));

        // 3. Resolutions: skip, use_local, use_remote, keep_both
        let resp_skip =
            handle_control_command("resolve_conflict file.txt skip", &state, &shutdown_tx).await;
        assert!(resp_skip.contains("Status: Conflict dismissed"));

        let watch_dir = state.lock().await.watch_dir.clone();
        tokio::fs::create_dir_all(&watch_dir).await.unwrap();
        let local_file = watch_dir.join("doc.txt");
        tokio::fs::write(&local_file, b"local data").await.unwrap();

        let resp_use_local =
            handle_control_command("resolve_conflict doc.txt use_local", &state, &shutdown_tx)
                .await;
        assert!(resp_use_local.contains("Status: Conflict resolved (use_local)"));

        let resp_use_remote =
            handle_control_command("resolve_conflict doc.txt use_remote", &state, &shutdown_tx)
                .await;
        assert!(resp_use_remote.contains("Status: Conflict resolved (use_remote)"));

        let resp_keep_both =
            handle_control_command("resolve_conflict doc.txt keep_both", &state, &shutdown_tx)
                .await;
        assert!(resp_keep_both.contains("Status: Conflict resolved (keep_both)"));

        // 4. Plan commands
        let resp_plan = handle_control_command("plan", &state, &shutdown_tx).await;
        assert!(resp_plan.starts_with('['));
    }

    #[tokio::test]
    async fn test_diagnostics_and_storage_ops() {
        let temp = tempdir().unwrap();
        let (state, shutdown_tx, _) = create_test_state(temp.path());

        // 1. Verify / check
        let resp_verify = handle_control_command("verify --checksum", &state, &shutdown_tx).await;
        assert!(resp_verify.starts_with('['));

        // 2. Dedupe
        let resp_dedupe = handle_control_command("dedupe", &state, &shutdown_tx).await;
        assert!(resp_dedupe.starts_with('['));

        // 3. Stats / size
        let resp_stats = handle_control_command("stats", &state, &shutdown_tx).await;
        assert!(resp_stats.starts_with('['));

        // 4. Tree
        let resp_tree = handle_control_command("tree 2", &state, &shutdown_tx).await;
        assert!(resp_tree.starts_with('['));

        // 5. Trash commands
        let resp_trash = handle_control_command("trash", &state, &shutdown_tx).await;
        assert!(resp_trash.starts_with('['));

        let resp_trash_restore_missing =
            handle_control_command("trash_restore", &state, &shutdown_tx).await;
        assert!(resp_trash_restore_missing.contains("Error: trash_restore requires <archive_id>"));

        let resp_trash_restore_fail =
            handle_control_command("trash_restore bogus_id", &state, &shutdown_tx).await;
        assert!(resp_trash_restore_fail.contains("Error: Failed to restore version"));

        // 6. Transfer command
        let resp_transfer_missing = handle_control_command("transfer", &state, &shutdown_tx).await;
        assert!(resp_transfer_missing
            .contains("Error: transfer requires <source_backend> <dest_backend>"));

        let resp_transfer_missing_backend =
            handle_control_command("transfer non_existent1 non_existent2", &state, &shutdown_tx)
                .await;
        assert!(resp_transfer_missing_backend.contains("Error: Could not find active source"));

        let resp_transfer_ok =
            handle_control_command("transfer mockcloud mocksim", &state, &shutdown_tx).await;
        assert!(resp_transfer_ok.contains('{'));

        // 7. Snapshots
        let resp_snap = handle_control_command("snapshot manual_test", &state, &shutdown_tx).await;
        assert!(resp_snap.starts_with('['));

        let resp_snap_list = handle_control_command("snapshots", &state, &shutdown_tx).await;
        assert!(resp_snap_list.starts_with('['));

        let resp_snap_restore_missing =
            handle_control_command("snapshot_restore", &state, &shutdown_tx).await;
        assert!(
            resp_snap_restore_missing.contains("Error: snapshot_restore requires <snapshot_id>")
        );

        let resp_snap_restore_not_found =
            handle_control_command("snapshot_restore non_existent_snap", &state, &shutdown_tx)
                .await;
        assert!(
            resp_snap_restore_not_found.contains("Error: Snapshot 'non_existent_snap' not found")
        );

        // 8. Server constructor
        let server = DaemonControlServer::new("127.0.0.1:9099");
        assert_eq!(server.bind_addr(), "127.0.0.1:9099");
    }
}
