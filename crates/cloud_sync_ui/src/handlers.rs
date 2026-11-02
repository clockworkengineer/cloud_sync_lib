//! Axum HTTP endpoint handlers routing to the daemon TCP control socket.

use axum::{
    extract::State,
    response::{Html, IntoResponse},
    Json,
};
use serde_json::json;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::client::DaemonClientPort;
use crate::parser::parse_status;
use crate::{DAEMON_SPAWN_DELAY_MS, DEFAULT_CONFIG_FILE, PRIVATE_CONFIG_FILE};

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
    let client = crate::TcpDaemonClient::new(crate::get_daemon_control_addr());
    client.send_command(cmd).await.map_err(|e| match e {
        crate::DaemonClientError::Io(err) => err,
        other => std::io::Error::other(other.to_string()),
    })
}

/// Helper to determine active configured log file path when daemon is offline.
pub fn get_offline_log_path() -> String {
    let config_file = if std::path::Path::new(PRIVATE_CONFIG_FILE).exists() {
        PRIVATE_CONFIG_FILE
    } else {
        DEFAULT_CONFIG_FILE
    };
    if let Ok(content) = std::fs::read_to_string(config_file) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("log_file") {
                if let Some((_, val)) = line.split_once('=') {
                    let val = val.trim().trim_matches('"').trim_matches('\'');
                    if !val.is_empty() {
                        return val.to_string();
                    }
                }
            }
        }
    }
    "daemon.log".to_string()
}

/// HTTP Endpoint: Returns the current status of the daemon.
pub async fn api_status(State(client): State<Arc<dyn DaemonClientPort>>) -> impl IntoResponse {
    match client.send_command("status").await {
        Ok(raw) => {
            let mut val = parse_status(&raw);
            val["daemon_running"] = serde_json::Value::Bool(true);
            Json(val).into_response()
        }
        Err(_) => Json(json!({
            "paused": false,
            "watch_directory": "-",
            "config_file": "-",
            "active_backends": [],
            "failed_backends": [],
            "simulated_backends": [],
            "backend_details": [],
            "syncing": false,
            "daemon_running": false,
            "web_ui_address": "-",
            "log_file": get_offline_log_path(),
        }))
        .into_response(),
    }
}

/// HTTP Endpoint: Serves the active daemon log file as plain text.
pub async fn api_logs(State(client): State<Arc<dyn DaemonClientPort>>) -> impl IntoResponse {
    if let Ok(resp) = client.send_command("get_log").await {
        if resp.starts_with("Status: OK") {
            if let Some(pos) = resp.find("\n\n") {
                let body = &resp[pos + 2..];
                return (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/plain; charset=utf-8",
                    )],
                    body.to_string(),
                )
                    .into_response();
            }
        }
    }

    let offline_path = get_offline_log_path();
    match tokio::fs::read_to_string(&offline_path).await {
        Ok(content) => (
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            content,
        )
            .into_response(),
        Err(e) => (
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            format!(
                "(Daemon is offline and could not read local log file '{}': {})",
                offline_path, e
            ),
        )
            .into_response(),
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
        println!("Spawning daemon binary: {:?}", bin_path);
        let mut c = tokio::process::Command::new(bin_path);
        c.arg(config_file);
        c
    } else {
        println!("Spawning daemon via cargo run");
        let mut c = tokio::process::Command::new("cargo");
        c.arg("run")
            .arg("--bin")
            .arg("cloud_sync_daemon")
            .arg("--")
            .arg(config_file);
        c
    };

    cmd.arg("--ui-addr")
        .arg(crate::get_ui_bind_addr())
        .arg("--control-addr")
        .arg(crate::get_daemon_control_addr())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    match cmd.spawn() {
        Ok(_) => {
            // Give it some time to build (if needed) and bind to its TCP socket
            tokio::time::sleep(Duration::from_millis(DAEMON_SPAWN_DELAY_MS)).await;
            Json(json!({ "status": "Daemon started successfully" })).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to spawn daemon: {}", e) })),
        )
            .into_response(),
    }
}

/// HTTP Endpoint: Commands the daemon to pause synchronization operations.
pub async fn api_pause(State(client): State<Arc<dyn DaemonClientPort>>) -> impl IntoResponse {
    match client.send_command("pause").await {
        Ok(raw) => Json(json!({ "status": raw.trim() })).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) })),
        )
            .into_response(),
    }
}

/// HTTP Endpoint: Commands the daemon to resume synchronization operations.
pub async fn api_resume(State(client): State<Arc<dyn DaemonClientPort>>) -> impl IntoResponse {
    match client.send_command("resume").await {
        Ok(raw) => Json(json!({ "status": raw.trim() })).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) })),
        )
            .into_response(),
    }
}

