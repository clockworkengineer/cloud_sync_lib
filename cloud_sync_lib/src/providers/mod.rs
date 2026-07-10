//! Cloud storage provider implementations.
//!
//! This module houses individual provider clients (Google Drive, Dropbox, OneDrive)
//! and definitions for sharing OAuth client credentials.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SyncMode {
    /// Two-way sync: uploads/downloads changes and propagates deletions.
    TwoWay,
    /// One-way sync: uploads changes and propagates deletions from local to remote.
    #[default]
    OneWay,
    /// One-way sync without propagating deletions.
    OneWayNoDeletions,
}

/// Common configuration settings shared by all storage providers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommonProviderSettings {
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional sync mode: two-way, one-way, one-way-no-deletions
    pub sync_mode: Option<SyncMode>,
    /// Optional password for client-side encryption.
    pub encryption_password: Option<String>,
    /// Optional upload rate limit in KB/s.
    pub max_upload_rate: Option<u64>,
    /// Optional download rate limit in KB/s.
    pub max_download_rate: Option<u64>,
}

pub trait ProviderConfig {
    fn common_settings(&self) -> &CommonProviderSettings;

    fn is_enabled(&self) -> bool {
        self.common_settings().enabled.unwrap_or(true)
    }

    fn sync_mode(&self) -> SyncMode {
        self.common_settings().sync_mode.unwrap_or(SyncMode::OneWay)
    }

    fn sync_deletions(&self) -> bool {
        match self.sync_mode() {
            SyncMode::TwoWay | SyncMode::OneWay => true,
            SyncMode::OneWayNoDeletions => false,
        }
    }

    fn sync_both(&self) -> bool {
        match self.sync_mode() {
            SyncMode::TwoWay => true,
            SyncMode::OneWay | SyncMode::OneWayNoDeletions => false,
        }
    }

    fn destination_folder(&self) -> Option<&str> {
        self.common_settings().destination_folder.as_deref()
    }

    fn encryption_password(&self) -> Option<&str> {
        self.common_settings().encryption_password.as_deref()
    }

    fn max_upload_rate(&self) -> Option<u64> {
        self.common_settings().max_upload_rate
    }

    fn max_download_rate(&self) -> Option<u64> {
        self.common_settings().max_download_rate
    }
}

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
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for OAuthCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
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
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for WebDAVCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
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
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for S3Credentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
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
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for SFTPCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
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
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for NextcloudCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
}

/// Credentials configuration for MEGA cloud storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MegaCredentials {
    /// MEGA Account Email.
    pub email: String,
    /// MEGA Account Password.
    pub password: String,
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for MegaCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
}

/// Credentials configuration for Azure Blob Storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureBlobCredentials {
    /// Azure Storage Account name.
    pub account_name: String,
    /// Azure Storage Account Access Key.
    pub account_key: String,
    /// Target Container name.
    pub container: String,
    /// Custom endpoint URL (optional, used for local Azurite emulator).
    pub endpoint: Option<String>,
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for AzureBlobCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
}

/// Credentials configuration for Google Cloud Storage (GCS).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCSCredentials {
    /// Target Google Cloud Storage bucket name.
    pub bucket: String,
    /// Absolute path to the Service Account JSON credentials key file.
    pub service_account_key_path: String,
    /// Custom endpoint URL (optional, used for local fake-gcs-server emulator).
    pub endpoint: Option<String>,
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for GCSCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
}

/// Credentials configuration for Backblaze B2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct B2Credentials {
    /// Target Backblaze B2 bucket name.
    pub bucket: String,
    /// Backblaze B2 Key ID.
    pub key_id: String,
    /// Backblaze B2 Application Key.
    pub application_key: String,
    /// Custom endpoint URL (optional, used for mocking / alternate endpoints).
    pub endpoint: Option<String>,
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for B2Credentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
}

/// Credentials configuration for pCloud.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PCloudCredentials {
    /// pCloud OAuth2 Access Token.
    pub access_token: String,
    /// Custom API endpoint (optional, e.g. for European accounts or testing).
    pub endpoint: Option<String>,
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for PCloudCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
}

/// Credentials configuration for IPFS Pinning Service (e.g. Pinata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IPFSCredentials {
    /// JWT Bearer Token for authorization.
    pub jwt_token: String,
    /// Custom API endpoint (optional, defaults to Pinata's API https://api.pinata.cloud).
    pub endpoint: Option<String>,
    /// Gateway URL to resolve pinned CIDs (optional, defaults to https://gateway.pinata.cloud/ipfs/).
    pub gateway_url: Option<String>,
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}

impl ProviderConfig for IPFSCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
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
#[cfg(feature = "box")]
pub mod box_provider;
#[cfg(feature = "mega")]
pub mod mega_provider;
#[cfg(feature = "azure_blob")]
pub mod azure_blob;
#[cfg(feature = "gcs")]
pub mod gcs;
#[cfg(feature = "b2")]
pub mod b2;
#[cfg(feature = "pcloud")]
pub mod pcloud;
#[cfg(feature = "ipfs")]
pub mod ipfs;
pub mod local_sim;
pub mod utils;
pub mod fallback;
pub mod encryption;


#[cfg(feature = "google_drive")]
pub use google_drive::{GoogleDriveProvider, GoogleDriveProviderBuilder};
#[cfg(feature = "dropbox")]
pub use dropbox::{DropboxProvider, DropboxProviderBuilder};
#[cfg(feature = "onedrive")]
pub use onedrive::{OneDriveProvider, OneDriveProviderBuilder};
#[cfg(feature = "webdav")]
pub use webdav::{WebDAVProvider, WebDAVProviderBuilder};
#[cfg(feature = "s3")]
pub use s3::{S3Provider, S3ProviderBuilder};
#[cfg(feature = "sftp")]
pub use sftp::{SFTPProvider, SFTPProviderBuilder};
#[cfg(feature = "nextcloud")]
pub use nextcloud::{NextcloudProvider, NextcloudProviderBuilder};
#[cfg(feature = "box")]
pub use box_provider::{BoxProvider, BoxProviderBuilder};
#[cfg(feature = "mega")]
pub use mega_provider::{MegaProvider, MegaProviderBuilder};
#[cfg(feature = "azure_blob")]
pub use azure_blob::{AzureBlobProvider, AzureBlobProviderBuilder};
#[cfg(feature = "gcs")]
pub use gcs::{GCSProvider, GCSProviderBuilder};
#[cfg(feature = "b2")]
pub use b2::{B2Provider, B2ProviderBuilder};
#[cfg(feature = "pcloud")]
pub use pcloud::{PCloudProvider, PCloudProviderBuilder};
#[cfg(feature = "ipfs")]
pub use ipfs::{IPFSProvider, IPFSProviderBuilder};
pub use fallback::SimulatedFallback;
pub use encryption::EncryptedBackend;

