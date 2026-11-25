#![windows_subsystem = "windows"]

use eframe::egui;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct DaemonClient {
    status: serde_json::Value,
    last_update: Option<Instant>,
    last_success_time: Option<chrono::DateTime<chrono::Local>>,
    error_message: Option<String>,
    cached_config_file: Option<String>,
}

impl DaemonClient {
    fn new() -> Self {
        Self {
            status: serde_json::Value::Null,
            last_update: None,
            last_success_time: None,
            error_message: None,
            cached_config_file: None,
        }
    }
}

struct CloudSyncApp {
    client: Arc<Mutex<DaemonClient>>,
    runtime: tokio::runtime::Runtime,
    show_log_modal: bool,
    show_help_modal: bool,
    show_confirm_stop_modal: bool,
    log_content: Arc<Mutex<Option<String>>>,
    is_loading_log: Arc<Mutex<bool>>,
    action_feedback: Option<(String, Instant)>,
    app_icon_texture: Option<egui::TextureHandle>,
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
                            lock.last_success_time = Some(chrono::Local::now());
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

        let app_icon_texture =
            eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon_64.png"))
                .ok()
                .map(|icon| {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [icon.width as usize, icon.height as usize],
                        &icon.rgba,
                    );
                    cc.egui_ctx
                        .load_texture("app_icon", color_image, Default::default())
                });

        Self {
            client,
            runtime,
            show_log_modal: false,
            show_help_modal: false,
            show_confirm_stop_modal: false,
            log_content: Arc::new(Mutex::new(None)),
            is_loading_log: Arc::new(Mutex::new(false)),
            action_feedback: None,
            app_icon_texture,
        }
    }

    async fn send_daemon_command(cmd: &str) -> Result<String, String> {
        let addr =
            std::env::var("CLOUDSYNC_DAEMON_ADDR").unwrap_or_else(|_| "127.0.0.1:8081".to_string());
        let mut stream = TcpStream::connect(&addr).await.map_err(|e| {
            format!(
                "Could not reach daemon at {}. (Is it running?): {}",
                addr, e
            )
        })?;
        stream
            .write_all(format!("{}\n", cmd).as_bytes())
            .await
            .map_err(|e| format!("Communication write error: {}", e))?;
        stream
            .shutdown()
            .await
            .map_err(|e| format!("Connection shutdown error: {}", e))?;

        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("Response read error: {}", e))?;
        Ok(String::from_utf8_lossy(&buf).to_string())
    }

    async fn query_daemon_status() -> Result<serde_json::Value, String> {
        let raw = Self::send_daemon_command("status").await?;
        Ok(cloud_sync_ui::parser::parse_status(&raw))
    }

    fn run_command_sync(&mut self, cmd: &'static str, description: &str) {
        self.action_feedback = Some((description.to_string(), Instant::now()));
        let client_clone = self.client.clone();
        self.runtime.spawn(async move {
            if let Err(e) = Self::send_daemon_command(cmd).await {
                let mut lock = client_clone.lock().unwrap();
                lock.error_message = Some(e);
            }
        });
    }

    fn start_daemon_process(&mut self, config_file: &str) {
        self.action_feedback = Some((
            "Starting CloudSync background service...".to_string(),
            Instant::now(),
        ));
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

            let _ = cmd.spawn();
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
                        .unwrap_or_else(|e| {
                            format!("Could not read log file '{}': {}", file_path, e)
                        })
                }
            };

            *log_content.lock().unwrap() = Some(content);
            *is_loading.lock().unwrap() = false;
        });
    }

    fn handle_keyboard_shortcuts(
        &mut self,
        ctx: &egui::Context,
        paused: bool,
        daemon_online: bool,
    ) {
        // Principle 3: User Control & Freedom - Escape closes any active modal
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_log_modal = false;
            self.show_help_modal = false;
            self.show_confirm_stop_modal = false;
        }

        // Principle 7: Flexibility & Accelerators for Power Users
        if daemon_online {
            // F5 or Ctrl+R -> Immediate Sync
            if ctx.input(|i| {
                i.key_pressed(egui::Key::F5) || (i.modifiers.command && i.key_pressed(egui::Key::R))
            }) {
                self.run_command_sync("sync", "Manual sync triggered via shortcut");
            }

            // Space (when no text input has focus) -> Toggle Pause/Resume
            if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                if paused {
                    self.run_command_sync("resume", "Sync resumed via shortcut");
                } else {
                    self.run_command_sync("pause", "Sync paused via shortcut");
                }
            }
        }

        // Ctrl+L -> Toggle Logs
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::L)) {
            self.show_log_modal = !self.show_log_modal;
            if self.show_log_modal {
                self.fetch_logs();
            }
        }

        // Ctrl+H or F1 -> Toggle Help
        if ctx.input(|i| {
            (i.modifiers.command && i.key_pressed(egui::Key::H)) || i.key_pressed(egui::Key::F1)
        }) {
            self.show_help_modal = !self.show_help_modal;
        }
    }
}

