//! # Cloud Sync UI Backend
//!
//! A lightweight HTTP API server built with Axum that routes commands from the web UI
//! to the background daemon via TCP sockets.

#[tokio::main]
async fn main() {
    if let Err(e) = cloud_sync_ui::start_ui_server().await {
        eprintln!("UI server crashed: {}", e);
    }
}
