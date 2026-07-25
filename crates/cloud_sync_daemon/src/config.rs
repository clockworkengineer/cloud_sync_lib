//! Configuration handling and parsing module.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;
use cloud_sync_lib::ProviderConfig;

pub const DEFAULT_CONFIG_FILE: &str = "config.toml";
pub const DEFAULT_WATCH_DIR: &str = "./watched_folder";
pub const DEFAULT_GOOGLE_DRIVE_ROOT: &str = "./cloud_simulation/google_drive";
pub const DEFAULT_DROPBOX_ROOT: &str = "./cloud_simulation/dropbox";
pub const DEFAULT_ONEDRIVE_ROOT: &str = "./cloud_simulation/onedrive";
pub const DEFAULT_WEBDAV_ROOT: &str = "./cloud_simulation/webdav";
pub const DEFAULT_S3_ROOT: &str = "./cloud_simulation/s3";
pub const DEFAULT_SFTP_ROOT: &str = "./cloud_simulation/sftp";
pub const DEFAULT_NEXTCLOUD_ROOT: &str = "./cloud_simulation/nextcloud";
pub const DEFAULT_BOX_ROOT: &str = "./cloud_simulation/box";
pub const DEFAULT_MEGA_ROOT: &str = "./cloud_simulation/mega";
pub const DEFAULT_AZURE_BLOB_ROOT: &str = "./cloud_simulation/azure_blob";
pub const DEFAULT_GCS_ROOT: &str = "./cloud_simulation/gcs";
pub const DEFAULT_B2_ROOT: &str = "./cloud_simulation/b2";
pub const DEFAULT_PCLOUD_ROOT: &str = "./cloud_simulation/pcloud";
pub const DEFAULT_IPFS_ROOT: &str = "./cloud_simulation/ipfs";

/// Global configuration parsed from the configuration TOML file.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub watch_directory: PathBuf,
    pub google_drive_root: PathBuf,
    pub dropbox_root: PathBuf,
    pub onedrive_root: PathBuf,
    pub webdav_root: PathBuf,
    pub s3_root: PathBuf,
    pub sftp_root: PathBuf,
    pub nextcloud_root: PathBuf,
    pub box_root: Option<PathBuf>,
    pub mega_root: Option<PathBuf>,
    pub azure_blob_root: Option<PathBuf>,
    pub gcs_root: Option<PathBuf>,
    pub b2_root: Option<PathBuf>,
    pub pcloud_root: Option<PathBuf>,
    pub ipfs_root: Option<PathBuf>,
    #[serde(flatten)]
    pub credentials: cloud_sync_lib::ProviderCredentialsConfig,
    pub exclude: Option<Vec<String>>,
    pub max_upload_rate: Option<u64>,
    pub max_download_rate: Option<u64>,
    pub pull_interval_secs: Option<u64>,
    pub max_concurrency: Option<usize>,
    pub pmu_hook: Option<String>,
    pub conflict_policy: Option<cloud_sync_lib::ConflictPolicy>,
    pub dry_run: Option<bool>,
    pub bandwidth_schedule: Option<Vec<BandwidthSchedule>>,
    pub error_recovery: Option<ErrorRecoveryConfig>,
}

impl std::ops::Deref for AppConfig {
    type Target = cloud_sync_lib::ProviderCredentialsConfig;
    fn deref(&self) -> &Self::Target {
        &self.credentials
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BandwidthSchedule {
    pub start_time: String,
    pub end_time: String,
    pub max_upload_rate: Option<u64>,
    pub max_download_rate: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ErrorRecoveryConfig {
    pub max_retries: Option<usize>,
    pub initial_delay_ms: Option<u64>,
    pub multiplier: Option<f64>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let home_dir = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());

        let watch_dir = home_dir.join("CloudSync");
        let sim_dir = home_dir.join(".config").join("cloud_sync").join("simulation");

        Self {
            watch_directory: watch_dir,
            google_drive_root: sim_dir.join("google_drive"),
            dropbox_root: sim_dir.join("dropbox"),
            onedrive_root: sim_dir.join("onedrive"),
            webdav_root: sim_dir.join("webdav"),
            s3_root: sim_dir.join("s3"),
            sftp_root: sim_dir.join("sftp"),
            nextcloud_root: sim_dir.join("nextcloud"),
            box_root: Some(sim_dir.join("box")),
            mega_root: Some(sim_dir.join("mega")),
            azure_blob_root: Some(sim_dir.join("azure_blob")),
            gcs_root: Some(sim_dir.join("gcs")),
            b2_root: Some(sim_dir.join("b2")),
            pcloud_root: Some(sim_dir.join("pcloud")),
            ipfs_root: Some(sim_dir.join("ipfs")),
            credentials: Default::default(),
            exclude: None,
            max_upload_rate: None,
            max_download_rate: None,
            pull_interval_secs: Some(30),
            max_concurrency: Some(4),
            pmu_hook: None,
            conflict_policy: Some(cloud_sync_lib::ConflictPolicy::RenameLocal),
            dry_run: Some(false),
            bandwidth_schedule: None,
            error_recovery: None,
        }
    }
}

/// Resolves the default configuration file location using standard user profile directories.
pub fn get_default_config_path() -> std::path::PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = std::path::PathBuf::from(appdata).join("CloudSync");
        let _ = std::fs::create_dir_all(&p);
        p.join("config.toml")
    } else if let Ok(home) = std::env::var("HOME") {
        let p = std::path::PathBuf::from(home).join(".config").join("cloud_sync");
        let _ = std::fs::create_dir_all(&p);
        p.join("config.toml")
    } else {
        std::path::PathBuf::from(DEFAULT_CONFIG_FILE)
    }
}

