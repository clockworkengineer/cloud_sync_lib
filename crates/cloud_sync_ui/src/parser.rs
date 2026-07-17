//! Daemon TCP status response parser.

use serde_json::json;

/// Parses the daemon's raw status response into a structured JSON value.
///
/// # Arguments
/// * `raw` - Raw multi-line string output from the daemon's status command.
///
/// # Returns
/// A `serde_json::Value` object containing status details.
pub fn parse_status(raw: &str) -> serde_json::Value {
    let mut paused = false;
    let mut watch_directory = String::new();
    let mut config_file = String::new();
    let mut active_backends = Vec::new();
    let mut failed_backends = Vec::new();
    let mut syncing = false;
    let mut web_ui_address = String::new();

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
        } else if line.starts_with("Failed Backends: ") {
            let list_str = line.trim_start_matches("Failed Backends: ").trim();
            if list_str.starts_with('[') && list_str.ends_with(']') {
                let inner = &list_str[1..list_str.len() - 1];
                for item in inner.split(',') {
                    let item_clean = item.trim();
                    let item_clean = item_clean.strip_prefix('"').unwrap_or(item_clean);
                    let item_clean = item_clean.strip_suffix('"').unwrap_or(item_clean);
                    if !item_clean.is_empty() {
                        failed_backends.push(item_clean.to_string());
                    }
                }
            }
        } else if line.starts_with("Syncing: ") {
            syncing = line.trim_start_matches("Syncing: ").trim() == "true";
        } else if line.starts_with("Web UI Address: ") {
            let addr = line.trim_start_matches("Web UI Address: ").trim();
            let addr = addr.strip_prefix('"').unwrap_or(addr);
            let addr = addr.strip_suffix('"').unwrap_or(addr);
            web_ui_address = addr.to_string();
        }
    }

    json!({
        "paused": paused,
        "watch_directory": watch_directory,
        "config_file": config_file,
        "active_backends": active_backends,
        "failed_backends": failed_backends,
        "syncing": syncing,
        "web_ui_address": web_ui_address,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that the daemon TCP response parser maps raw lines correctly to a JSON status payload.
    #[test]
    fn test_parse_status() {
        let raw_status = "Status: OK\nPaused: false\nWatch Directory: \"/watch/dir\"\nConfig File: \"private.toml\"\nActive Backends: [\"Google Drive\",\"Dropbox\"]\nFailed Backends: [\"Google Drive\"]\nSyncing: false\nWeb UI Address: \"127.0.0.1:8082\"\n";
        let parsed = parse_status(raw_status);
        assert_eq!(parsed["paused"], false);
        assert_eq!(parsed["watch_directory"], "/watch/dir");
        assert_eq!(parsed["config_file"], "private.toml");
        assert_eq!(parsed["active_backends"], serde_json::json!(["Google Drive", "Dropbox"]));
        assert_eq!(parsed["failed_backends"], serde_json::json!(["Google Drive"]));
        assert_eq!(parsed["syncing"], false);
        assert_eq!(parsed["web_ui_address"], "127.0.0.1:8082");
    }
}
