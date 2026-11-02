use cloud_sync_lib::{ProviderCredentialsConfig, ProviderRootsConfig};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
        let p = std::path::PathBuf::from(home)
            .join(".config")
            .join("cloud_sync");
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_expand_path() {
        let plain = std::path::PathBuf::from("/var/data");
        assert_eq!(expand_path(plain.clone()), plain);

        let tilde_exact = std::path::PathBuf::from("~");
        let expanded_exact = expand_path(tilde_exact);
        assert!(!expanded_exact.to_string_lossy().starts_with('~'));

        let tilde_sub = std::path::PathBuf::from("~/my_backup");
        let expanded_sub = expand_path(tilde_sub);
        assert!(expanded_sub.to_string_lossy().ends_with("my_backup"));
    }

    #[test]
    fn test_get_default_backup_config_path() {
        let p = get_default_backup_config_path();
        assert!(p.to_string_lossy().ends_with("backup_config.toml"));
    }

    #[tokio::test]
    async fn test_load_config_valid() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("backup.toml");

        let toml_content = r#"
max_upload_rate = 1000000
max_download_rate = 2000000
max_concurrency = 8
exclude = ["*.tmp", ".git/*"]

[backup]
source_provider = "local"
source_path = "source_dir"
destination_provider = "google_drive"
destination_path = "dest_dir"
backup_interval_secs = 60
"#;

        tokio::fs::write(&config_path, toml_content).await.unwrap();

        let cfg = load_config(config_path.to_str().unwrap())
            .await
            .expect("config should load");
        assert_eq!(cfg.backup.source_provider, "local");
        assert_eq!(cfg.backup.destination_provider, "google_drive");
        assert_eq!(cfg.backup.backup_interval_secs, Some(60));
        assert_eq!(cfg.max_upload_rate, Some(1000000));
        assert_eq!(cfg.max_concurrency, Some(8));
        assert_eq!(cfg.exclude.as_ref().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_load_config_missing_file() {
        let res = load_config("non_existent_path_xyz_123.toml").await;
        assert!(res.is_err());
    }
}