impl eframe::App for CloudSyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (status, last_update, last_success_time, error_message, cached_config_file) = {
            let client = self.client.lock().unwrap();
            (
                client.status.clone(),
                client.last_update,
                client.last_success_time,
                client.error_message.clone(),
                client.cached_config_file.clone(),
            )
        };

        let daemon_online = !status.is_null();
        let paused = status["paused"].as_bool().unwrap_or(false);
        let syncing = status["syncing"].as_bool().unwrap_or(false);

        // Process accelerators and shortcuts
        self.handle_keyboard_shortcuts(ctx, paused, daemon_online);

        // Refined dark styling adjustments
        let panel_frame = egui::Frame::central_panel(&ctx.style())
            .fill(egui::Color32::from_rgb(18, 20, 26))
            .inner_margin(egui::Margin::symmetric(20.0, 16.0));

        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                // Top Header Section with Help & Log Actions
                ui.horizontal(|ui| {
                    if let Some(ref texture) = self.app_icon_texture {
                        ui.image((texture.id(), egui::vec2(28.0, 28.0)));
                        ui.add_space(4.0);
                    }
                    ui.label(
                        egui::RichText::new("CloudSync Control Center")
                            .font(egui::FontId::proportional(22.0))
                            .strong()
                            .color(egui::Color32::from_rgb(240, 244, 252)),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Principle 10: Help and Documentation button
                        let help_btn = egui::Button::new(egui::RichText::new("❓ Help").size(12.0))
                            .rounding(14.0)
                            .fill(egui::Color32::from_rgb(45, 52, 68));
                        if ui.add(help_btn).on_hover_text("View user guide, shortcuts, and troubleshooting (F1 or Ctrl+H)").clicked() {
                            self.show_help_modal = true;
                        }

                        // Logs button
                        let log_btn = egui::Button::new(egui::RichText::new("📄 Logs").size(12.0))
                            .rounding(14.0)
                            .fill(egui::Color32::from_rgb(38, 44, 58));
                        if ui.add(log_btn).on_hover_text("Inspect real-time background logs (Ctrl+L)").clicked() {
                            self.show_log_modal = true;
                            self.fetch_logs();
                        }
                    });
                });

                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Multi-Cloud Continuous File Synchronization & Disaster Recovery")
                        .font(egui::FontId::proportional(11.0))
                        .color(egui::Color32::from_rgb(130, 142, 165)),
                );
                ui.add_space(10.0);

                // Principle 1: Visibility of System Status - Operation feedback banner
                if let Some((ref msg, time)) = self.action_feedback {
                    if time.elapsed() < Duration::from_secs(3) {
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(28, 55, 45))
                            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(46, 204, 113)))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .rounding(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("✔ ").color(egui::Color32::from_rgb(46, 204, 113)).strong());
                                    ui.label(egui::RichText::new(msg).color(egui::Color32::WHITE).size(12.0));
                                });
                            });
                        ui.add_space(6.0);
                    }
                }

                // If daemon is offline, show welcoming recovery screen (Principle 9: Recover from errors)
                if !daemon_online {
                    let config_to_use = if let Some(ref cached) = cached_config_file {
                        cached.clone()
                    } else if std::path::Path::new("private_config.toml").exists() {
                        "private_config.toml".to_string()
                    } else {
                        "config.toml".to_string()
                    };

                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(
                                egui::RichText::new("🛑  Service Disconnected")
                                    .font(egui::FontId::proportional(20.0))
                                    .strong()
                                    .color(egui::Color32::from_rgb(231, 76, 60)),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(
                                    "The CloudSync background daemon is not running on port 8081.",
                                )
                                .color(egui::Color32::from_rgb(180, 190, 210)),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Click 'Start Daemon' below to launch the background service.",
                                )
                                .size(11.0)
                                .color(egui::Color32::from_rgb(140, 150, 170)),
                            );
                            ui.add_space(18.0);

                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(26, 30, 40))
                                .inner_margin(12.0)
                                .rounding(8.0)
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    egui::Color32::from_rgb(46, 52, 66),
                                ))
                                .show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Profile / Config File:").color(egui::Color32::from_rgb(150, 160, 180)));
                                            ui.monospace(&config_to_use);
                                        });
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Log Destination:").color(egui::Color32::from_rgb(150, 160, 180)));
                                            ui.monospace("daemon.log");
                                            if ui.small_button("📄 View Log").clicked() {
                                                self.show_log_modal = true;
                                                self.fetch_logs();
                                            }
                                        });
                                    });
                                });

                            ui.add_space(18.0);

                            let start_btn =
                                egui::Button::new(egui::RichText::new("▶  Start Background Daemon").strong().size(14.0))
                                    .fill(egui::Color32::from_rgb(46, 204, 113))
                                    .min_size(egui::vec2(220.0, 38.0));
                            if ui.add(start_btn).on_hover_text("Spawns the background synchronization service").clicked() {
                                self.start_daemon_process(&config_to_use);
                            }

                            ui.add_space(16.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(egui::RichText::new("Listening for background daemon on 127.0.0.1:8081...").size(11.0).color(egui::Color32::from_rgb(120, 130, 150)));
                            });
                        });
                    });
                    return;
                }

                let watch_dir = status["watch_directory"].as_str().unwrap_or("");
                let clean_watch_dir = if let Some(stripped) = watch_dir.strip_prefix(r"\\?\") {
                    stripped
                } else {
                    watch_dir
                };
                let config_file = status["config_file"].as_str().unwrap_or("");

                // Error Banner (Principle 9: Plain language diagnosis)
                if let Some(ref err) = error_message {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(85, 25, 25))
                        .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(231, 76, 60)))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .rounding(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("⚠ Notice:")
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                );
                                ui.label(
                                    egui::RichText::new(err)
                                        .color(egui::Color32::from_rgb(250, 210, 210)),
                                );
                            });
                        });
                    ui.add_space(8.0);
                }

                // --- Card 1: System Info (Principle 1 & 2: System Status & Real World Language) ---
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(26, 30, 40))
                    .inner_margin(12.0)
                    .rounding(8.0)
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(46, 52, 66),
                    ))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("SYSTEM OVERVIEW")
                                        .strong()
                                        .color(egui::Color32::from_rgb(130, 142, 165)),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if let Some(time) = last_success_time {
                                        ui.label(
                                            egui::RichText::new(format!("Last checked: {}", time.format("%H:%M:%S")))
                                                .size(10.0)
                                                .color(egui::Color32::from_rgb(110, 120, 140)),
                                        );
                                    }
                                });
                            });
                            ui.add_space(6.0);

                            egui::Grid::new("status_grid")
                                .num_columns(2)
                                .spacing([24.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("Sync Status:")
                                            .color(egui::Color32::from_rgb(170, 180, 200)),
                                    );
                                    if paused {
                                        ui.label(
                                            egui::RichText::new("⏸ Paused (Idle)")
                                                .strong()
                                                .color(egui::Color32::from_rgb(230, 180, 34)),
                                        ).on_hover_text("File monitoring is suspended. Press Space or 'Resume' to continue syncing.");
                                    } else if syncing {
                                        ui.label(
                                            egui::RichText::new("🔄 Syncing changes...")
                                                .strong()
                                                .color(egui::Color32::from_rgb(52, 152, 219)),
                                        ).on_hover_text("Actively scanning or transferring files with cloud storage backends.");
                                    } else {
                                        ui.label(
                                            egui::RichText::new("🟢 Active (Continuous Monitoring)")
                                                .strong()
                                                .color(egui::Color32::from_rgb(46, 204, 113)),
                                        ).on_hover_text("Monitoring watched folder in real-time. Ready for changes.");
                                    }
                                    ui.end_row();

                                    ui.label(
                                        egui::RichText::new("Watched Folder:")
                                            .color(egui::Color32::from_rgb(170, 180, 200)),
                                    );
                                    ui.monospace(clean_watch_dir).on_hover_text("Local directory monitored for additions, changes, and deletions.");
                                    ui.end_row();

                                    ui.label(
                                        egui::RichText::new("Configuration File:")
                                            .color(egui::Color32::from_rgb(170, 180, 200)),
                                    );
                                    ui.monospace(config_file).on_hover_text("Active profile configuration path.");
                                    ui.end_row();

                                    let log_file = status["log_file"].as_str().unwrap_or("daemon.log");
                                    ui.label(
                                        egui::RichText::new("Activity Log:")
                                            .color(egui::Color32::from_rgb(170, 180, 200)),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.monospace(log_file);
                                        if ui.small_button("📄 View").on_hover_text("Open log viewer modal").clicked() {
                                            self.show_log_modal = true;
                                            self.fetch_logs();
                                        }
                                        if ui.small_button("↗ External").on_hover_text("Open log file in system default editor").clicked() {
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
                                        .text("Synchronizing files with remote cloud providers...")
                                        .fill(egui::Color32::from_rgb(52, 152, 219)),
                                );
                            }
                        });
                    });

                ui.add_space(10.0);

                // --- Card 2: Control Actions (Principle 3, 5 & 7: Control, Prevention & Accelerators) ---
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(26, 30, 40))
                    .inner_margin(12.0)
                    .rounding(8.0)
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(46, 52, 66),
                    ))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("OPERATIONS CENTER")
                                    .strong()
                                    .color(egui::Color32::from_rgb(130, 142, 165)),
                            );
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;

                                if paused {
                                    let resume_btn = egui::Button::new(
                                        egui::RichText::new("▶  Resume (Space)").strong(),
                                    )
                                    .fill(egui::Color32::from_rgb(39, 174, 96))
                                    .min_size(egui::vec2(130.0, 32.0));
                                    if ui.add(resume_btn).on_hover_text("Resume file synchronization (Keyboard shortcut: Space)").clicked() {
                                        self.run_command_sync("resume", "Sync resumed");
                                    }
                                } else {
                                    let pause_btn = egui::Button::new(
                                        egui::RichText::new("⏸  Pause (Space)").strong(),
                                    )
                                    .fill(egui::Color32::from_rgb(230, 126, 34))
                                    .min_size(egui::vec2(130.0, 32.0));
                                    if ui.add(pause_btn).on_hover_text("Pause file synchronization (Keyboard shortcut: Space)").clicked() {
                                        self.run_command_sync("pause", "Sync paused");
                                    }
                                }

                                let sync_btn =
                                    egui::Button::new(egui::RichText::new("🔄  Sync Now (F5)").strong())
                                        .fill(egui::Color32::from_rgb(41, 128, 185))
                                        .min_size(egui::vec2(120.0, 32.0));
                                if ui.add(sync_btn).on_hover_text("Perform immediate differential scan across all backends (Keyboard shortcut: F5 or Ctrl+R)").clicked() {
                                    self.run_command_sync("sync", "Manual sync triggered");
                                }

                                let reload_btn =
                                    egui::Button::new(egui::RichText::new("⚡  Reload Config").strong())
                                        .fill(egui::Color32::from_rgb(52, 73, 94))
                                        .min_size(egui::vec2(115.0, 32.0));
                                if ui.add(reload_btn).on_hover_text("Re-reads private_config.toml and reconnects modified providers").clicked() {
                                    self.run_command_sync("reload", "Configuration reloaded");
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let stop_btn = egui::Button::new(
                                            egui::RichText::new("🛑  Stop Service").strong(),
                                        )
                                        .fill(egui::Color32::from_rgb(192, 57, 43))
                                        .min_size(egui::vec2(110.0, 32.0));
                                        // Principle 5: Error Prevention - prompt confirmation before stopping
                                        if ui.add(stop_btn).on_hover_text("Shuts down the background synchronization daemon").clicked() {
                                            self.show_confirm_stop_modal = true;
                                        }
                                    },
                                );
                            });
                        });
                    });

                ui.add_space(10.0);

                // --- Card 3: Storage Providers (Principle 1 & 6: System Status & Recognition) ---
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(26, 30, 40))
                    .inner_margin(12.0)
                    .rounding(8.0)
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(46, 52, 66),
                    ))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("STORAGE BACKENDS & DESTINATIONS")
                                    .strong()
                                    .color(egui::Color32::from_rgb(130, 142, 165)),
                            );
                            ui.add_space(8.0);

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
                                egui::RichText::new("🟢 Live Cloud Backends (Active & Connected)")
                                    .strong()
                                    .color(egui::Color32::from_rgb(46, 204, 113)),
                            );
                            ui.add_space(4.0);
                            if live_cloud.is_empty() {
                                ui.label(
                                    egui::RichText::new("No providers currently connected to live cloud storage.")
                                        .italics()
                                        .color(egui::Color32::from_rgb(110, 120, 135)),
                                );
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    for name in &live_cloud {
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(32, 48, 38))
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
                                            }).response.on_hover_text(format!("{} is active and syncing to remote cloud storage", name));
                                    }
                                });
                            }

                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(8.0);

                            // 2. Simulated / Fallback Providers Section
                            ui.label(
                                egui::RichText::new("🟡 Local Backup & Simulation Mode (Offline Sandbox)")
                                    .strong()
                                    .color(egui::Color32::from_rgb(243, 156, 18)),
                            );
                            ui.add_space(4.0);
                            if sim_active.is_empty() {
                                ui.label(
                                    egui::RichText::new("No providers operating in simulation mode.")
                                        .italics()
                                        .color(egui::Color32::from_rgb(110, 120, 135)),
                                );
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    for name in &sim_active {
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(48, 40, 24))
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
                                            }).response.on_hover_text(format!("{} is running in local simulation (syncing to ./cloud_simulation/{})", name, name.to_lowercase()));
                                    }
                                });
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new("ℹ Credentials unconfigured or held. Files mirror safely to local folder ./cloud_simulation/")
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(190, 160, 100)),
                                );
                            }

                            if !failed_list.is_empty() {
                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                // 3. Failed Providers Section
                                ui.label(
                                    egui::RichText::new("🔴 Connection Needs Attention")
                                        .strong()
                                        .color(egui::Color32::from_rgb(231, 76, 60)),
                                );
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    for name in &failed_list {
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(48, 30, 30))
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
                                                        egui::RichText::new("[Attention]")
                                                            .size(10.0)
                                                            .color(egui::Color32::from_rgb(231, 76, 60)),
                                                    );
                                                });
                                            }).response.on_hover_text(format!("{} encountered authentication errors. Please check your token or credentials.", name));
                                    }
                                });
                            }
                        });
                    });

                ui.add_space(10.0);
                if let Some(last) = last_update {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Telemetry refreshed {}ms ago",
                                    last.elapsed().as_millis()
                                ))
                                .font(egui::FontId::proportional(10.0))
                                .color(egui::Color32::from_rgb(100, 110, 125)),
                            );
                        });
                    });
                }
            });

        // Modal 1: Confirmation Prompt before stopping daemon (Principle 5: Error Prevention)
        if self.show_confirm_stop_modal {
            egui::Window::new("🛑 Confirm Service Shutdown")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.set_width(360.0);
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Are you sure you want to stop the background synchronization service?")
                            .color(egui::Color32::WHITE),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("File monitoring and cloud uploads will be halted until you restart the service.")
                            .size(11.0)
                            .color(egui::Color32::from_rgb(170, 180, 200)),
                    );
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel (Esc)").clicked() {
                            self.show_confirm_stop_modal = false;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let confirm_btn = egui::Button::new(egui::RichText::new("Yes, Stop Service").strong())
                                .fill(egui::Color32::from_rgb(192, 57, 43));
                            if ui.add(confirm_btn).clicked() {
                                self.show_confirm_stop_modal = false;
                                self.run_command_sync("stop", "Service stopped");
                            }
                        });
                    });
                });
        }

        // Modal 2: In-App Help & Documentation Dialog (Principle 10: Help & Documentation)
        if self.show_help_modal {
            let mut open = self.show_help_modal;
            egui::Window::new("📖 CloudSync Help & Shortcuts")
                .open(&mut open)
                .default_size([640.0, 460.0])
                .resizable(true)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Keyboard Shortcuts")
                                .strong()
                                .size(14.0)
                                .color(egui::Color32::from_rgb(52, 152, 219)),
                        );
                        ui.add_space(4.0);
                        egui::Grid::new("shortcuts_grid")
                            .num_columns(2)
                            .spacing([24.0, 6.0])
                            .show(ui, |ui| {
                                ui.monospace("Space");
                                ui.label("Pause or Resume synchronization");
                                ui.end_row();

                                ui.monospace("F5 / Ctrl + R");
                                ui.label("Trigger immediate 3-way synchronization scan");
                                ui.end_row();

                                ui.monospace("Ctrl + L");
                                ui.label("Open or close daemon activity log viewer");
                                ui.end_row();

                                ui.monospace("F1 / Ctrl + H");
                                ui.label("Toggle this help dialog");
                                ui.end_row();

                                ui.monospace("Esc");
                                ui.label("Close any open dialog or modal");
                                ui.end_row();
                            });

                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(12.0);

                        ui.label(
                            egui::RichText::new("Status Indicators Guide")
                                .strong()
                                .size(14.0)
                                .color(egui::Color32::from_rgb(52, 152, 219)),
                        );
                        ui.add_space(4.0);
                        egui::Grid::new("badges_grid")
                            .num_columns(2)
                            .spacing([24.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("🟢 Live Cloud").strong().color(egui::Color32::from_rgb(46, 204, 113)));
                                ui.label("Valid credentials; changes mirror directly to remote cloud servers.");
                                ui.end_row();

                                ui.label(egui::RichText::new("🟡 Simulated").strong().color(egui::Color32::from_rgb(243, 156, 18)));
                                ui.label("Credentials absent or on email verification hold; changes mirror safely into ./cloud_simulation/.");
                                ui.end_row();

                                ui.label(egui::RichText::new("🔴 Attention").strong().color(egui::Color32::from_rgb(231, 76, 60)));
                                ui.label("Provider rejected authentication tokens. Re-run OAuth token helper script.");
                                ui.end_row();
                            });

                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(12.0);

                        ui.label(
                            egui::RichText::new("Documentation & Setup Guides")
                                .strong()
                                .size(14.0)
                                .color(egui::Color32::from_rgb(52, 152, 219)),
                        );
                        ui.add_space(4.0);
                        ui.label("For in-depth setup instructions, see the Markdown guides in the docs/ directory:");
                        ui.horizontal(|ui| {
                            if ui.button("📖 Architecture Guide").clicked() {
                                Self::open_file_externally("docs/ARCHITECTURE.md");
                            }
                            if ui.button("🛠 Troubleshooting").clicked() {
                                Self::open_file_externally("docs/TROUBLESHOOTING.md");
                            }
                            if ui.button("⚙ MEGA Setup").clicked() {
                                Self::open_file_externally("docs/setup/MEGA_SETUP.md");
                            }
                        });
                    });
                });
            self.show_help_modal = open;
        }

        // Modal 3: Daemon Log Viewer
        if self.show_log_modal {
            let mut open = self.show_log_modal;
            egui::Window::new("📄 Background Activity Log")
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
                        ui.label(format!("Active file: {}", log_file));
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

fn load_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon_256.png")).ok()
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([720.0, 620.0])
        .with_resizable(true);

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Cloud Sync Dashboard",
        native_options,
        Box::new(|cc| Box::new(CloudSyncApp::new(cc))),
    )
}
