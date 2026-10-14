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

/// Spawns a background task executing periodic synchronization passes at the specified interval.
pub fn spawn_periodic_sync_worker(state: SharedDaemonState, interval_secs: u64) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(interval_secs)).await;

            let should_run = {
                let mut lock = state.lock().await;
                if lock.paused || lock.syncing || lock.backends.is_empty() {
                    false
                } else {
                    lock.syncing = true;
                    true
                }
            };

            if !should_run {
                continue;
            }

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
                let lock = state.lock().await;
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

            cloud_sync_daemon::run_sync_all_backends(
                &watch_dir,
                &state_dir,
                &backends,
                &gitignore,
                cloud_sync_daemon::InProcessSyncOptions {
                    max_concurrency,
                    conflict_policy,
                    dry_run,
                },
                Some(event_tx),
            )
            .await;

            let mut lock = state.lock().await;
            lock.syncing = false;
        }
    });
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
            app.manage(in_process_state.clone());

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            let has_sidecar = {
                use tauri_plugin_shell::ShellExt;
                let mut spawned = false;
                if let Ok(sidecar) = app.shell().sidecar("cloud_sync_daemon") {
                    if let Ok((mut rx, _child)) = sidecar.args([&config_file]).spawn() {
                        spawned = true;
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
                spawned
            };

            #[cfg(any(target_os = "android", target_os = "ios"))]
            let has_sidecar = false;

            if !has_sidecar {
                // On mobile or standalone in-process runs, start periodic background synchronization
                spawn_periodic_sync_worker(in_process_state, 30);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::sync,
            commands::pause,
            commands::resume,
            commands::conflicts,
            commands::resolve_conflict,
            commands::get_config,
            commands::save_config,
            commands::list_directory,
            commands::create_directory
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
