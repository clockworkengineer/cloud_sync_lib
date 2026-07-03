//! Configuration handling and parsing module.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;
use cloud_sync_lib::{OAuthCredentials, WebDAVCredentials, S3Credentials, SFTPCredentials, NextcloudCredentials, MegaCredentials, AzureBlobCredentials, GCSCredentials};

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
///
/// # Arguments
/// * `credentials` - OAuth credentials configuration options.
///
/// # Returns
/// True if the provider is enabled or no enabled flag is explicitly set to false, false otherwise.
pub fn is_enabled(credentials: &Option<OAuthCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to check if WebDAV provider is enabled.
///
/// # Arguments
/// * `credentials` - WebDAV credentials configuration options.
///
/// # Returns
/// True if the provider is enabled, false otherwise.
pub fn is_webdav_enabled(credentials: &Option<WebDAVCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to check if S3 provider is enabled.
///
/// # Arguments
/// * `credentials` - S3 credentials configuration options.
///
/// # Returns
/// True if the provider is enabled, false otherwise.
pub fn is_s3_enabled(credentials: &Option<S3Credentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to check if SFTP provider is enabled.
///
/// # Arguments
/// * `credentials` - SFTP credentials configuration options.
///
/// # Returns
/// True if the provider is enabled, false otherwise.
pub fn is_sftp_enabled(credentials: &Option<SFTPCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to check if Nextcloud provider is enabled.
///
/// # Arguments
/// * `credentials` - Nextcloud credentials configuration options.
///
/// # Returns
/// True if the provider is enabled, false otherwise.
pub fn is_nextcloud_enabled(credentials: &Option<NextcloudCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to check if MEGA provider is enabled.
///
/// # Arguments
/// * `credentials` - MEGA credentials configuration options.
///
/// # Returns
/// True if the provider is enabled, false otherwise.
pub fn is_mega_enabled(credentials: &Option<MegaCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to check if Azure Blob provider is enabled.
///
/// # Arguments
/// * `credentials` - Azure Blob credentials configuration options.
///
/// # Returns
/// True if the provider is enabled, false otherwise.
pub fn is_azure_blob_enabled(credentials: &Option<AzureBlobCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

/// Helper function to check if GCS provider is enabled.
///
/// # Arguments
/// * `credentials` - GCS credentials configuration options.
///
/// # Returns
/// True if the provider is enabled, false otherwise.
pub fn is_gcs_enabled(credentials: &Option<GCSCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that `is_enabled` helper correctly returns true/false based on OAuth credentials status.
    #[test]
    fn test_is_enabled() {
        let creds_none: Option<OAuthCredentials> = None;
        assert!(is_enabled(&creds_none));

        let creds_disabled = Some(OAuthCredentials {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
            refresh_token: "token".to_string(),
            destination_folder: None,
            enabled: Some(false),
            sync: None,
        });
        assert!(!is_enabled(&creds_disabled));

        let creds_enabled = Some(OAuthCredentials {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
            refresh_token: "token".to_string(),
            destination_folder: None,
            enabled: Some(true),
            sync: None,
        });
        assert!(is_enabled(&creds_enabled));
    }
}
