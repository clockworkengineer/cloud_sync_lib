//! # Cloud Sync UI Library
//!
//! Exposes the Axum HTTP API server logic for integration into other runtimes (like Tauri).

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

/// Starts the Axum HTTP UI server on the configured address.
pub async fn start_ui_server() -> Result<(), std::io::Error> {
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

    let listener = tokio::net::TcpListener::bind(UI_BIND_ADDR).await?;
    println!("Decoupled UI server is running on http://{}", UI_BIND_ADDR);

    axum::serve(listener, router).await?;
    Ok(())
}
