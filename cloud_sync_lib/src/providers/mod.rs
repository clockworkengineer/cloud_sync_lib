//! Cloud storage provider implementations.
//!
//! This module houses individual provider clients (Google Drive, Dropbox, OneDrive)
//! and definitions for sharing OAuth client credentials.

use serde::{Deserialize, Serialize};

/// Credentials configuration for OAuth2 authorization flows.
///
/// Contains client secrets and long-lived refresh tokens used to retrieve
/// short-lived access tokens dynamically during API execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    /// OAuth2 Client ID.
    pub client_id: String,
    /// OAuth2 Client Secret.
    pub client_secret: String,
    /// Long-lived Refresh Token.
    pub refresh_token: String,
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional toggle to enable/disable deletion syncing.
    pub sync: Option<bool>,
}

/// Credentials and URL configuration for WebDAV servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDAVCredentials {
    /// WebDAV Server Base URL.
    pub url: String,
    /// WebDAV Username.
    pub username: String,
    /// WebDAV Password.
    pub password: String,
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional toggle to enable/disable deletion syncing.
    pub sync: Option<bool>,
}

/// Credentials configuration for Amazon S3 and S3-Compatible backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Credentials {
    /// S3 Bucket name.
    pub bucket: String,
    /// S3 Region name.
    pub region: String,
    /// S3 Access Key ID.
    pub access_key_id: String,
    /// S3 Secret Access Key.
    pub secret_access_key: String,
    /// Custom endpoint URL (optional, required for S3-compatible providers).
    pub endpoint: Option<String>,
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional toggle to enable/disable deletion syncing.
    pub sync: Option<bool>,
}

/// Credentials configuration for SFTP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SFTPCredentials {
    /// SFTP Host address.
    pub host: String,
    /// SFTP Port (defaults to 22 if None).
    pub port: Option<u16>,
    /// SFTP Username.
    pub username: String,
    /// SFTP Password (optional if using key-based auth).
    pub password: Option<String>,
    /// Path to the SSH private key (optional).
    pub private_key_path: Option<String>,
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional toggle to enable/disable deletion syncing.
    pub sync: Option<bool>,
}

/// Credentials configuration for Nextcloud WebDAV and OCS services.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextcloudCredentials {
    /// Nextcloud Server URL (e.g. https://nextcloud.example.com)
    pub url: String,
    /// Nextcloud Username.
    pub username: String,
    /// Nextcloud App Password.
    pub app_password: String,
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional toggle to enable/disable deletion syncing.
    pub sync: Option<bool>,
}

#[cfg(feature = "google_drive")]
pub mod google_drive;
#[cfg(feature = "dropbox")]
pub mod dropbox;
#[cfg(feature = "onedrive")]
pub mod onedrive;
#[cfg(feature = "webdav")]
pub mod webdav;
#[cfg(feature = "s3")]
pub mod s3;
#[cfg(feature = "sftp")]
pub mod sftp;
#[cfg(feature = "nextcloud")]
pub mod nextcloud;
pub mod local_sim;
pub mod utils;
pub mod fallback;

#[cfg(feature = "google_drive")]
pub use google_drive::GoogleDriveProvider;
#[cfg(feature = "dropbox")]
pub use dropbox::DropboxProvider;
#[cfg(feature = "onedrive")]
pub use onedrive::OneDriveProvider;
#[cfg(feature = "webdav")]
pub use webdav::WebDAVProvider;
#[cfg(feature = "s3")]
pub use s3::S3Provider;
#[cfg(feature = "sftp")]
pub use sftp::SFTPProvider;
#[cfg(feature = "nextcloud")]
pub use nextcloud::NextcloudProvider;
pub use fallback::SimulatedFallback;
