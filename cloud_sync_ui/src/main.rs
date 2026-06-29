//! # Cloud Sync UI Backend
//!
//! A lightweight HTTP API server built with Axum that routes commands from the web UI
//! to the background daemon via TCP sockets.

pub mod parser;
pub mod handlers;

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;

use handlers::{
    serve_index, api_status, api_start, api_pause, api_resume, api_sync,
    api_reload, api_stop
};

pub const UI_BIND_ADDR: &str = "127.0.0.1:8082";
pub const DAEMON_CONTROL_ADDR: &str = "127.0.0.1:8081";
pub const DAEMON_SPAWN_DELAY_MS: u64 = 1500;
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";
pub const PRIVATE_CONFIG_FILE: &str = "private_config.toml";

#[tokio::main]
async fn main() {
    let router = Router::new()
        .route("/", get(serve_index))
        .route("/api/status", get(api_status))
        .route("/api/start", post(api_start))
        .route("/api/pause", post(api_pause))
        .route("/api/resume", post(api_resume))
        .route("/api/sync", post(api_sync))
        .route("/api/reload", post(api_reload))
        .route("/api/stop", post(api_stop))
        .layer(CorsLayer::permissive());

    let listener = match tokio::net::TcpListener::bind(UI_BIND_ADDR).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind UI HTTP server to {}: {}", UI_BIND_ADDR, e);
            return;
        }
    };

    println!("Decoupled UI server is running on http://{}", UI_BIND_ADDR);

    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("UI server crashed: {}", e);
    }
}
