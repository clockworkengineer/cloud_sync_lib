use axum::{
    routing::{get, post},
    response::{Html, IntoResponse},
    Json, Router,
};
use tower_http::cors::CorsLayer;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::net::TcpStream;
use serde_json::json;

#[tokio::main]
async fn main() {
    let router = Router::new()
        .route("/", get(serve_index))
        .route("/api/status", get(api_status))
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

async fn serve_index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

async fn send_daemon_cmd(cmd: &str) -> Result<String, std::io::Error> {
    let mut stream = TcpStream::connect("127.0.0.1:8081").await?;
    stream.write_all(format!("{}\n", cmd).as_bytes()).await?;
    stream.flush().await?;
    
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}

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

async fn api_status() -> impl IntoResponse {
    match send_daemon_cmd("status").await {
        Ok(raw) => Json(parse_status(&raw)).into_response(),
        Err(e) => {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Could not connect to daemon socket: {}", e) }))
            ).into_response()
        }
    }
}

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