fn expand_path(p: PathBuf) -> PathBuf {
    let p_str = p.to_string_lossy();
    if p_str.starts_with("~/") || p_str == "~" {
        let home_dir = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(PathBuf::from)
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

/// Load configuration from a TOML file. If the file doesn't exist, a default config is created and saved.
///
/// # Arguments
/// * `path` - The path to the config file.
///
/// # Returns
/// The loaded config or an error if file I/O or parsing fails.
pub async fn load_or_create_config(path: &str) -> Result<AppConfig, Box<dyn std::error::Error>> {
    let config_path = Path::new(path);
    let mut config = if config_path.exists() {
        let content = tokio::fs::read_to_string(config_path).await?;
        let parsed: AppConfig = toml::from_str(&content)?;
        parsed
    } else {
        let default_config = AppConfig::default();
        let content = toml::to_string_pretty(&default_config)?;
        tokio::fs::write(config_path, content).await?;
        info!("Created default configuration file at {:?}", config_path);
        default_config
    };

    // Post-process to expand home directories
    config.watch_directory = expand_path(config.watch_directory);
    config.google_drive_root = expand_path(config.google_drive_root);
    config.dropbox_root = expand_path(config.dropbox_root);
    config.onedrive_root = expand_path(config.onedrive_root);
    config.webdav_root = expand_path(config.webdav_root);
    config.s3_root = expand_path(config.s3_root);
    config.sftp_root = expand_path(config.sftp_root);
    config.nextcloud_root = expand_path(config.nextcloud_root);
    config.box_root = config.box_root.map(expand_path);
    config.mega_root = config.mega_root.map(expand_path);
    config.azure_blob_root = config.azure_blob_root.map(expand_path);
    config.gcs_root = config.gcs_root.map(expand_path);
    config.b2_root = config.b2_root.map(expand_path);
    config.pcloud_root = config.pcloud_root.map(expand_path);
    config.ipfs_root = config.ipfs_root.map(expand_path);

    Ok(config)
}

/// Helper function to check if a provider is enabled based on its credentials config.
pub fn is_provider_enabled<C: ProviderConfig>(credentials: &Option<C>) -> bool {
    credentials.as_ref().is_none_or(|c| c.is_enabled())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_sync_lib::OAuthCredentials;

    /// Tests that `is_provider_enabled` helper correctly returns true/false based on OAuth credentials status.
    #[test]
    fn test_is_enabled() {
        let creds_none: Option<OAuthCredentials> = None;
        assert!(is_provider_enabled(&creds_none));

        let creds_disabled = Some(OAuthCredentials {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
            refresh_token: "token".to_string(),
            common: cloud_sync_lib::CommonProviderSettings {
                destination_folder: None,
                enabled: Some(false),
                sync_mode: None,
                encryption_password: None,
                max_upload_rate: None,
                max_download_rate: None,
                selective_sync: None,
            },
        });
        assert!(!is_provider_enabled(&creds_disabled));

        let creds_enabled = Some(OAuthCredentials {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
            refresh_token: "token".to_string(),
            common: cloud_sync_lib::CommonProviderSettings {
                destination_folder: None,
                enabled: Some(true),
                sync_mode: None,
                encryption_password: None,
                max_upload_rate: None,
                max_download_rate: None,
                selective_sync: None,
            },
        });
        assert!(is_provider_enabled(&creds_enabled));
    }

    #[test]
    fn test_bandwidth_schedule_parsing() {
        let toml_str = r#"
            watch_directory = "./watched_folder"
            google_drive_root = "./cloud_simulation/google_drive"
            dropbox_root = "./cloud_simulation/dropbox"
            onedrive_root = "./cloud_simulation/onedrive"
            webdav_root = "./cloud_simulation/webdav"
            s3_root = "./cloud_simulation/s3"
            sftp_root = "./cloud_simulation/sftp"
            nextcloud_root = "./cloud_simulation/nextcloud"

            [[bandwidth_schedule]]
            start_time = "09:00"
            end_time = "17:00"
            max_upload_rate = 100
            max_download_rate = 200
        "#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        let schedules = config.bandwidth_schedule.unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].start_time, "09:00");
        assert_eq!(schedules[0].end_time, "17:00");
        assert_eq!(schedules[0].max_upload_rate, Some(100));
        assert_eq!(schedules[0].max_download_rate, Some(200));
    }

    #[test]
    fn test_error_recovery_parsing() {
        let toml_str = r#"
            watch_directory = "./watched_folder"
            google_drive_root = "./cloud_simulation/google_drive"
            dropbox_root = "./cloud_simulation/dropbox"
            onedrive_root = "./cloud_simulation/onedrive"
            webdav_root = "./cloud_simulation/webdav"
            s3_root = "./cloud_simulation/s3"
            sftp_root = "./cloud_simulation/sftp"
            nextcloud_root = "./cloud_simulation/nextcloud"

            [error_recovery]
            max_retries = 3
            initial_delay_ms = 1000
            multiplier = 1.5
        "#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        let recovery = config.error_recovery.unwrap();
        assert_eq!(recovery.max_retries, Some(3));
        assert_eq!(recovery.initial_delay_ms, Some(1000));
        assert_eq!(recovery.multiplier, Some(1.5));
    }
}
