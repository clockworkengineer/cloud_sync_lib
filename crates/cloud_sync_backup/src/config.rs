use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use cloud_sync_lib::{OAuthCredentials, WebDAVCredentials, S3Credentials, SFTPCredentials, NextcloudCredentials, MegaCredentials, AzureBlobCredentials, GCSCredentials, B2Credentials, PCloudCredentials, IPFSCredentials};

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
    pub watch_directory: Option<PathBuf>,
    pub google_drive_root: Option<PathBuf>,
    pub dropbox_root: Option<PathBuf>,
    pub onedrive_root: Option<PathBuf>,
    pub webdav_root: Option<PathBuf>,
    pub s3_root: Option<PathBuf>,
    pub sftp_root: Option<PathBuf>,
    pub nextcloud_root: Option<PathBuf>,
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
    pub max_concurrency: Option<usize>,
}

pub async fn load_config(path: &str) -> Result<BackupConfig, Box<dyn std::error::Error>> {
    let config_path = Path::new(path);
    let content = tokio::fs::read_to_string(config_path).await?;
    let config: BackupConfig = toml::from_str(&content)?;
    Ok(config)
}
