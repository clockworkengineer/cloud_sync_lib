//! # Cloud Sync UI Backend
//!
//! A lightweight HTTP API server built with Axum that routes commands from the web UI
//! to the background daemon via TCP sockets.

use axum::{
    routing::{get, post},
    response::{Html, IntoResponse},
    Json, Router,
};
use tower_http::cors::CorsLayer;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::net::TcpStream;
use serde_json::json;
use std::time::Duration;
use std::process::Stdio;

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

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:8082").await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind UI HTTP server to 127.0.0.1:8082: {}", e);
            return;
        }
    };

    println!("Decoupled UI server is running on http://127.0.0.1:8082");

    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("UI server crashed: {}", e);
    }
}

/// Serves the static HTML user interface dashboard.
async fn serve_index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

/// Helper function to transmit a control command to the daemon via local TCP socket.
///
/// # Arguments
/// * `cmd` - The command string to transmit.
///
/// # Returns
/// The raw response string returned by the daemon.
async fn send_daemon_cmd(cmd: &str) -> Result<String, std::io::Error> {
    let mut stream = TcpStream::connect("127.0.0.1:8081").await?;
    stream.write_all(format!("{}\n", cmd).as_bytes()).await?;
    stream.flush().await?;
    
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}

/// Parses the daemon's raw status response into a structured JSON value.
///
/// # Arguments
/// * `raw` - Raw multi-line string output from the daemon's status command.
///
/// # Returns
/// A `serde_json::Value` object containing status details.
fn parse_status(raw: &str) -> serde_json::Value {
    let mut paused = false;
    let mut watch_directory = String::new();
    let mut config_file = String::new();
    let mut active_backends = Vec::new();
    let mut syncing = false;

    for line in raw.lines() {
        if line.starts_with("Paused: ") {
            paused = line.trim_start_matches("Paused: ").trim() == "true";
        } else if line.starts_with("Watch Directory: ") {
            let dir = line.trim_start_matches("Watch Directory: ").trim();
            let dir = dir.strip_prefix('"').unwrap_or(dir);
            let dir = dir.strip_suffix('"').unwrap_or(dir);
            watch_directory = dir.to_string();
        } else if line.starts_with("Config File: ") {
            let file = line.trim_start_matches("Config File: ").trim();
            let file = file.strip_prefix('"').unwrap_or(file);
            let file = file.strip_suffix('"').unwrap_or(file);
            config_file = file.to_string();
        } else if line.starts_with("Active Backends: ") {
            let list_str = line.trim_start_matches("Active Backends: ").trim();
            if list_str.starts_with('[') && list_str.ends_with(']') {
                let inner = &list_str[1..list_str.len() - 1];
                for item in inner.split(',') {
                    let item_clean = item.trim();
                    let item_clean = item_clean.strip_prefix('"').unwrap_or(item_clean);
                    let item_clean = item_clean.strip_suffix('"').unwrap_or(item_clean);
                    if !item_clean.is_empty() {
                        active_backends.push(item_clean.to_string());
                    }
                }
            }
        } else if line.starts_with("Syncing: ") {
            syncing = line.trim_start_matches("Syncing: ").trim() == "true";
        }
    }

    json!({
        "paused": paused,
        "watch_directory": watch_directory,
        "config_file": config_file,
        "active_backends": active_backends,
        "syncing": syncing,
    })
}

/// HTTP Endpoint: Returns the current status of the daemon.
async fn api_status() -> impl IntoResponse {
    match send_daemon_cmd("status").await {
        Ok(raw) => {
            let mut val = parse_status(&raw);
            val["daemon_running"] = serde_json::Value::Bool(true);
            Json(val).into_response()
        }
        Err(_) => {
            Json(json!({
                "paused": false,
                "watch_directory": "-",
                "config_file": "-",
                "active_backends": [],
                "syncing": false,
                "daemon_running": false
            })).into_response()
        }
    }
}

/// HTTP Endpoint: Spawns the background daemon as a detached process if not running.
async fn api_start() -> impl IntoResponse {
    let config_file = if std::path::Path::new("private_config.toml").exists() {
        "private_config.toml"
    } else {
        "config.toml"
    };

    println!("Starting cloud_sync_daemon with config: {}", config_file);

    // Spawn cargo run --bin cloud_sync_daemon private_config.toml as a detached background command
    let mut cmd = tokio::process::Command::new("cargo");
    cmd.arg("run")
       .arg("--bin")
       .arg("cloud_sync_daemon")
       .arg("--")
       .arg(config_file)
       .stdout(Stdio::null())
       .stderr(Stdio::null())
       .stdin(Stdio::null());

    match cmd.spawn() {
        Ok(_) => {
            // Give it 1.5 seconds to build (if needed) and bind to its TCP socket
            tokio::time::sleep(Duration::from_millis(1500)).await;
            Json(json!({ "status": "Daemon started successfully" })).into_response()
        }
        Err(e) => {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Failed to spawn daemon: {}", e) }))
            ).into_response()
        }
    }
}

/// HTTP Endpoint: Commands the daemon to pause synchronization operations.
async fn api_pause() -> impl IntoResponse {
    match send_daemon_cmd("pause").await {
        Ok(raw) => Json(json!({ "status": raw.trim() })).into_response(),
        Err(e) => {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) }))
            ).into_response()
        }
    }
}

/// HTTP Endpoint: Commands the daemon to resume synchronization operations.
async fn api_resume() -> impl IntoResponse {
    match send_daemon_cmd("resume").await {
        Ok(raw) => Json(json!({ "status": raw.trim() })).into_response(),
        Err(e) => {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) }))
            ).into_response()
        }
    }
}

/// HTTP Endpoint: Triggers a manual full synchronization across all enabled backends.
async fn api_sync() -> impl IntoResponse {
    match send_daemon_cmd("sync").await {
        Ok(raw) => Json(json!({ "status": raw.trim() })).into_response(),
        Err(e) => {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) }))
            ).into_response()
        }
    }
}

/// HTTP Endpoint: Reloads the daemon configuration.
async fn api_reload() -> impl IntoResponse {
    match send_daemon_cmd("reload").await {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.starts_with("Error:") {
                Json(json!({ "error": trimmed })).into_response()
            } else {
                Json(json!({ "status": trimmed })).into_response()
            }
        }
        Err(e) => {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) }))
            ).into_response()
        }
    }
}

/// HTTP Endpoint: Requests the daemon to stop executing and terminate gracefully.
async fn api_stop() -> impl IntoResponse {
    match send_daemon_cmd("stop").await {
        Ok(raw) => Json(json!({ "status": raw.trim() })).into_response(),
        Err(e) => {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) }))
            ).into_response()
        }
    }
}
