#![windows_subsystem = "windows"]

use eframe::egui;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct DaemonClient {
    status: serde_json::Value,
    last_update: Option<Instant>,
    error_message: Option<String>,
    cached_config_file: Option<String>,
}

impl DaemonClient {
    fn new() -> Self {
        Self {
            status: serde_json::Value::Null,
            last_update: None,
            error_message: None,
            cached_config_file: None,
        }
    }
}

struct CloudSyncApp {
    client: Arc<Mutex<DaemonClient>>,
    runtime: tokio::runtime::Runtime,
    show_log_modal: bool,
    log_content: Arc<Mutex<Option<String>>>,
    is_loading_log: Arc<Mutex<bool>>,
}

impl CloudSyncApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize style to feel premium and sleek
        let mut visuals = egui::Visuals::dark();
        visuals.window_rounding = 8.0.into();
        visuals.widgets.hovered.rounding = 4.0.into();
        visuals.widgets.active.rounding = 4.0.into();
        visuals.widgets.inactive.rounding = 4.0.into();
        cc.egui_ctx.set_visuals(visuals);

        let client = Arc::new(Mutex::new(DaemonClient::new()));
        let client_clone = client.clone();
        let ctx = cc.egui_ctx.clone();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        // Spawn a background task to poll daemon status every 1 second
        runtime.spawn(async move {
            loop {
                let status_res = Self::query_daemon_status().await;
                {
                    let mut lock = client_clone.lock().unwrap();
                    match status_res {
                        Ok(val) => {
                            if let Some(cfg) = val["config_file"].as_str() {
                                lock.cached_config_file = Some(cfg.to_string());
                            }
                            lock.status = val;
                            lock.last_update = Some(Instant::now());
                            lock.error_message = None;
                        }
                        Err(e) => {
                            lock.status = serde_json::Value::Null;
                            lock.error_message = Some(e);
                        }
                    }
                }
                ctx.request_repaint();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        Self {
            client,
            runtime,
            show_log_modal: false,
            log_content: Arc::new(Mutex::new(None)),
            is_loading_log: Arc::new(Mutex::new(false)),
        }
    }

    async fn send_daemon_command(cmd: &str) -> Result<String, String> {
        let addr =
            std::env::var("CLOUDSYNC_DAEMON_ADDR").unwrap_or_else(|_| "127.0.0.1:8081".to_string());
        let mut stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| format!("Connection error on {}: {}", addr, e))?;
        stream
            .write_all(format!("{}\n", cmd).as_bytes())
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        stream
            .shutdown()
            .await
            .map_err(|e| format!("Shutdown error: {}", e))?;

        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("Read error: {}", e))?;
        Ok(String::from_utf8_lossy(&buf).to_string())
    }

    async fn query_daemon_status() -> Result<serde_json::Value, String> {
        let raw = Self::send_daemon_command("status").await?;
        Ok(cloud_sync_ui::parser::parse_status(&raw))
    }

    fn run_command_sync(&self, cmd: &'static str) {
        let client_clone = self.client.clone();
        self.runtime.spawn(async move {
            if let Err(e) = Self::send_daemon_command(cmd).await {
                let mut lock = client_clone.lock().unwrap();
                lock.error_message = Some(e);
            }
        });
    }

    fn start_daemon_process(&self, config_file: &str) {
        let config_file = config_file.to_string();
        self.runtime.spawn(async move {
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
            if daemon_bin_path.is_none() {
                let path = std::path::Path::new(daemon_exe_name);
                if path.exists() {
                    daemon_bin_path = Some(path.to_path_buf());
                }
            }

            if daemon_bin_path.is_none() {
                let candidate = std::path::Path::new("target")
                    .join("debug")
                    .join(daemon_exe_name);
                if candidate.exists() {
                    daemon_bin_path = Some(candidate);
                }
            }
            if daemon_bin_path.is_none() {
                let candidate = std::path::Path::new("target")
                    .join("release")
                    .join(daemon_exe_name);
                if candidate.exists() {
                    daemon_bin_path = Some(candidate);
                }
            }

            let mut cmd = if let Some(bin_path) = daemon_bin_path {
                let mut c = std::process::Command::new(bin_path);
                c.arg(&config_file);
                c
            } else {
                let mut c = std::process::Command::new("cargo");
                c.args(["run", "--bin", "cloud_sync_daemon", "--", &config_file]);
                c
            };

            if let Ok(control_addr) = std::env::var("CLOUDSYNC_DAEMON_ADDR") {
                cmd.args(["--control-addr", &control_addr]);
            }
            if let Ok(ui_addr) = std::env::var("CLOUDSYNC_UI_ADDR") {
                cmd.args(["--ui-addr", &ui_addr]);
            }

            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }

            cmd.stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null());

            let res = cmd.spawn();

            match res {
                Ok(_) => {
                    println!(
                        "Successfully spawned daemon process with config: {}",
                        config_file
                    );
                }
                Err(e) => {
                    eprintln!("Failed to spawn daemon process: {}", e);
                }
            }
        });
    }

    fn open_file_externally(path: &str) {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            let absolute_path = std::path::Path::new(path);
            let full_path = if absolute_path.is_relative() {
                if let Ok(cwd) = std::env::current_dir() {
                    cwd.join(path)
                } else {
                    absolute_path.to_path_buf()
                }
            } else {
                absolute_path.to_path_buf()
            };

            let script = format!("Start-Process -FilePath '{}'", full_path.display());
            let res = std::process::Command::new("powershell")
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();

            if res.is_err() {
                let _ = std::process::Command::new("notepad.exe")
                    .arg(&full_path)
                    .spawn();
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("xdg-open").arg(path).spawn();
        }
    }

    fn fetch_logs(&self) {
        let log_content = self.log_content.clone();
        let is_loading = self.is_loading_log.clone();
        let client = self.client.clone();
        *is_loading.lock().unwrap() = true;

        self.runtime.spawn(async move {
            let content = match Self::send_daemon_command("get_log").await {
                Ok(resp) => {
                    if resp.starts_with("Status: OK") {
                        if let Some(pos) = resp.find("\n\n") {
                            resp[pos + 2..].to_string()
                        } else {
                            resp
                        }
                    } else {
                        resp
                    }
                }
                Err(_) => {
                    let file_path = {
                        let lock = client.lock().unwrap();
                        lock.status["log_file"]
                            .as_str()
                            .unwrap_or("daemon.log")
                            .to_string()
                    };
                    tokio::fs::read_to_string(&file_path)
                        .await
                        .unwrap_or_else(|e| format!("Could not read log file '{}': {}", file_path, e))
                }
            };

            *log_content.lock().unwrap() = Some(content);
            *is_loading.lock().unwrap() = false;
        });
    }
}