/// HTTP Endpoint: Triggers a manual full synchronization across all enabled backends.
pub async fn api_sync(State(client): State<Arc<dyn DaemonClientPort>>) -> impl IntoResponse {
    match client.send_command("sync").await {
        Ok(raw) => Json(json!({ "status": raw.trim() })).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) })),
        )
            .into_response(),
    }
}

/// HTTP Endpoint: Reloads the daemon configuration.
pub async fn api_reload(State(client): State<Arc<dyn DaemonClientPort>>) -> impl IntoResponse {
    match client.send_command("reload").await {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.starts_with("Error:") {
                Json(json!({ "error": trimmed })).into_response()
            } else {
                Json(json!({ "status": trimmed })).into_response()
            }
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) })),
        )
            .into_response(),
    }
}

/// HTTP Endpoint: Requests the daemon to stop executing and terminate gracefully.
pub async fn api_stop(State(client): State<Arc<dyn DaemonClientPort>>) -> impl IntoResponse {
    match client.send_command("stop").await {
        Ok(raw) => Json(json!({ "status": raw.trim() })).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct ClearRequest {
    pub provider: String,
}

/// HTTP Endpoint: Clears a provider's remote destination.
pub async fn api_clear(
    State(client): State<Arc<dyn DaemonClientPort>>,
    Json(payload): axum::Json<ClearRequest>,
) -> impl IntoResponse {
    let cmd = format!("clear {}", payload.provider);
    match client.send_command(&cmd).await {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.starts_with("Error:") {
                Json(json!({ "error": trimmed })).into_response()
            } else {
                Json(json!({ "status": trimmed })).into_response()
            }
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct ResolveConflictRequest {
    pub rel_path: String,
    pub resolution: String,
}

/// HTTP Endpoint: Lists all pending conflicts from the daemon.
pub async fn api_conflicts(State(client): State<Arc<dyn DaemonClientPort>>) -> impl IntoResponse {
    match client.send_command("conflicts").await {
        Ok(raw) => {
            let trimmed = raw.trim();
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                Json(val).into_response()
            } else {
                Json(json!([])).into_response()
            }
        }
        Err(_) => Json(json!([])).into_response(),
    }
}

/// HTTP Endpoint: Resolves a conflict with a specific choice (use_local, use_remote, keep_both, skip).
pub async fn api_resolve_conflict(
    State(client): State<Arc<dyn DaemonClientPort>>,
    Json(payload): axum::Json<ResolveConflictRequest>,
) -> impl IntoResponse {
    let cmd = format!(
        "resolve_conflict {} {}",
        payload.rel_path, payload.resolution
    );
    match client.send_command(&cmd).await {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.starts_with("Error:") {
                Json(json!({ "error": trimmed })).into_response()
            } else {
                Json(json!({ "status": trimmed })).into_response()
            }
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) })),
        )
            .into_response(),
    }
}

fn constrain_stream<S>(stream: S) -> S
where
    S: futures_util::stream::Stream<Item = Result<axum::response::sse::Event, std::io::Error>>,
{
    stream
}

