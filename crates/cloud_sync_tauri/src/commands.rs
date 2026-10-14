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

#[tauri::command]
pub async fn get_config(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    let config_file = {
        let lock = state.lock().await;
        lock.config_file.clone()
    };

    let config = cloud_sync_daemon::config::load_or_create_config(&config_file)
        .await
        .map_err(|e| format!("Failed to read configuration: {}", e))?;

    serde_json::to_value(&config).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(
    state: tauri::State<'_, SharedDaemonState>,
    config: cloud_sync_daemon::config::AppConfig,
) -> Result<serde_json::Value, String> {
    let mut lock = state.lock().await;
    let config_file = lock.config_file.clone();

    // Ensure watch directory exists
    let _ = tokio::fs::create_dir_all(&config.watch_directory).await;
    lock.watch_dir = config.watch_directory.clone();

    if let Some(cp) = config.conflict_policy {
        lock.conflict_policy = cp;
    }
    if let Some(dr) = config.dry_run {
        lock.dry_run = dr;
    }
    if let Some(mc) = config.max_concurrency {
        lock.max_concurrency = mc;
    }

    // Rebuild active backends
    lock.backends = cloud_sync_daemon::build_backends(
        &config,
        lock.upload_limiter.clone(),
        lock.download_limiter.clone(),
    );

    // Save TOML
    let toml_str = toml::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize TOML configuration: {}", e))?;
    tokio::fs::write(&config_file, toml_str)
        .await
        .map_err(|e| format!("Failed to save config file: {}", e))?;

    let _ = lock.event_tx.send(
        serde_json::json!({
            "type": "status",
            "message": format!("Configuration saved. Active backends: {}", lock.backends.len())
        })
        .to_string(),
    );

    Ok(serde_json::json!({
        "status": "saved",
        "watch_directory": lock.watch_dir.to_string_lossy(),
        "active_backends_count": lock.backends.len()
    }))
}

#[derive(serde::Serialize)]
pub struct DirectoryPreset {
    pub name: String,
    pub path: String,
}

#[derive(serde::Serialize)]
pub struct DirectoryListing {
    pub current: String,
    pub parent: Option<String>,
    pub subdirectories: Vec<String>,
    pub presets: Vec<DirectoryPreset>,
}

#[tauri::command]
pub async fn list_directory(path: Option<String>) -> Result<DirectoryListing, String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());

    let target_path = match path {
        Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => std::path::PathBuf::from(&home),
    };

    let canonical = target_path.canonicalize().unwrap_or(target_path.clone());
    let mut subdirectories = Vec::new();

    if let Ok(mut entries) = tokio::fs::read_dir(&canonical).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(file_type) = entry.file_type().await {
                if file_type.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with('.') && !name.starts_with('$') {
                        subdirectories.push(name);
                    }
                }
            }
        }
    }
    subdirectories.sort_by_key(|a| a.to_lowercase());

    let parent = canonical.parent().map(|p| p.to_string_lossy().to_string());

    let mut presets = Vec::new();
    let home_path = std::path::Path::new(&home);
    presets.push(DirectoryPreset {
        name: "Home".into(),
        path: home.clone(),
    });
    for (name, sub) in &[
        ("Documents", "Documents"),
        ("Downloads", "Downloads"),
        ("Pictures", "Pictures"),
        ("CloudSync", "CloudSync"),
    ] {
        let p = home_path.join(sub);
        presets.push(DirectoryPreset {
            name: (*name).into(),
            path: p.to_string_lossy().to_string(),
        });
    }

    Ok(DirectoryListing {
        current: canonical.to_string_lossy().to_string(),
        parent,
        subdirectories,
        presets,
    })
}

#[tauri::command]
pub async fn create_directory(path: String) -> Result<serde_json::Value, String> {
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| format!("Failed to create directory '{}': {}", path, e))?;
    Ok(serde_json::json!({ "status": "created", "path": path }))
}
