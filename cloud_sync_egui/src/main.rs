#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct DaemonClient {
    status: serde_json::Value,
    last_update: Option<Instant>,
    error_message: Option<String>,
}

impl DaemonClient {
    fn new() -> Self {
        Self {
            status: serde_json::Value::Null,
            last_update: None,
            error_message: None,
        }
    }
}

struct CloudSyncApp {
    client: Arc<Mutex<DaemonClient>>,
    runtime: tokio::runtime::Runtime,
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
                            lock.status = val;
                            lock.last_update = Some(Instant::now());
                            lock.error_message = None;
                        }
                        Err(e) => {
                            lock.error_message = Some(e);
                        }
                    }
                }
                ctx.request_repaint();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        Self { client, runtime }
    }

    async fn send_daemon_command(cmd: &str) -> Result<String, String> {
        let mut stream = TcpStream::connect("127.0.0.1:8081")
            .await
            .map_err(|e| format!("Connection error: {}", e))?;
        stream.write_all(format!("{}\n", cmd).as_bytes())
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        stream.shutdown().await.map_err(|e| format!("Shutdown error: {}", e))?;

        let mut buf = Vec::new();
        stream.read_to_end(&mut buf)
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
}

impl eframe::App for CloudSyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("☁ Cloud Sync Dashboard");
                ui.add_space(8.0);
            });

            let client = self.client.lock().unwrap();

            if client.status.is_null() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Connecting to background sync daemon...");
                });
                return;
            }

            let status = &client.status;
            let paused = status["paused"].as_bool().unwrap_or(false);
            let syncing = status["syncing"].as_bool().unwrap_or(false);
            let watch_dir = status["watch_directory"].as_str().unwrap_or("");
            let config_file = status["config_file"].as_str().unwrap_or("");

            // Render Status Grid
            egui::Frame::none()
                .fill(egui::Color32::from_gray(30))
                .inner_margin(8.0)
                .rounding(6.0)
                .show(ui, |ui| {
                    egui::Grid::new("status_grid")
                        .num_columns(2)
                        .spacing([40.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Daemon Status:");
                            if paused {
                                ui.colored_label(egui::Color32::YELLOW, "⏸ Paused");
                            } else if syncing {
                                ui.colored_label(egui::Color32::LIGHT_BLUE, "🔄 Syncing...");
                            } else {
                                ui.colored_label(egui::Color32::LIGHT_GREEN, "🟢 Running (Idle)");
                            }
                            ui.end_row();

                            ui.label("Watch Folder:");
                            ui.monospace(watch_dir);
                            ui.end_row();

                            ui.label("Config File:");
                            ui.monospace(config_file);
                            ui.end_row();
                        });
                });

            ui.add_space(16.0);

            // Render Control Actions Panel
            ui.heading("Actions");
            ui.horizontal(|ui| {
                if paused {
                    if ui.button("▶ Resume Syncing").clicked() {
                        self.run_command_sync("start");
                    }
                } else {
                    if ui.button("⏸ Pause Syncing").clicked() {
                        self.run_command_sync("pause");
                    }
                }

                if ui.button("🔄 Sync Now").clicked() {
                    self.run_command_sync("sync");
                }

                if ui.button("🛑 Stop Daemon").clicked() {
                    self.run_command_sync("stop");
                }
            });

            ui.add_space(16.0);

            // Render Backends Lists
            ui.columns(2, |columns| {
                columns[0].vertical(|ui| {
                    ui.heading("🟢 Active Providers");
                    ui.add_space(4.0);
                    if let Some(active) = status["active_backends"].as_array() {
                        if active.is_empty() {
                            ui.label("None active.");
                        } else {
                            for backend in active {
                                ui.label(format!("• {}", backend.as_str().unwrap_or("")));
                            }
                        }
                    }
                });

                columns[1].vertical(|ui| {
                    ui.heading("🔴 Failed Providers");
                    ui.add_space(4.0);
                    if let Some(failed) = status["failed_backends"].as_array() {
                        if failed.is_empty() {
                            ui.label("No failures.");
                        } else {
                            for backend in failed {
                                ui.colored_label(egui::Color32::LIGHT_RED, format!("• {}", backend.as_str().unwrap_or("")));
                            }
                        }
                    }
                });
            });

            ui.add_space(16.0);
            if let Some(last) = client.last_update {
                ui.horizontal(|ui| {
                    ui.small(format!("Last update: {}ms ago", last.elapsed().as_millis()));
                });
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 360.0])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "Cloud Sync Dashboard",
        native_options,
        Box::new(|cc| Box::new(CloudSyncApp::new(cc))),
    )
}
