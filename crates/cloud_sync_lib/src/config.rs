//! Shared provider configuration structs and models.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::{
    OAuthCredentials, WebDAVCredentials, S3Credentials, SFTPCredentials,
    NextcloudCredentials, MegaCredentials, AzureBlobCredentials, GCSCredentials,
    B2Credentials, PCloudCredentials, IPFSCredentials,
};

/// Shared configuration holding root directories for all supported storage backends.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ProviderRootsConfig {
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
}

/// Shared configuration holding credentials for all supported storage backends.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ProviderCredentialsConfig {
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
}
