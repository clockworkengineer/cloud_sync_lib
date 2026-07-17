//! Axum HTTP endpoint handlers routing to the daemon TCP control socket.

use axum::{
    response::{Html, IntoResponse},
    Json,
};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::net::TcpStream;
use serde_json::json;
use std::time::Duration;
use std::process::Stdio;

use crate::parser::parse_status;
use crate::{
    UI_BIND_ADDR, DAEMON_CONTROL_ADDR, DAEMON_SPAWN_DELAY_MS,
    DEFAULT_CONFIG_FILE, PRIVATE_CONFIG_FILE
};

/// Serves the static HTML user interface dashboard.
pub async fn serve_index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

/// Helper function to transmit a control command to the daemon via local TCP socket.
///
/// # Arguments
/// * `cmd` - The command string to transmit.
///
/// # Returns
/// The raw response string returned by the daemon.
pub async fn send_daemon_cmd(cmd: &str) -> Result<String, std::io::Error> {
    let mut stream = TcpStream::connect(DAEMON_CONTROL_ADDR).await?;
    stream.write_all(format!("{}\n", cmd).as_bytes()).await?;
    stream.flush().await?;
    
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}

/// HTTP Endpoint: Returns the current status of the daemon.
pub async fn api_status() -> impl IntoResponse {
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
                "daemon_running": false,
                "web_ui_address": "-",
            })).into_response()
        }
    }
}

/// HTTP Endpoint: Spawns the background daemon as a detached process if not running.
pub async fn api_start() -> impl IntoResponse {
    let config_file = if std::path::Path::new(PRIVATE_CONFIG_FILE).exists() {
        PRIVATE_CONFIG_FILE
    } else {
        DEFAULT_CONFIG_FILE
    };

    println!("Starting cloud_sync_daemon with config: {}", config_file);

    // Spawn cargo run --bin cloud_sync_daemon private_config.toml as a detached background command
    let mut cmd = tokio::process::Command::new("cargo");
    cmd.arg("run")
       .arg("--bin")
       .arg("cloud_sync_daemon")
       .arg("--")
       .arg(config_file)
       .arg("--ui-addr")
       .arg(UI_BIND_ADDR)
       .stdout(Stdio::null())
       .stderr(Stdio::null())
       .stdin(Stdio::null());

    match cmd.spawn() {
        Ok(_) => {
            // Give it some time to build (if needed) and bind to its TCP socket
            tokio::time::sleep(Duration::from_millis(DAEMON_SPAWN_DELAY_MS)).await;
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
pub async fn api_pause() -> impl IntoResponse {
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
pub async fn api_resume() -> impl IntoResponse {
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
pub async fn api_sync() -> impl IntoResponse {
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
pub async fn api_reload() -> impl IntoResponse {
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
pub async fn api_stop() -> impl IntoResponse {
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

#[derive(serde::Deserialize)]
pub struct ClearRequest {
    pub provider: String,
}

/// HTTP Endpoint: Clears a provider's remote destination.
pub async fn api_clear(Json(payload): axum::Json<ClearRequest>) -> impl IntoResponse {
    let cmd = format!("clear {}", payload.provider);
    match send_daemon_cmd(&cmd).await {
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

use axum::response::sse::{Event, Sse};
use tokio::io::BufReader;
use tokio::io::AsyncBufReadExt;

fn constrain_stream<S>(stream: S) -> S
where
    S: futures_util::stream::Stream<Item = Result<Event, std::io::Error>>,
{
    stream
}

/// HTTP Endpoint: Exposes the Server-Sent Events stream from the daemon.
pub async fn api_events() -> impl IntoResponse {
    let stream = constrain_stream(async_stream::try_stream! {
        let mut tcp_stream = TcpStream::connect(DAEMON_CONTROL_ADDR).await?;
        tcp_stream.write_all(b"subscribe\n").await?;
        tcp_stream.flush().await?;

        let (reader, mut writer) = tcp_stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        loop {
            tokio::select! {
                read_res = reader.read_line(&mut line) => {
                    match read_res {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line.trim();
                            if !trimmed.starts_with("Status:") {
                                yield Event::default().data(trimmed.to_string());
                            }
                            line.clear();
                        }
                        Err(_) => break,
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    if writer.write_all(b"\n").await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}
