//! # Cloud Sync Tauri Mobile & Desktop Application
//!
//! Provides the cross-platform Tauri 2.0 shell, supporting both desktop execution
//! (sidecar process + Axum UI proxy) and native mobile execution on Android/iOS
//! (in-process `cloud_sync_daemon` synchronization engine with direct Tauri IPC).

pub mod commands;

use cloud_sync_daemon::{build_backends, config, engine::conflict::ConflictManager, DaemonState};
pub use commands::SharedDaemonState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

/// Creates an in-process daemon state for mobile / standalone execution.
pub async fn create_in_process_state(
    watch_dir: PathBuf,
    state_dir: PathBuf,
    config_file: String,
) -> SharedDaemonState {
    let (event_tx, _) = tokio::sync::broadcast::channel::<String>(100);

    // Ensure watch_dir and state_dir exist
    let _ = tokio::fs::create_dir_all(&watch_dir).await;
    let _ = tokio::fs::create_dir_all(&state_dir).await;

    let app_config = config::load_or_create_config(&config_file)
        .await
        .unwrap_or_else(|_| config::AppConfig {
            watch_directory: watch_dir.clone(),
            ..Default::default()
        });

    let backends = build_backends(&app_config, None, None);

    Arc::new(Mutex::new(DaemonState {
        paused: false,
        backends,
        watch_dir,
        config_file,
        syncing: false,
        ui_addr: None,
        gitignore: cloud_sync_lib::SyncIgnore::empty(),
        exclude: None,
        upload_limiter: None,
        download_limiter: None,
        max_concurrency: app_config.max_concurrency.unwrap_or(4),
        connection_errors: HashMap::new(),
        conflict_policy: cloud_sync_lib::ConflictPolicy::RenameLocal,
        dry_run: false,
        event_tx,
        conflict_manager: ConflictManager::new(),
        log_file: "mobile_sync.log".to_string(),
        state_dir,
    }))
}

// ---------------------------------------------------------------------------
// Application Entrypoint
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        // Desktop auto-launch initialization
        if let Ok(current_exe) = std::env::current_exe() {
            if let Ok(auto_start) = auto_launch::AutoLaunchBuilder::new()
                .set_app_name("Cloud Sync")
                .set_app_path(&current_exe.to_string_lossy())
                .build()
            {
                if let Ok(false) = auto_start.is_enabled() {
                    let _ = auto_start.enable();
                }
            }
        }

        // Spawn desktop background Axum HTTP UI server
        tauri::async_runtime::spawn(async {
            if let Err(e) = cloud_sync_ui::start_ui_server().await {
                eprintln!("Axum background server error: {}", e);
            }
        });
    }

    let mut builder = tauri::Builder::default();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder.plugin(tauri_plugin_shell::init());
    }

    builder
        .setup(|app| {
            // Resolve directories for in-process sync engine
            let app_data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let watch_dir = app_data_dir.join("watched_folder");
            let state_dir = app_data_dir.join("sync_state");
            let config_file = if std::path::Path::new("private_config.toml").exists() {
                "private_config.toml".to_string()
            } else if std::path::Path::new("config.toml").exists() {
                "config.toml".to_string()
            } else {
                app_data_dir
                    .join("config.toml")
                    .to_string_lossy()
                    .to_string()
            };

            let in_process_state = tauri::async_runtime::block_on(create_in_process_state(
                watch_dir,
                state_dir,
                config_file.clone(),
            ));
            app.manage(in_process_state);

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                use tauri_plugin_shell::ShellExt;
                // On desktop, spawn sidecar process if present
                if let Ok(sidecar) = app.shell().sidecar("cloud_sync_daemon") {
                    if let Ok((mut rx, _child)) = sidecar.args([&config_file]).spawn() {
                        tauri::async_runtime::spawn(async move {
                            while let Some(event) = rx.recv().await {
                                if let tauri_plugin_shell::process::CommandEvent::Stdout(line) =
                                    event
                                {
                                    println!(
                                        "[Daemon] {}",
                                        String::from_utf8_lossy(&line).trim_end()
                                    );
                                }
                            }
                        });
                    }
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::sync,
            commands::pause,
            commands::resume,
            commands::conflicts,
            commands::resolve_conflict
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