/// HTTP Endpoint: Exposes the Server-Sent Events stream from the daemon.
pub async fn api_events() -> impl IntoResponse {
    let stream = constrain_stream(async_stream::try_stream! {
        let mut tcp_stream = TcpStream::connect(crate::get_daemon_control_addr()).await?;
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
                                yield axum::response::sse::Event::default().data(trimmed);
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

    axum::response::sse::Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

/// HTTP Endpoint: Returns active or preview synchronization plans across backends.
pub async fn api_sync_plan(State(client): State<Arc<dyn DaemonClientPort>>) -> impl IntoResponse {
    match client.send_command("get_plan").await {
        Ok(raw) => {
            let json_val: serde_json::Value =
                serde_json::from_str(&raw).unwrap_or_else(|_| json!([]));
            Json(json_val).into_response()
        }
        Err(_) => Json(json!([])).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockDaemonClient;

    #[tokio::test]
    async fn test_api_conflicts_with_mock() {
        let client: Arc<dyn DaemonClientPort> = Arc::new(MockDaemonClient::new(|cmd| {
            if cmd == "conflicts" {
                Ok(r#"[{"path":"file.txt","reason":"ModifiedBoth"}]"#.to_string())
            } else {
                Err(crate::client::DaemonClientError::CommandFailed(
                    "unknown".into(),
                ))
            }
        }));

        let resp = api_conflicts(State(client)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_status_with_mock() {
        let client: Arc<dyn DaemonClientPort> = Arc::new(MockDaemonClient::new(|cmd| {
            if cmd == "status" {
                Ok(
                    "Watch Dir: /sync\nBackends: [local (ok)]\nSyncing: false\nPaused: false"
                        .to_string(),
                )
            } else {
                Err(crate::client::DaemonClientError::CommandFailed(
                    "unknown".into(),
                ))
            }
        }));

        let resp = api_status(State(client)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_pause_resume_sync_with_mock() {
        let client: Arc<dyn DaemonClientPort> = Arc::new(MockDaemonClient::new(|cmd| match cmd {
            "pause" => Ok("Paused".to_string()),
            "resume" => Ok("Resumed".to_string()),
            "sync" => Ok("Sync initiated".to_string()),
            _ => Err(crate::client::DaemonClientError::CommandFailed(
                "unknown".into(),
            )),
        }));

        let pause_resp = api_pause(State(client.clone())).await.into_response();
        assert_eq!(pause_resp.status(), axum::http::StatusCode::OK);

        let resume_resp = api_resume(State(client.clone())).await.into_response();
        assert_eq!(resume_resp.status(), axum::http::StatusCode::OK);

        let sync_resp = api_sync(State(client)).await.into_response();
        assert_eq!(sync_resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_disconnected_graceful_fallback() {
        let client: Arc<dyn DaemonClientPort> = Arc::new(MockDaemonClient::new(|_| {
            Err(crate::client::DaemonClientError::ConnectionFailed(
                "Connection refused".into(),
            ))
        }));

        let status_resp = api_status(State(client.clone())).await.into_response();
        assert_eq!(status_resp.status(), axum::http::StatusCode::OK);

        let conflicts_resp = api_conflicts(State(client)).await.into_response();
        assert_eq!(conflicts_resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_reload_and_stop() {
        let client: Arc<dyn DaemonClientPort> = Arc::new(MockDaemonClient::new(|cmd| match cmd {
            "reload" => Ok("Configuration reloaded".to_string()),
            "stop" => Ok("Daemon stopping".to_string()),
            _ => Err(crate::client::DaemonClientError::CommandFailed(
                "fail".into(),
            )),
        }));

        let reload_resp = api_reload(State(client.clone())).await.into_response();
        assert_eq!(reload_resp.status(), axum::http::StatusCode::OK);

        let stop_resp = api_stop(State(client)).await.into_response();
        assert_eq!(stop_resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_clear_and_resolve_conflict() {
        let client: Arc<dyn DaemonClientPort> = Arc::new(MockDaemonClient::new(|cmd| {
            if cmd.starts_with("clear ") {
                Ok("Cleared provider".to_string())
            } else if cmd.starts_with("resolve_conflict ") {
                Ok("Resolved successfully".to_string())
            } else {
                Err(crate::client::DaemonClientError::CommandFailed(
                    "fail".into(),
                ))
            }
        }));

        let clear_payload = axum::Json(ClearRequest {
            provider: "dropbox".into(),
        });
        let clear_resp = api_clear(State(client.clone()), clear_payload)
            .await
            .into_response();
        assert_eq!(clear_resp.status(), axum::http::StatusCode::OK);

        let resolve_payload = axum::Json(ResolveConflictRequest {
            rel_path: "conflict.txt".into(),
            resolution: "use_local".into(),
        });
        let resolve_resp = api_resolve_conflict(State(client), resolve_payload)
            .await
            .into_response();
        assert_eq!(resolve_resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_sync_plan_and_logs() {
        let client: Arc<dyn DaemonClientPort> = Arc::new(MockDaemonClient::new(|cmd| {
            if cmd == "get_plan" {
                Ok(r#"[{"action":"upload","path":"test.txt"}]"#.to_string())
            } else if cmd == "get_log" {
                Ok("Status: OK\n\nLog line 1\nLog line 2".to_string())
            } else {
                Err(crate::client::DaemonClientError::CommandFailed(
                    "fail".into(),
                ))
            }
        }));

        let plan_resp = api_sync_plan(State(client.clone())).await.into_response();
        assert_eq!(plan_resp.status(), axum::http::StatusCode::OK);

        let logs_resp = api_logs(State(client)).await.into_response();
        assert_eq!(logs_resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_serve_index() {
        let html = serve_index().await;
        assert!(html.0.contains("CloudSync Control Center"));
    }

    #[test]
    fn test_daemon_client_error_display() {
        let conn_err = crate::client::DaemonClientError::ConnectionFailed("refused".into());
        assert!(format!("{}", conn_err).contains("refused"));

        let io_err: crate::client::DaemonClientError = std::io::Error::other("disk fault").into();
        assert!(format!("{}", io_err).contains("disk fault"));

        let ser_err = crate::client::DaemonClientError::Serialization("bad json".into());
        assert!(format!("{}", ser_err).contains("bad json"));

        let cmd_err = crate::client::DaemonClientError::CommandFailed("bad command".into());
        assert!(format!("{}", cmd_err).contains("bad command"));
    }
}
