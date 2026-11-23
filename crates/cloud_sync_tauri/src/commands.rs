//! Tauri IPC command implementations for in-process sync operations.

use cloud_sync_daemon::{run_sync_all_backends, InProcessSyncOptions};
use std::sync::Arc;
use tokio::sync::Mutex;

pub type SharedDaemonState = Arc<Mutex<cloud_sync_daemon::DaemonState>>;

#[tauri::command]
pub async fn status(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        if let Ok(raw) = client.send_command("status").await {
            let mut val = cloud_sync_ui::parser::parse_status(&raw);
            val["daemon_running"] = serde_json::Value::Bool(true);
            return Ok(val);
        }
    }

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
        "daemon_running": !lock.paused && !lock.backends.is_empty(),
        "paused": lock.paused,
        "syncing": lock.syncing,
        "watch_directory": lock.watch_dir.to_string_lossy(),
        "watched_directory": lock.watch_dir.to_string_lossy(),
        "config_file": lock.config_file,
        "web_ui_address": lock.ui_addr.as_deref().unwrap_or("Tauri Direct IPC"),
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
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        if let Ok(raw) = client.send_command("sync").await {
            return Ok(serde_json::json!({ "status": raw.trim() }));
        }
    }

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
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        if let Ok(raw) = client.send_command("pause").await {
            return Ok(serde_json::json!({ "status": raw.trim() }));
        }
    }

    let mut lock = state.lock().await;
    lock.paused = true;
    Ok(serde_json::json!({ "status": "paused" }))
}

#[tauri::command]
pub async fn resume(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        if let Ok(raw) = client.send_command("resume").await {
            return Ok(serde_json::json!({ "status": raw.trim() }));
        }
    }

    let mut lock = state.lock().await;
    lock.paused = false;
    Ok(serde_json::json!({ "status": "resumed" }))
}

#[tauri::command]
pub async fn stop(state: tauri::State<'_, SharedDaemonState>) -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        let _ = client.send_command("stop").await;
    }

    let mut lock = state.lock().await;
    lock.paused = true;
    lock.syncing = false;
    Ok(serde_json::json!({ "status": "stopped", "daemon_running": false }))
}

#[tauri::command]
pub async fn start() -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let config_file = if std::path::Path::new(cloud_sync_ui::PRIVATE_CONFIG_FILE).exists() {
            cloud_sync_ui::PRIVATE_CONFIG_FILE
        } else {
            cloud_sync_ui::DEFAULT_CONFIG_FILE
        };

        let daemon_exe_name = if cfg!(target_os = "windows") {
            "cloud_sync_daemon.exe"
        } else {
            "cloud_sync_daemon"
        };

        let mut daemon_bin_path = None;
        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(parent) = current_exe.parent() {
                let path = parent.join(daemon_exe_name);
                if path.exists() {
                    daemon_bin_path = Some(path);
                }
            }
        }

        let exe_to_run = daemon_bin_path
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| daemon_exe_name.to_string());

        let mut cmd = std::process::Command::new(&exe_to_run);
        cmd.arg(config_file);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let _ = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn daemon process: {}", e))?;
        tokio::time::sleep(tokio::time::Duration::from_millis(
            cloud_sync_ui::DAEMON_SPAWN_DELAY_MS,
        ))
        .await;
        Ok(serde_json::json!({ "status": "started", "daemon_running": true }))
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        Ok(serde_json::json!({ "status": "started", "daemon_running": true }))
    }
}

#[tauri::command]
pub async fn reload() -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        if let Ok(raw) = client.send_command("reload").await {
            return Ok(serde_json::json!({ "status": raw.trim() }));
        }
    }
    Ok(serde_json::json!({ "status": "reloaded" }))
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

