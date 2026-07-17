// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri_plugin_shell::ShellExt;

#[tokio::main]
async fn main() {
    // Set up auto-launch on system boot
    if let Ok(current_exe) = std::env::current_exe() {
        let auto_start = auto_launch::AutoLaunchBuilder::new()
            .set_app_name("Cloud Sync")
            .set_app_path(&current_exe.to_string_lossy())
            .build();
        if let Ok(auto_start) = auto_start {
            if let Ok(false) = auto_start.is_enabled() {
                let _ = auto_start.enable();
            }
        }
    }

    // Spawn the background Axum HTTP UI server in a separate task
    tokio::spawn(async {
        if let Err(e) = cloud_sync_ui::start_ui_server().await {
            eprintln!("Axum background server error: {}", e);
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // Check if private_config.toml exists, otherwise default to config.toml
            let config_arg = if std::path::Path::new("private_config.toml").exists() {
                "private_config.toml"
            } else {
                "config.toml"
            };

            // Spawn the cloud_sync_daemon sidecar process
            if let Ok(sidecar) = app.shell().sidecar("cloud_sync_daemon") {
                match sidecar.args([config_arg]).spawn() {
                    Ok((mut rx, _child)) => {
                        tauri::async_runtime::spawn(async move {
                            while let Some(event) = rx.recv().await {
                                if let tauri_plugin_shell::process::CommandEvent::Stdout(line) = event {
                                    println!("[Daemon] {}", String::from_utf8_lossy(&line).trim_end());
                                }
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("Failed to spawn daemon sidecar: {}", e);
                    }
                }
            } else {
                eprintln!("Failed to locate daemon sidecar binary in application package.");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