impl eframe::App for CloudSyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (status, last_update, error_message, cached_config_file) = {
            let client = self.client.lock().unwrap();
            (
                client.status.clone(),
                client.last_update,
                client.error_message.clone(),
                client.cached_config_file.clone(),
            )
        };

        // Refined dark styling adjustments
        let panel_frame = egui::Frame::central_panel(&ctx.style())
            .fill(egui::Color32::from_rgb(20, 22, 26))
            .inner_margin(egui::Margin::same(16.0));

        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                // Header Section
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("☁  Cloud Sync Dashboard")
                            .font(egui::FontId::proportional(22.0))
                            .strong()
                            .color(egui::Color32::from_rgb(240, 242, 248)),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("Real-time Multi-Cloud Synchronizer")
                            .font(egui::FontId::proportional(11.0))
                            .color(egui::Color32::from_rgb(120, 130, 150)),
                    );
                    ui.add_space(12.0);
                });

                if status.is_null() {
                    let config_to_use = if let Some(ref cached) = cached_config_file {
                        cached.clone()
                    } else if std::path::Path::new("private_config.toml").exists() {
                        "private_config.toml".to_string()
                    } else {
                        "config.toml".to_string()
                    };

                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(30.0);
                            ui.label(
                                egui::RichText::new("🛑  Daemon Offline")
                                    .font(egui::FontId::proportional(20.0))
                                    .strong()
                                    .color(egui::Color32::from_rgb(231, 76, 60)),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(
                                    "The background synchronization daemon is not running.",
                                )
                                .color(egui::Color32::from_rgb(160, 170, 190)),
                            );
                            ui.add_space(20.0);

                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(28, 30, 38))
                                .inner_margin(12.0)
                                .rounding(8.0)
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    egui::Color32::from_rgb(44, 48, 58),
                                ))
                                .show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label("Profile / Config File:");
                                            ui.monospace(&config_to_use);
                                        });
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            ui.label("Log File:");
                                            ui.monospace("daemon.log");
                                            if ui.small_button("📄 View Log").clicked() {
                                                self.show_log_modal = true;
                                                self.fetch_logs();
                                            }
                                            if ui.small_button("↗ Open Externally").clicked() {
                                                Self::open_file_externally("daemon.log");
                                            }
                                        });
                                    });
                                });

                            ui.add_space(20.0);

                            let start_btn =
                                egui::Button::new(egui::RichText::new("▶  Start Daemon").strong())
                                    .fill(egui::Color32::from_rgb(46, 204, 113))
                                    .min_size(egui::vec2(150.0, 36.0));
                            if ui.add(start_btn).clicked() {
                                self.start_daemon_process(&config_to_use);
                            }

                            ui.add_space(20.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Waiting for connection...");
                            });
                        });
                    });
                    return;
                }

                let paused = status["paused"].as_bool().unwrap_or(false);
                let syncing = status["syncing"].as_bool().unwrap_or(false);
                let watch_dir = status["watch_directory"].as_str().unwrap_or("");
                let clean_watch_dir = if let Some(stripped) = watch_dir.strip_prefix(r"\\?\") {
                    stripped
                } else {
                    watch_dir
                };
                let config_file = status["config_file"].as_str().unwrap_or("");

                // Error Banner (if any error in client)
                if let Some(ref err) = error_message {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(80, 20, 20))
                        .inner_margin(8.0)
                        .rounding(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("⚠ Error:")
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                );
                                ui.label(
                                    egui::RichText::new(err)
                                        .color(egui::Color32::from_rgb(240, 200, 200)),
                                );
                            });
                        });
                    ui.add_space(10.0);
                }

                // --- Card 1: System Info ---
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(28, 30, 38))
                    .inner_margin(12.0)
                    .rounding(8.0)
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(44, 48, 58),
                    ))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("SYSTEM STATUS")
                                    .strong()
                                    .color(egui::Color32::from_rgb(110, 120, 140)),
                            );
                            ui.add_space(6.0);

                            egui::Grid::new("status_grid")
                                .num_columns(2)
                                .spacing([24.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("Daemon Status:")
                                            .color(egui::Color32::from_rgb(170, 180, 200)),
                                    );
                                    if paused {
                                        ui.label(
                                            egui::RichText::new("⏸ Paused")
                                                .strong()
                                                .color(egui::Color32::from_rgb(230, 180, 34)),
                                        );
                                    } else if syncing {
                                        ui.label(
                                            egui::RichText::new("🔄 Syncing...")
                                                .strong()
                                                .color(egui::Color32::from_rgb(52, 152, 219)),
                                        );
                                    } else {
                                        ui.label(
                                            egui::RichText::new("🟢 Active (Idle)")
                                                .strong()
                                                .color(egui::Color32::from_rgb(46, 204, 113)),
                                        );
                                    }
                                    ui.end_row();

                                    ui.label(
                                        egui::RichText::new("Watch Folder:")
                                            .color(egui::Color32::from_rgb(170, 180, 200)),
                                    );
                                    ui.monospace(clean_watch_dir);
                                    ui.end_row();

                                    ui.label(
                                        egui::RichText::new("Config File:")
                                            .color(egui::Color32::from_rgb(170, 180, 200)),
                                    );
                                    ui.monospace(config_file);
                                    ui.end_row();

                                    let log_file = status["log_file"].as_str().unwrap_or("daemon.log");
                                    ui.label(
                                        egui::RichText::new("Log File:")
                                            .color(egui::Color32::from_rgb(170, 180, 200)),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.monospace(log_file);
                                        if ui.small_button("📄 View Log").clicked() {
                                            self.show_log_modal = true;
                                            self.fetch_logs();
                                        }
                                        if ui.small_button("↗ Open Externally").clicked() {
                                            Self::open_file_externally(log_file);
                                        }
                                    });
                                    ui.end_row();
                                });

                            if syncing {
                                ui.add_space(8.0);
                                ui.add(
                                    egui::ProgressBar::new(1.0)
                                        .animate(true)
                                        .text("Syncing file modifications to remote providers...")
                                        .fill(egui::Color32::from_rgb(52, 152, 219)),
                                );
                            }
                        });
                    });

                ui.add_space(12.0);

                // --- Card 2: Control Actions ---
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(28, 30, 38))
                    .inner_margin(12.0)
                    .rounding(8.0)
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(44, 48, 58),
                    ))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("ACTIONS")
                                    .strong()
                                    .color(egui::Color32::from_rgb(110, 120, 140)),
                            );
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;

                                if paused {
                                    let resume_btn = egui::Button::new(
                                        egui::RichText::new("▶  Resume Syncing").strong(),
                                    )
                                    .fill(egui::Color32::from_rgb(39, 174, 96))
                                    .min_size(egui::vec2(130.0, 30.0));
                                    if ui.add(resume_btn).clicked() {
                                        self.run_command_sync("resume");
                                    }
                                } else {
                                    let pause_btn = egui::Button::new(
                                        egui::RichText::new("⏸  Pause Syncing").strong(),
                                    )
                                    .fill(egui::Color32::from_rgb(230, 126, 34))
                                    .min_size(egui::vec2(130.0, 30.0));
                                    if ui.add(pause_btn).clicked() {
                                        self.run_command_sync("pause");
                                    }
                                }

                                let sync_btn =
                                    egui::Button::new(egui::RichText::new("🔄  Sync Now").strong())
                                        .fill(egui::Color32::from_rgb(41, 128, 185))
                                        .min_size(egui::vec2(110.0, 30.0));
                                if ui.add(sync_btn).clicked() {
                                    self.run_command_sync("sync");
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let stop_btn = egui::Button::new(
                                            egui::RichText::new("🛑  Stop Daemon").strong(),
                                        )
                                        .fill(egui::Color32::from_rgb(192, 57, 43))
                                        .min_size(egui::vec2(110.0, 30.0));
                                        if ui.add(stop_btn).clicked() {
                                            self.run_command_sync("stop");
                                        }
                                    },
                                );
                            });
                        });
                    });

                ui.add_space(12.0);

                // --- Card 3: Storage Providers ---
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(28, 30, 38))
                    .inner_margin(12.0)
                    .rounding(8.0)
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(44, 48, 58),
                    ))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("STORAGE PROVIDERS")
                                    .strong()
                                    .color(egui::Color32::from_rgb(110, 120, 140)),
                            );
                            ui.add_space(8.0);

                            // Separate active into live cloud vs simulated fallback
                            let active_list: Vec<String> = status["active_backends"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();

                            let failed_list: Vec<String> = status["failed_backends"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();

                            let simulated_list: Vec<String> = status["simulated_backends"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();

                            let live_cloud: Vec<String> = active_list
                                .iter()
                                .filter(|name| !failed_list.contains(name) && !simulated_list.contains(name))
                                .cloned()
                                .collect();

                            let sim_active: Vec<String> = active_list
                                .iter()
                                .filter(|name| !failed_list.contains(name) && simulated_list.contains(name))
                                .cloned()
                                .collect();

                            // 1. Live Cloud Providers Section
                            ui.label(
                                egui::RichText::new("🟢 Live Cloud (Connected)")
                                    .strong()
                                    .color(egui::Color32::from_rgb(46, 204, 113)),
                            );
                            ui.add_space(4.0);
                            if live_cloud.is_empty() {
                                ui.label(
                                    egui::RichText::new("None currently connected to live cloud")
                                        .italics()
                                        .color(egui::Color32::from_rgb(100, 110, 120)),
                                );
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    for name in &live_cloud {
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(35, 48, 40))
                                            .stroke(egui::Stroke::new(
                                                1.0_f32,
                                                egui::Color32::from_rgb(46, 160, 80),
                                            ))
                                            .rounding(4.0)
                                            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        egui::RichText::new(name).strong().color(
                                                            egui::Color32::from_rgb(200, 250, 215),
                                                        ),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new("[Live Cloud]")
                                                            .size(10.0)
                                                            .color(egui::Color32::from_rgb(90, 200, 130)),
                                                    );
                                                });
                                            });
                                    }
                                });
                            }

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(10.0);

                            // 2. Simulated / Fallback Providers Section
                            ui.label(
                                egui::RichText::new("🟡 Simulated Fallback (Local Mock)")
                                    .strong()
                                    .color(egui::Color32::from_rgb(243, 156, 18)),
                            );
                            ui.add_space(4.0);
                            if sim_active.is_empty() {
                                ui.label(
                                    egui::RichText::new("No providers in simulation fallback")
                                        .italics()
                                        .color(egui::Color32::from_rgb(100, 110, 120)),
                                );
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    for name in &sim_active {
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(52, 44, 25))
                                            .stroke(egui::Stroke::new(
                                                1.0_f32,
                                                egui::Color32::from_rgb(215, 140, 30),
                                            ))
                                            .rounding(4.0)
                                            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        egui::RichText::new(name).strong().color(
                                                            egui::Color32::from_rgb(255, 235, 180),
                                                        ),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new("[Simulated]")
                                                            .size(10.0)
                                                            .color(egui::Color32::from_rgb(243, 156, 18)),
                                                    );
                                                });
                                            });
                                    }
                                });
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new("ℹ Active in local simulation fallback folder (./cloud_simulation/)")
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(190, 160, 100)),
                                );
                            }

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(10.0);

                            // 3. Failed Providers Section
                            ui.label(
                                egui::RichText::new("🔴 Failed")
                                    .strong()
                                    .color(egui::Color32::from_rgb(231, 76, 60)),
                            );
                            ui.add_space(4.0);
                            if failed_list.is_empty() {
                                ui.label(
                                    egui::RichText::new("No failures")
                                        .italics()
                                        .color(egui::Color32::from_rgb(100, 110, 120)),
                                );
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    for name in &failed_list {
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(50, 35, 35))
                                            .stroke(egui::Stroke::new(
                                                1.0_f32,
                                                egui::Color32::from_rgb(180, 50, 50),
                                            ))
                                            .rounding(4.0)
                                            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        egui::RichText::new(name).strong().color(
                                                            egui::Color32::from_rgb(250, 200, 200),
                                                        ),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new("[Failed]")
                                                            .size(10.0)
                                                            .color(egui::Color32::from_rgb(231, 76, 60)),
                                                    );
                                                });
                                            });
                                    }
                                });
                            }
                        });
                    });

                ui.add_space(16.0);
                if let Some(last) = last_update {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Last update: {}ms ago",
                                    last.elapsed().as_millis()
                                ))
                                .font(egui::FontId::proportional(10.0))
                                .color(egui::Color32::from_rgb(100, 110, 120)),
                            );
                        });
                    });
                }
            });

        if self.show_log_modal {
            let mut open = self.show_log_modal;
            egui::Window::new("📄 Daemon Log Viewer")
                .open(&mut open)
                .default_size([720.0, 500.0])
                .resizable(true)
                .show(ctx, |ui| {
                    let log_file = status["log_file"]
                        .as_str()
                        .unwrap_or("daemon.log")
                        .to_string();

                    ui.horizontal(|ui| {
                        if ui.button("🔄 Refresh").clicked() {
                            self.fetch_logs();
                        }
                        if ui.button("↗ Open in Text Editor").clicked() {
                            Self::open_file_externally(&log_file);
                        }
                        ui.separator();
                        ui.label(format!("File: {}", log_file));
                    });
                    ui.separator();

                    let is_loading = *self.is_loading_log.lock().unwrap();
                    if is_loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Loading log file contents...");
                        });
                    } else {
                        let content_guard = self.log_content.lock().unwrap();
                        let mut text = content_guard
                            .clone()
                            .unwrap_or_else(|| "No log content loaded.".to_string());
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut text)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY),
                                );
                            });
                    }
                });
            self.show_log_modal = open;
        }
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([650.0, 560.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "Cloud Sync Dashboard",
        native_options,
        Box::new(|cc| Box::new(CloudSyncApp::new(cc))),
    )
}