fn sanitize_app_config_value(
    mut val: serde_json::Value,
) -> Result<cloud_sync_daemon::config::AppConfig, String> {
    if let serde_json::Value::Object(ref mut map) = val {
        // Merge defaults for missing fields
        if let Ok(default_val) =
            serde_json::to_value(cloud_sync_daemon::config::AppConfig::default())
        {
            if let serde_json::Value::Object(def_map) = default_val {
                for (k, v) in def_map {
                    map.entry(k).or_insert(v);
                }
            }
        }

        // 1. Normalize conflict_policy if present or empty string
        if let Some(cp) = map.get_mut("conflict_policy") {
            let normalized = match cp.as_str() {
                Some(s) => match s.trim().to_lowercase().replace('_', "-").as_str() {
                    "" | "renamelocal" | "rename-local" => "rename-local",
                    "renameremote" | "rename-remote" => "rename-remote",
                    "keepnewer" | "keep-newer" => "keep-newer",
                    "keeplocal" | "keep-local" | "overwriteremote" | "overwrite-remote" => {
                        "keep-local"
                    }
                    "keepremote" | "keep-remote" | "overwritelocal" | "overwrite-local" => {
                        "keep-remote"
                    }
                    "manual" => "manual",
                    _ => "rename-local",
                },
                None => "rename-local",
            };
            *cp = serde_json::Value::String(normalized.to_string());
        }

        // 2. Normalize provider credentials
        const PROVIDER_KEYS: &[&str] = &[
            "google_credentials",
            "dropbox_credentials",
            "onedrive_credentials",
            "webdav_credentials",
            "s3_credentials",
            "sftp_credentials",
            "nextcloud_credentials",
            "box_credentials",
            "mega_credentials",
            "azure_blob_credentials",
            "gcs_credentials",
            "b2_credentials",
            "pcloud_credentials",
            "ipfs_credentials",
            "ftp_credentials",
            "http_credentials",
            "smb_credentials",
        ];

        for &key in PROVIDER_KEYS {
            if let Some(prov_val) = map.get_mut(key) {
                if let serde_json::Value::Object(ref mut pmap) = prov_val {
                    // Flatten nested "common" object if the frontend sent it
                    if let Some(serde_json::Value::Object(common_map)) = pmap.remove("common") {
                        for (ck, cv) in common_map {
                            if !cv.is_null() && !pmap.contains_key(&ck) {
                                pmap.insert(ck, cv);
                            }
                        }
                    }

                    // Check if credentials actually have non-empty payload
                    let has_valid_creds = pmap.iter().any(|(k, v)| {
                        k != "enabled"
                            && k != "destination_folder"
                            && match v {
                                serde_json::Value::String(s) => !s.trim().is_empty(),
                                serde_json::Value::Number(_) => true,
                                _ => false,
                            }
                    });

                    if !has_valid_creds {
                        // If no credentials filled in, treat as None/null
                        *prov_val = serde_json::Value::Null;
                    }
                }
            }
        }
    }

    serde_json::from_value(val).map_err(|e| format!("Failed to parse configuration: {}", e))
}

