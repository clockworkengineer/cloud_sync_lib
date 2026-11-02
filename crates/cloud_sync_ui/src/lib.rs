//! # Cloud Sync UI Library
//!
//! Exposes the Axum HTTP API server logic for integration into other runtimes (like Tauri).

pub mod client;
pub mod handlers;
pub mod parser;

pub use client::{DaemonClientError, DaemonClientPort, MockDaemonClient, TcpDaemonClient};
pub use handlers::get_offline_log_path;

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use handlers::{
    api_clear, api_conflicts, api_events, api_logs, api_pause, api_reload, api_resolve_conflict,
    api_resume, api_start, api_status, api_stop, api_sync, api_sync_plan, serve_index,
};

pub const UI_BIND_ADDR: &str = "127.0.0.1:8082";
pub const DAEMON_CONTROL_ADDR: &str = "127.0.0.1:8081";
pub const DAEMON_SPAWN_DELAY_MS: u64 = 1500;
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";
pub const PRIVATE_CONFIG_FILE: &str = "private_config.toml";

/// Returns the configured UI bind address, checking `CLOUDSYNC_UI_ADDR` env var first.
pub fn get_ui_bind_addr() -> String {
    std::env::var("CLOUDSYNC_UI_ADDR").unwrap_or_else(|_| UI_BIND_ADDR.to_string())
}

/// Returns the configured Daemon control socket address, checking `CLOUDSYNC_DAEMON_ADDR` env var first.
pub fn get_daemon_control_addr() -> String {
    std::env::var("CLOUDSYNC_DAEMON_ADDR").unwrap_or_else(|_| DAEMON_CONTROL_ADDR.to_string())
}

/// Builds an Axum router injected with the provided daemon client gateway port.
pub fn build_router(client: Arc<dyn DaemonClientPort>) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/api/status", get(api_status))
        .route("/api/logs", get(api_logs))
        .route("/api/events", get(api_events))
        .route("/api/start", post(api_start))
        .route("/api/pause", post(api_pause))
        .route("/api/resume", post(api_resume))
        .route("/api/sync", post(api_sync))
        .route("/api/sync/execute", post(api_sync))
        .route("/api/reload", post(api_reload))
        .route("/api/stop", post(api_stop))
        .route("/api/clear", post(api_clear))
        .route("/api/conflicts", get(api_conflicts))
        .route("/api/conflicts/resolve", post(api_resolve_conflict))
        .route("/api/sync/plan", get(api_sync_plan))
        .layer(CorsLayer::permissive())
        .with_state(client)
}

/// Starts the Axum HTTP UI server on the configured address.
pub async fn start_ui_server() -> Result<(), std::io::Error> {
    let client = Arc::new(TcpDaemonClient::new(get_daemon_control_addr()));
    let router = build_router(client);

    let bind_addr = get_ui_bind_addr();
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    println!("Decoupled UI server is running on http://{}", bind_addr);

    axum::serve(listener, router).await?;
    Ok(())
}
