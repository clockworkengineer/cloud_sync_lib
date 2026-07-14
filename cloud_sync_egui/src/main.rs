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
        // Refined dark styling adjustments
        let panel_frame = egui::Frame::central_panel(&ctx.style())
            .fill(egui::Color32::from_rgb(20, 22, 26))
            .inner_margin(egui::Margin::same(16.0));

        egui::CentralPanel::default().frame(panel_frame).show(ctx, |ui| {
            // Header Section
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("☁  Cloud Sync Dashboard")
                    .font(egui::FontId::proportional(22.0))
                    .strong()
                    .color(egui::Color32::from_rgb(240, 242, 248)));
                ui.add_space(2.0);
                ui.label(egui::RichText::new("Real-time Multi-Cloud Synchronizer")
                    .font(egui::FontId::proportional(11.0))
                    .color(egui::Color32::from_rgb(120, 130, 150)));
                ui.add_space(12.0);
            });

            let client = self.client.lock().unwrap();

            if client.status.is_null() {
                ui.centered_and_justified(|ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new("Connecting to background sync daemon...")
                            .color(egui::Color32::from_rgb(160, 170, 190)));
                    });
                });
                return;
            }

            let status = &client.status;
            let paused = status["paused"].as_bool().unwrap_or(false);
            let syncing = status["syncing"].as_bool().unwrap_or(false);
            let watch_dir = status["watch_directory"].as_str().unwrap_or("");
            let config_file = status["config_file"].as_str().unwrap_or("");

            // Error Banner (if any error in client)
            if let Some(ref err) = client.error_message {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(80, 20, 20))
                    .inner_margin(8.0)
                    .rounding(6.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("⚠ Error:").strong().color(egui::Color32::WHITE));
                            ui.label(egui::RichText::new(err).color(egui::Color32::from_rgb(240, 200, 200)));
                        });
                    });
                ui.add_space(10.0);
            }

            // --- Card 1: System Info ---
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(28, 30, 38))
                .inner_margin(12.0)
                .rounding(8.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(44, 48, 58)))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("SYSTEM STATUS").strong().color(egui::Color32::from_rgb(110, 120, 140)));
                        ui.add_space(6.0);

                        egui::Grid::new("status_grid")
                            .num_columns(2)
                            .spacing([24.0, 8.0])
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("Daemon Status:").color(egui::Color32::from_rgb(170, 180, 200)));
                                if paused {
                                    ui.label(egui::RichText::new("⏸ Paused").strong().color(egui::Color32::from_rgb(230, 180, 34)));
                                } else if syncing {
                                    ui.label(egui::RichText::new("🔄 Syncing...").strong().color(egui::Color32::from_rgb(52, 152, 219)));
                                } else {
                                    ui.label(egui::RichText::new("🟢 Active (Idle)").strong().color(egui::Color32::from_rgb(46, 204, 113)));
                                }
                                ui.end_row();

                                ui.label(egui::RichText::new("Watch Folder:").color(egui::Color32::from_rgb(170, 180, 200)));
                                ui.monospace(watch_dir);
                                ui.end_row();

                                ui.label(egui::RichText::new("Config File:").color(egui::Color32::from_rgb(170, 180, 200)));
                                ui.monospace(config_file);
                                ui.end_row();
                            });

                        if syncing {
                            ui.add_space(8.0);
                            ui.add(egui::ProgressBar::new(1.0)
                                .animate(true)
                                .text("Syncing file modifications to remote providers...")
                                .fill(egui::Color32::from_rgb(52, 152, 219)));
                        }
                    });
                });

            ui.add_space(12.0);

            // --- Card 2: Control Actions ---
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(28, 30, 38))
                .inner_margin(12.0)
                .rounding(8.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(44, 48, 58)))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("ACTIONS").strong().color(egui::Color32::from_rgb(110, 120, 140)));
                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;

                            if paused {
                                let resume_btn = egui::Button::new(egui::RichText::new("▶  Resume Syncing").strong())
                                    .fill(egui::Color32::from_rgb(39, 174, 96))
                                    .min_size(egui::vec2(130.0, 30.0));
                                if ui.add(resume_btn).clicked() {
                                    self.run_command_sync("resume");
                                }
                            } else {
                                let pause_btn = egui::Button::new(egui::RichText::new("⏸  Pause Syncing").strong())
                                    .fill(egui::Color32::from_rgb(230, 126, 34))
                                    .min_size(egui::vec2(130.0, 30.0));
                                if ui.add(pause_btn).clicked() {
                                    self.run_command_sync("pause");
                                }
                            }

                            let sync_btn = egui::Button::new(egui::RichText::new("🔄  Sync Now").strong())
                                .fill(egui::Color32::from_rgb(41, 128, 185))
                                .min_size(egui::vec2(110.0, 30.0));
                            if ui.add(sync_btn).clicked() {
                                self.run_command_sync("sync");
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let stop_btn = egui::Button::new(egui::RichText::new("🛑  Stop Daemon").strong())
                                    .fill(egui::Color32::from_rgb(192, 57, 43))
                                    .min_size(egui::vec2(110.0, 30.0));
                                if ui.add(stop_btn).clicked() {
                                    self.run_command_sync("stop");
                                }
                            });
                        });
                    });
                });

            ui.add_space(12.0);

            // --- Card 3: Storage Providers ---
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(28, 30, 38))
                .inner_margin(12.0)
                .rounding(8.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(44, 48, 58)))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("STORAGE PROVIDERS").strong().color(egui::Color32::from_rgb(110, 120, 140)));
                        ui.add_space(8.0);

                        // Active Providers Section
                        ui.label(egui::RichText::new("🟢 Active").strong().color(egui::Color32::from_rgb(46, 204, 113)));
                        ui.add_space(4.0);
                        if let Some(active) = status["active_backends"].as_array() {
                            if active.is_empty() {
                                ui.label(egui::RichText::new("None active").italics().color(egui::Color32::from_rgb(100, 110, 120)));
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    for backend in active {
                                        let name = backend.as_str().unwrap_or("");
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(35, 45, 38))
                                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(46, 113, 60)))
                                            .rounding(4.0)
                                            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                            .show(ui, |ui| {
                                                ui.label(egui::RichText::new(name).strong().color(egui::Color32::from_rgb(200, 240, 210)));
                                            });
                                    }
                                });
                            }
                        }

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        // Failed Providers Section
                        ui.label(egui::RichText::new("🔴 Failed").strong().color(egui::Color32::from_rgb(231, 76, 60)));
                        ui.add_space(4.0);
                        if let Some(failed) = status["failed_backends"].as_array() {
                            if failed.is_empty() {
                                ui.label(egui::RichText::new("No failures").italics().color(egui::Color32::from_rgb(100, 110, 120)));
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    for backend in failed {
                                        let name = backend.as_str().unwrap_or("");
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(50, 35, 35))
                                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(180, 50, 50)))
                                            .rounding(4.0)
                                            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                            .show(ui, |ui| {
                                                ui.label(egui::RichText::new(name).strong().color(egui::Color32::from_rgb(250, 200, 200)));
                                            });
                                    }
                                });
                            }
                        }
                    });
                });

            ui.add_space(16.0);
            if let Some(last) = client.last_update {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("Last update: {}ms ago", last.elapsed().as_millis()))
                            .font(egui::FontId::proportional(10.0))
                            .color(egui::Color32::from_rgb(100, 110, 120)));
                    });
                });
            }
        });
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