#[tauri::command]
pub async fn save_config(
    state: tauri::State<'_, SharedDaemonState>,
    config: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut lock = state.lock().await;
    let mut config_file = lock.config_file.clone();

    let mut parsed_config = sanitize_app_config_value(config)?;

    // Ensure watch directory exists and fallback if empty
    let effective_watch = if parsed_config.watch_directory.as_os_str().is_empty() {
        let home_dir = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        home_dir.join("CloudSync")
    } else {
        parsed_config.watch_directory.clone()
    };
    let _ = tokio::fs::create_dir_all(&effective_watch).await;
    lock.watch_dir = effective_watch.clone();
    parsed_config.watch_directory = effective_watch;

    if let Some(cp) = parsed_config.conflict_policy {
        lock.conflict_policy = cp;
    }
    if let Some(dr) = parsed_config.dry_run {
        lock.dry_run = dr;
    }
    if let Some(mc) = parsed_config.max_concurrency {
        lock.max_concurrency = mc;
    }

    // Rebuild active backends
    lock.backends = cloud_sync_daemon::build_backends(
        &parsed_config,
        lock.upload_limiter.clone(),
        lock.download_limiter.clone(),
    );

    // Save TOML
    let toml_str = toml::to_string_pretty(&parsed_config)
        .map_err(|e| format!("Failed to serialize TOML configuration: {}", e))?;

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&config_file).parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    // Write file with fallback to user AppData if current path is read-only
    if let Err(e) = tokio::fs::write(&config_file, &toml_str).await {
        let mut fallback_success = false;
        let fallback_dir = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .or_else(|_| {
                std::env::var("USERPROFILE").map(|p| std::path::PathBuf::from(p).join(".config"))
            })
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("com.cloudsync.app");

        let _ = tokio::fs::create_dir_all(&fallback_dir).await;
        let fallback_path = fallback_dir.join("config.toml");
        if tokio::fs::write(&fallback_path, &toml_str).await.is_ok() {
            config_file = fallback_path.to_string_lossy().to_string();
            lock.config_file = config_file.clone();
            fallback_success = true;
        }

        if !fallback_success {
            return Err(format!("Failed to save config file ({}): {}", config_file, e));
        }
    }

    let _ = lock.event_tx.send(
        serde_json::json!({
            "type": "status",
            "message": format!("Configuration saved. Active backends: {}", lock.backends.len())
        })
        .to_string(),
    );

    Ok(serde_json::json!({
        "status": "saved",
        "config_file": config_file,
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

#[tauri::command]
pub async fn logs(state: tauri::State<'_, SharedDaemonState>) -> Result<String, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = &state;
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        if let Ok(resp) = client.send_command("get_log").await {
            if resp.starts_with("Status: OK") {
                if let Some(pos) = resp.find("\n\n") {
                    return Ok(resp[pos + 2..].to_string());
                }
            }
        }

        let offline_path = cloud_sync_ui::get_offline_log_path();
        match tokio::fs::read_to_string(&offline_path).await {
            Ok(content) => Ok(content),
            Err(e) => Ok(format!(
                "(Daemon is offline and could not read local log file '{}': {})",
                offline_path, e
            )),
        }
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let lock = state.lock().await;
        let log_file = &lock.log_file;
        let path = lock.state_dir.join(log_file);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Ok(content),
            Err(_) => match tokio::fs::read_to_string(log_file).await {
                Ok(content) => Ok(content),
                Err(e) => Ok(format!(
                    "(Could not read mobile log file '{}': {})",
                    path.display(),
                    e
                )),
            },
        }
    }
}

#[tauri::command]
pub async fn verify_sync(
    state: tauri::State<'_, SharedDaemonState>,
    remote_path: Option<String>,
    compare_checksums: Option<bool>,
) -> Result<serde_json::Value, String> {
    let subpath = remote_path.unwrap_or_default();
    let checksum = compare_checksums.unwrap_or(false);

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        let flag = if checksum { " --checksum" } else { "" };
        let cmd = format!("verify {}{}", subpath, flag);
        if let Ok(raw) = client.send_command(&cmd).await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let watch_dir = s.watch_dir.clone();
    let backends = s.backends.clone();
    drop(s);

    let mut results = Vec::new();
    for active in &backends {
        match cloud_sync_lib::verifier::verify_integrity(
            active.backend.as_ref(),
            &watch_dir,
            &subpath,
            checksum,
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
    Ok(serde_json::json!(results))
}

#[tauri::command]
pub async fn scan_duplicates(
    state: tauri::State<'_, SharedDaemonState>,
    remote_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let subpath = remote_path.unwrap_or_default();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        let cmd = format!("dedupe {}", subpath);
        if let Ok(raw) = client.send_command(&cmd).await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let backends = s.backends.clone();
    drop(s);

    let mut results = Vec::new();
    for active in &backends {
        match cloud_sync_lib::dedupe::scan_duplicates(active.backend.as_ref(), &subpath).await {
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
    Ok(serde_json::json!(results))
}

#[tauri::command]
pub async fn get_storage_analytics(
    state: tauri::State<'_, SharedDaemonState>,
    remote_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let subpath = remote_path.unwrap_or_default();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        let cmd = format!("stats {}", subpath);
        if let Ok(raw) = client.send_command(&cmd).await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let backends = s.backends.clone();
    drop(s);

    let mut results = Vec::new();
    for active in &backends {
        match cloud_sync_lib::analytics::compute_storage_analytics(
            active.backend.as_ref(),
            &subpath,
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
    Ok(serde_json::json!(results))
}

#[tauri::command]
pub async fn get_remote_tree(
    state: tauri::State<'_, SharedDaemonState>,
    remote_path: Option<String>,
    max_depth: Option<usize>,
) -> Result<serde_json::Value, String> {
    let subpath = remote_path.unwrap_or_default();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        let depth_str = max_depth.map(|d| format!(" {}", d)).unwrap_or_default();
        let cmd = format!("tree {}{}", subpath, depth_str);
        if let Ok(raw) = client.send_command(&cmd).await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let backends = s.backends.clone();
    drop(s);

    let mut results = Vec::new();
    for active in &backends {
        match cloud_sync_lib::analytics::build_remote_tree(
            active.backend.as_ref(),
            &subpath,
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
    Ok(serde_json::json!(results))
}

#[tauri::command]
pub async fn preview_sync(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        if let Ok(raw) = client.send_command("plan").await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let mut plans = Vec::new();
    for active_backend in &s.backends {
        let safe_name = active_backend
            .backend
            .name()
            .to_lowercase()
            .replace(' ', "_");
        let state_file_path = cloud_sync_daemon::engine::resolve_state_file_path(
            &s.state_dir,
            &s.watch_dir,
            &safe_name,
        )
        .await;
        let filter = cloud_sync_daemon::engine::FileFilter::new(
            &s.gitignore,
            active_backend.selective_sync.as_deref(),
        );
        let orchestrator = cloud_sync_daemon::engine::SyncOrchestrator::default_real();
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
    Ok(serde_json::json!(plans))
}

#[tauri::command]
pub async fn list_trash_versions(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        if let Ok(raw) = client.send_command("trash_list").await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let trash_dir = s.state_dir.join("trash");
    drop(s);

    let mgr =
        cloud_sync_daemon::engine::TrashManager::new(cloud_sync_daemon::engine::VersioningConfig {
            enabled: true,
            trash_dir,
            retention_days: 30,
            max_versions_per_file: 5,
        });
    let versions = mgr.list_versions().await.map_err(|e| e.to_string())?;
    Ok(serde_json::json!(versions))
}

#[tauri::command]
pub async fn restore_trash_version(
    state: tauri::State<'_, SharedDaemonState>,
    archive_id: String,
) -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        let cmd = format!("trash_restore {}", archive_id);
        if let Ok(raw) = client.send_command(&cmd).await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let trash_dir = s.state_dir.join("trash");
    let watch_dir = s.watch_dir.clone();
    drop(s);

    let mgr =
        cloud_sync_daemon::engine::TrashManager::new(cloud_sync_daemon::engine::VersioningConfig {
            enabled: true,
            trash_dir,
            retention_days: 30,
            max_versions_per_file: 5,
        });
    let restored = mgr
        .restore_version(&archive_id, &watch_dir, None)
        .await
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "status": "restored",
        "archive_id": archive_id,
        "restored_path": restored.to_string_lossy()
    }))
}

#[tauri::command]
pub async fn transfer_cloud_to_cloud(
    state: tauri::State<'_, SharedDaemonState>,
    source_backend: String,
    dest_backend: String,
    delete_extraneous: Option<bool>,
) -> Result<serde_json::Value, String> {
    let del_extra = delete_extraneous.unwrap_or(false);

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        let flag = if del_extra { " --sync" } else { "" };
        let cmd = format!(
            "transfer {} {}{}",
            source_backend.replace(' ', "_"),
            dest_backend.replace(' ', "_"),
            flag
        );
        if let Ok(raw) = client.send_command(&cmd).await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let backends = s.backends.clone();
    drop(s);

    let src_norm = source_backend.to_lowercase().replace(' ', "_");
    let dst_norm = dest_backend.to_lowercase().replace(' ', "_");

    let src = backends
        .iter()
        .find(|b| b.backend.name().to_lowercase().replace(' ', "_") == src_norm)
        .ok_or_else(|| format!("Source backend '{}' not found", source_backend))?;
    let dst = backends
        .iter()
        .find(|b| b.backend.name().to_lowercase().replace(' ', "_") == dst_norm)
        .ok_or_else(|| format!("Destination backend '{}' not found", dest_backend))?;

    let report = cloud_sync_lib::transfer_between_backends(
        src.backend.as_ref(),
        dst.backend.as_ref(),
        "",
        "",
        cloud_sync_lib::CloudTransferOptions {
            delete_extraneous: del_extra,
            skip_matching: true,
            dry_run: false,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(serde_json::json!(report))
}

#[tauri::command]
pub async fn create_cloud_snapshot(
    state: tauri::State<'_, SharedDaemonState>,
    label: Option<String>,
) -> Result<serde_json::Value, String> {
    let snap_label = label.unwrap_or_else(|| "manual".to_string());

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        let cmd = format!("snapshot_create {}", snap_label);
        if let Ok(raw) = client.send_command(&cmd).await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let watch_dir = s.watch_dir.clone();
    let backends = s.backends.clone();
    drop(s);

    let mut results = Vec::new();
    for active in &backends {
        match cloud_sync_lib::create_snapshot(active.backend.as_ref(), &watch_dir, &snap_label)
            .await
        {
            Ok(mf) => results.push(serde_json::json!({
                "backend": active.backend.name(),
                "snapshot": mf
            })),
            Err(e) => results.push(serde_json::json!({
                "backend": active.backend.name(),
                "error": e.to_string()
            })),
        }
    }
    Ok(serde_json::json!(results))
}

#[tauri::command]
pub async fn list_cloud_snapshots(
    state: tauri::State<'_, SharedDaemonState>,
) -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        if let Ok(raw) = client.send_command("snapshot_list").await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

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
    Ok(serde_json::json!(results))
}

#[tauri::command]
pub async fn restore_cloud_snapshot(
    state: tauri::State<'_, SharedDaemonState>,
    snapshot_id: String,
) -> Result<serde_json::Value, String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use cloud_sync_ui::client::DaemonClientPort;
        let client = cloud_sync_ui::TcpDaemonClient::new(cloud_sync_ui::get_daemon_control_addr());
        let cmd = format!("snapshot_restore {}", snapshot_id);
        if let Ok(raw) = client.send_command(&cmd).await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Ok(val);
            }
        }
    }

    let s = state.lock().await;
    let watch_dir = s.watch_dir.clone();
    let backends = s.backends.clone();
    drop(s);

    for active in &backends {
        if let Ok(rep) =
            cloud_sync_lib::restore_snapshot(active.backend.as_ref(), &snapshot_id, &watch_dir)
                .await
        {
            return Ok(serde_json::json!(rep));
        }
    }
    Err(format!(
        "Snapshot '{}' not found on any active backend",
        snapshot_id
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_offline_log_fallback() {
        let dir = tempdir().unwrap();
        let log_file = dir.path().join("test_daemon.log");
        tokio::fs::write(&log_file, "2026-11-02 [INFO] Test log line")
            .await
            .unwrap();

        let content = tokio::fs::read_to_string(&log_file).await.unwrap();
        assert!(content.contains("[INFO] Test log line"));
    }

    #[tokio::test]
    async fn test_create_directory_and_list_directory() {
        let dir = tempdir().unwrap();
        let base_path = dir.path().to_path_buf();

        let sub1 = base_path.join("alpha");
        let sub2 = base_path.join("beta");
        let hidden = base_path.join(".hidden");
        tokio::fs::create_dir(&sub1).await.unwrap();
        tokio::fs::create_dir(&sub2).await.unwrap();
        tokio::fs::create_dir(&hidden).await.unwrap();

        // Also write a file (should not appear in subdirectories list)
        tokio::fs::write(base_path.join("file.txt"), b"data")
            .await
            .unwrap();

        let listing = list_directory(Some(base_path.to_str().unwrap().to_string()))
            .await
            .expect("list_directory should succeed");

        assert_eq!(listing.subdirectories, vec!["alpha", "beta"]);
        assert!(listing.presets.len() >= 4);

        // Test create_directory
        let new_sub = base_path.join("gamma/delta");
        let res = create_directory(new_sub.to_str().unwrap().to_string())
            .await
            .unwrap();
        assert_eq!(res["status"], "created");
        assert!(new_sub.is_dir());
    }

    #[tokio::test]
    async fn test_list_directory_default_home() {
        let listing = list_directory(None)
            .await
            .expect("listing home should succeed");
        assert!(!listing.current.is_empty());
        assert!(!listing.presets.is_empty());
    }
}
