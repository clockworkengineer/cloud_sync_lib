//! Tauri IPC command implementations for in-process sync operations.

use cloud_sync_daemon::{run_sync_all_backends, InProcessSyncOptions};
use std::sync::Arc;
use tokio::sync::Mutex;

pub type SharedDaemonState = Arc<Mutex<cloud_sync_daemon::DaemonState>>;

#[tauri::command]
pub async fn status(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    let lock = state.lock().await;

    let active_backends: Vec<String> = lock
        .backends
        .iter()
        .map(|b| b.backend.name().to_string())
        .collect();

    let mut simulated_backends = Vec::new();
    let mut failed_backends = Vec::new();

    for b in &lock.backends {
        let name = b.backend.name().to_string();
        if let Some(err) = lock.connection_errors.get(&name) {
            if err.contains("Simulated") || err.contains("Fallback") {
                simulated_backends.push(name);
            } else {
                failed_backends.push(name);
            }
        }
    }

    Ok(serde_json::json!({
        "daemon_running": true,
        "paused": lock.paused,
        "syncing": lock.syncing,
        "watched_directory": lock.watch_dir.to_string_lossy(),
        "config_file": lock.config_file,
        "active_backends": active_backends,
        "simulated_backends": simulated_backends,
        "failed_backends": failed_backends,
        "log_file": lock.log_file,
        "dry_run": lock.dry_run,
        "conflict_policy": format!("{:?}", lock.conflict_policy),
    }))
}

#[tauri::command]
pub async fn sync(state: tauri::State<'_, SharedDaemonState>) -> Result<serde_json::Value, String> {
    let state_clone = state.inner().clone();

    // Mark syncing
    {
        let mut lock = state_clone.lock().await;
        if lock.syncing {
            return Ok(serde_json::json!({ "status": "sync_already_in_progress" }));
        }
        lock.syncing = true;
    }

    // Spawn synchronization task in the background
    tokio::spawn(async move {
        let (
            watch_dir,
            state_dir,
            backends,
            gitignore,
            max_concurrency,
            conflict_policy,
            dry_run,
            event_tx,
        ) = {
            let lock = state_clone.lock().await;
            (
                lock.watch_dir.clone(),
                lock.state_dir.clone(),
                lock.backends.clone(),
                lock.gitignore.clone(),
                lock.max_concurrency,
                lock.conflict_policy,
                lock.dry_run,
                lock.event_tx.clone(),
            )
        };

        run_sync_all_backends(
            &watch_dir,
            &state_dir,
            &backends,
            &gitignore,
            InProcessSyncOptions {
                max_concurrency,
                conflict_policy,
                dry_run,
            },
            Some(event_tx),
        )
        .await;

        let mut lock = state_clone.lock().await;
        lock.syncing = false;
    });

    Ok(serde_json::json!({ "status": "sync_started" }))
}

#[tauri::command]
pub async fn pause(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    let mut lock = state.lock().await;
    lock.paused = true;
    Ok(serde_json::json!({ "status": "paused" }))
}

#[tauri::command]
pub async fn resume(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    let mut lock = state.lock().await;
    lock.paused = false;
    Ok(serde_json::json!({ "status": "resumed" }))
}

#[tauri::command]
pub async fn conflicts(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    let lock = state.lock().await;
    let list = lock.conflict_manager.list_conflicts().await;
    Ok(serde_json::to_value(list).unwrap_or(serde_json::json!([])))
}

#[tauri::command]
pub async fn resolve_conflict(
    state: tauri::State<'_, SharedDaemonState>,
    path: String,
    resolution: String,
) -> Result<serde_json::Value, String> {
    let s = state.lock().await;
    let watch_dir = s.watch_dir.clone();
    let backends = s.backends.clone();
    let conflict_manager = s.conflict_manager.clone();
    let event_tx = s.event_tx.clone();
    drop(s);

    let local_path = watch_dir.join(&path);

    match resolution.to_lowercase().as_str() {
        "use_local" => {
            for active in &backends {
                if let Err(e) = active.backend.upload(&local_path, &path).await {
                    return Err(format!("Failed to upload file: {}", e));
                }
            }
            conflict_manager.remove_conflict(&path).await;
            let _ = event_tx.send(
                serde_json::json!({
                    "type": "conflict_resolved",
                    "rel_path": path,
                    "resolution": "use_local"
                })
                .to_string(),
            );
            Ok(serde_json::json!({ "status": "resolved", "choice": "use_local" }))
        }
        "use_remote" => {
            if let Some(active) = backends.first() {
                if let Err(e) = active.backend.download(&path, &local_path).await {
                    return Err(format!("Failed to download file: {}", e));
                }
            }
            conflict_manager.remove_conflict(&path).await;
            let _ = event_tx.send(
                serde_json::json!({
                    "type": "conflict_resolved",
                    "rel_path": path,
                    "resolution": "use_remote"
                })
                .to_string(),
            );
            Ok(serde_json::json!({ "status": "resolved", "choice": "use_remote" }))
        }
        "keep_both" => {
            let conflict_rel = format!("{}.local-conflict", path);
            let conflict_local = watch_dir.join(&conflict_rel);
            let _ = tokio::fs::rename(&local_path, &conflict_local).await;
            if let Some(active) = backends.first() {
                let _ = active.backend.download(&path, &local_path).await;
            }
            conflict_manager.remove_conflict(&path).await;
            let _ = event_tx.send(
                serde_json::json!({
                    "type": "conflict_resolved",
                    "rel_path": path,
                    "resolution": "keep_both"
                })
                .to_string(),
            );
            Ok(serde_json::json!({ "status": "resolved", "choice": "keep_both" }))
        }
        "skip" => {
            conflict_manager.remove_conflict(&path).await;
            Ok(serde_json::json!({ "status": "skipped" }))
        }
        _ => Err(format!("Unknown resolution: {}", resolution)),
    }
}
