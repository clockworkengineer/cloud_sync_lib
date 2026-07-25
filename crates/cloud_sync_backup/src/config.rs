use serde::{Deserialize, Serialize};
use std::path::Path;
use cloud_sync_lib::{ProviderRootsConfig, ProviderCredentialsConfig};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupSection {
    pub source_provider: String,
    pub source_path: Option<String>,
    pub destination_provider: String,
    pub destination_path: Option<String>,
    pub backup_interval_secs: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupConfig {
    pub backup: BackupSection,
    #[serde(flatten)]
    pub roots: ProviderRootsConfig,
    #[serde(flatten)]
    pub credentials: ProviderCredentialsConfig,
    pub exclude: Option<Vec<String>>,
    pub max_upload_rate: Option<u64>,
    pub max_download_rate: Option<u64>,
    pub max_concurrency: Option<usize>,
}

impl std::ops::Deref for BackupConfig {
    type Target = ProviderCredentialsConfig;
    fn deref(&self) -> &Self::Target {
        &self.credentials
    }
}

pub fn get_default_backup_config_path() -> std::path::PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = std::path::PathBuf::from(appdata).join("CloudSync");
        let _ = std::fs::create_dir_all(&p);
        p.join("backup_config.toml")
    } else if let Ok(home) = std::env::var("HOME") {
        let p = std::path::PathBuf::from(home).join(".config").join("cloud_sync");
        let _ = std::fs::create_dir_all(&p);
        p.join("backup_config.toml")
    } else {
        std::path::PathBuf::from("backup_config.toml")
    }
}

fn expand_path(p: std::path::PathBuf) -> std::path::PathBuf {
    let p_str = p.to_string_lossy();
    if p_str.starts_with("~/") || p_str == "~" {
        let home_dir = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        if p_str == "~" {
            home_dir
        } else {
            home_dir.join(&p_str[2..])
        }
    } else {
        p
    }
}

pub async fn load_config(path: &str) -> Result<BackupConfig, Box<dyn std::error::Error>> {
    let config_path = Path::new(path);
    let content = tokio::fs::read_to_string(config_path).await?;
    let mut config: BackupConfig = toml::from_str(&content)?;

    // Expand tildes in roots
    config.roots.google_drive_root = config.roots.google_drive_root.map(expand_path);
    config.roots.dropbox_root = config.roots.dropbox_root.map(expand_path);
    config.roots.onedrive_root = config.roots.onedrive_root.map(expand_path);
    config.roots.webdav_root = config.roots.webdav_root.map(expand_path);
    config.roots.s3_root = config.roots.s3_root.map(expand_path);
    config.roots.sftp_root = config.roots.sftp_root.map(expand_path);
    config.roots.nextcloud_root = config.roots.nextcloud_root.map(expand_path);

    Ok(config)
}
