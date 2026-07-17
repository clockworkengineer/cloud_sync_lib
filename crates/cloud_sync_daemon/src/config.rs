//! Configuration handling and parsing module.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;
use cloud_sync_lib::{OAuthCredentials, WebDAVCredentials, S3Credentials, SFTPCredentials, NextcloudCredentials, MegaCredentials, AzureBlobCredentials, GCSCredentials, B2Credentials, PCloudCredentials, IPFSCredentials, ProviderConfig};

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
    pub google_credentials: Option<OAuthCredentials>,
    pub dropbox_credentials: Option<OAuthCredentials>,
    pub onedrive_credentials: Option<OAuthCredentials>,
    pub webdav_credentials: Option<WebDAVCredentials>,
    pub s3_credentials: Option<S3Credentials>,
    pub sftp_credentials: Option<SFTPCredentials>,
    pub nextcloud_credentials: Option<NextcloudCredentials>,
    pub box_credentials: Option<OAuthCredentials>,
    pub mega_credentials: Option<MegaCredentials>,
    pub azure_blob_credentials: Option<AzureBlobCredentials>,
    pub gcs_credentials: Option<GCSCredentials>,
    pub b2_credentials: Option<B2Credentials>,
    pub pcloud_credentials: Option<PCloudCredentials>,
    pub ipfs_credentials: Option<IPFSCredentials>,
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
        Self {
            watch_directory: PathBuf::from(DEFAULT_WATCH_DIR),
            google_drive_root: PathBuf::from(DEFAULT_GOOGLE_DRIVE_ROOT),
            dropbox_root: PathBuf::from(DEFAULT_DROPBOX_ROOT),
            onedrive_root: PathBuf::from(DEFAULT_ONEDRIVE_ROOT),
            webdav_root: PathBuf::from(DEFAULT_WEBDAV_ROOT),
            s3_root: PathBuf::from(DEFAULT_S3_ROOT),
            sftp_root: PathBuf::from(DEFAULT_SFTP_ROOT),
            nextcloud_root: PathBuf::from(DEFAULT_NEXTCLOUD_ROOT),
            box_root: Some(PathBuf::from(DEFAULT_BOX_ROOT)),
            mega_root: Some(PathBuf::from(DEFAULT_MEGA_ROOT)),
            azure_blob_root: Some(PathBuf::from(DEFAULT_AZURE_BLOB_ROOT)),
            gcs_root: Some(PathBuf::from(DEFAULT_GCS_ROOT)),
            b2_root: Some(PathBuf::from(DEFAULT_B2_ROOT)),
            pcloud_root: Some(PathBuf::from(DEFAULT_PCLOUD_ROOT)),
            ipfs_root: Some(PathBuf::from(DEFAULT_IPFS_ROOT)),
            google_credentials: None,
            dropbox_credentials: None,
            onedrive_credentials: None,
            webdav_credentials: None,
            s3_credentials: None,
            sftp_credentials: None,
            nextcloud_credentials: None,
            box_credentials: None,
            mega_credentials: None,
            azure_blob_credentials: None,
            gcs_credentials: None,
            b2_credentials: None,
            pcloud_credentials: None,
            ipfs_credentials: None,
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

/// Load configuration from a TOML file. If the file doesn't exist, a default config is created and saved.
///
/// # Arguments
/// * `path` - The path to the config file.
///
/// # Returns
/// The loaded config or an error if file I/O or parsing fails.
pub async fn load_or_create_config(path: &str) -> Result<AppConfig, Box<dyn std::error::Error>> {
    let config_path = Path::new(path);
    if config_path.exists() {
        let content = tokio::fs::read_to_string(config_path).await?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    } else {
        let config = AppConfig::default();
        let content = toml::to_string_pretty(&config)?;
        tokio::fs::write(config_path, content).await?;
        info!("Created default configuration file at {:?}", config_path);
        Ok(config)
    }
}

/// Helper function to check if a provider is enabled based on its credentials config.
pub fn is_provider_enabled<C: ProviderConfig>(credentials: &Option<C>) -> bool {
    credentials.as_ref().is_none_or(|c| c.is_enabled())
}

#[cfg(test)]
mod tests {
    use super::*;

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
