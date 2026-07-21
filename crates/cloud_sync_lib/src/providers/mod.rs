//! Cloud storage provider implementations.
//!
//! This module houses individual provider clients (Google Drive, Dropbox, OneDrive)
//! and definitions for sharing OAuth client credentials.

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

pub use cloud_sync_core::SyncMode;

/// Common configuration settings shared by all storage providers.
#[derive(Debug, Clone, Serialize, Deserialize, Default, Zeroize, ZeroizeOnDrop)]
pub struct CommonProviderSettings {
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional sync mode: two-way, one-way, one-way-no-deletions
    #[zeroize(skip)]
    pub sync_mode: Option<SyncMode>,
    /// Optional password for client-side encryption.
    pub encryption_password: Option<String>,
    /// Optional upload rate limit in KB/s.
    pub max_upload_rate: Option<u64>,
    /// Optional download rate limit in KB/s.
    pub max_download_rate: Option<u64>,
    /// Optional selective synchronization folder paths.
    #[zeroize(skip)]
    pub selective_sync: Option<Vec<String>>,
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

    fn selective_sync(&self) -> Option<Vec<String>> {
        self.common_settings().selective_sync.clone()
    }
}

/// Credentials configuration for OAuth2 authorization flows.
///
/// Contains client secrets and long-lived refresh tokens used to retrieve
/// short-lived access tokens dynamically during API execution.
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
pub use utils::OAuthTokenManager;

use std::sync::Arc;
use crate::traits::StorageBackend;

/// Enum wrapping credentials for any compiled backend.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, zeroize::Zeroize)]
#[serde(tag = "type", content = "config")]
pub enum BackendCredentials {
    #[cfg(feature = "google_drive")]
    GoogleDrive(OAuthCredentials),
    #[cfg(feature = "dropbox")]
    Dropbox(OAuthCredentials),
    #[cfg(feature = "onedrive")]
    OneDrive(OAuthCredentials),
    #[cfg(feature = "webdav")]
    WebDAV(WebDAVCredentials),
    #[cfg(feature = "s3")]
    S3(S3Credentials),
    #[cfg(feature = "sftp")]
    SFTP(SFTPCredentials),
    #[cfg(feature = "nextcloud")]
    Nextcloud(NextcloudCredentials),
    #[cfg(feature = "box")]
    Box(OAuthCredentials),
    #[cfg(feature = "mega")]
    Mega(MegaCredentials),
    #[cfg(feature = "azure_blob")]
    AzureBlob(AzureBlobCredentials),
    #[cfg(feature = "gcs")]
    GCS(GCSCredentials),
    #[cfg(feature = "b2")]
    B2(B2Credentials),
    #[cfg(feature = "pcloud")]
    PCloud(PCloudCredentials),
    #[cfg(feature = "ipfs")]
    IPFS(IPFSCredentials),
}

#[allow(unreachable_patterns)]
impl ProviderConfig for BackendCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        match self {
            #[cfg(feature = "google_drive")]
            BackendCredentials::GoogleDrive(c) => c.common_settings(),
            #[cfg(feature = "dropbox")]
            BackendCredentials::Dropbox(c) => c.common_settings(),
            #[cfg(feature = "onedrive")]
            BackendCredentials::OneDrive(c) => c.common_settings(),
            #[cfg(feature = "webdav")]
            BackendCredentials::WebDAV(c) => c.common_settings(),
            #[cfg(feature = "s3")]
            BackendCredentials::S3(c) => c.common_settings(),
            #[cfg(feature = "sftp")]
            BackendCredentials::SFTP(c) => c.common_settings(),
            #[cfg(feature = "nextcloud")]
            BackendCredentials::Nextcloud(c) => c.common_settings(),
            #[cfg(feature = "box")]
            BackendCredentials::Box(c) => c.common_settings(),
            #[cfg(feature = "mega")]
            BackendCredentials::Mega(c) => c.common_settings(),
            #[cfg(feature = "azure_blob")]
            BackendCredentials::AzureBlob(c) => c.common_settings(),
            #[cfg(feature = "gcs")]
            BackendCredentials::GCS(c) => c.common_settings(),
            #[cfg(feature = "b2")]
            BackendCredentials::B2(c) => c.common_settings(),
            #[cfg(feature = "pcloud")]
            BackendCredentials::PCloud(c) => c.common_settings(),
            #[cfg(feature = "ipfs")]
            BackendCredentials::IPFS(c) => c.common_settings(),
            _ => unreachable!(),
        }
    }
}

impl BackendCredentials {
    pub fn sync_mode(&self) -> SyncMode {
        ProviderConfig::sync_mode(self)
    }

    pub fn selective_sync(&self) -> Option<Vec<String>> {
        ProviderConfig::selective_sync(self)
    }
}

/// Unified factory registry to build storage providers dynamically.
pub struct BackendRegistry;

impl BackendRegistry {
    /// Dynamically instantiates a provider using its config credentials.
    pub fn build(creds: BackendCredentials) -> Arc<dyn StorageBackend> {
        match creds {
            #[cfg(feature = "google_drive")]
            BackendCredentials::GoogleDrive(c) => Arc::new(GoogleDriveProvider::new(c)),
            #[cfg(feature = "dropbox")]
            BackendCredentials::Dropbox(c) => Arc::new(DropboxProvider::new(c)),
            #[cfg(feature = "onedrive")]
            BackendCredentials::OneDrive(c) => Arc::new(OneDriveProvider::new(c)),
            #[cfg(feature = "webdav")]
            BackendCredentials::WebDAV(c) => Arc::new(WebDAVProvider::new(c)),
            #[cfg(feature = "s3")]
            BackendCredentials::S3(c) => Arc::new(S3Provider::new(c)),
            #[cfg(feature = "sftp")]
            BackendCredentials::SFTP(c) => Arc::new(SFTPProvider::new(c)),
            #[cfg(feature = "nextcloud")]
            BackendCredentials::Nextcloud(c) => Arc::new(NextcloudProvider::new(c)),
            #[cfg(feature = "box")]
            BackendCredentials::Box(c) => Arc::new(BoxProvider::new(c)),
            #[cfg(feature = "mega")]
            BackendCredentials::Mega(c) => Arc::new(MegaProvider::new(c)),
            #[cfg(feature = "azure_blob")]
            BackendCredentials::AzureBlob(c) => Arc::new(AzureBlobProvider::new(c)),
            #[cfg(feature = "gcs")]
            BackendCredentials::GCS(c) => Arc::new(GCSProvider::new(c)),
            #[cfg(feature = "b2")]
            BackendCredentials::B2(c) => Arc::new(B2Provider::new(c)),
            #[cfg(feature = "pcloud")]
            BackendCredentials::PCloud(c) => Arc::new(PCloudProvider::new(c)),
            #[cfg(feature = "ipfs")]
            BackendCredentials::IPFS(c) => Arc::new(IPFSProvider::new(c)),
        }
    }

    /// Dynamically instantiates a fully wrapped provider (with fallback/encryption/limiters) using its config credentials.
    #[allow(unreachable_patterns)]
    pub fn build_wrapped(
        creds: BackendCredentials,
        sim_root: std::path::PathBuf,
        global_upload_limiter: Option<crate::rate_limit::TokenBucket>,
        global_download_limiter: Option<crate::rate_limit::TokenBucket>,
    ) -> Arc<dyn StorageBackend> {
        let provider_name = match &creds {
            #[cfg(feature = "google_drive")]
            BackendCredentials::GoogleDrive(_) => "Google Drive",
            #[cfg(feature = "dropbox")]
            BackendCredentials::Dropbox(_) => "Dropbox",
            #[cfg(feature = "onedrive")]
            BackendCredentials::OneDrive(_) => "OneDrive",
            #[cfg(feature = "webdav")]
            BackendCredentials::WebDAV(_) => "WebDAV",
            #[cfg(feature = "s3")]
            BackendCredentials::S3(_) => "S3",
            #[cfg(feature = "sftp")]
            BackendCredentials::SFTP(_) => "SFTP",
            #[cfg(feature = "nextcloud")]
            BackendCredentials::Nextcloud(_) => "Nextcloud",
            #[cfg(feature = "box")]
            BackendCredentials::Box(_) => "Box",
            #[cfg(feature = "mega")]
            BackendCredentials::Mega(_) => "MEGA",
            #[cfg(feature = "azure_blob")]
            BackendCredentials::AzureBlob(_) => "Azure Blob",
            #[cfg(feature = "gcs")]
            BackendCredentials::GCS(_) => "GCS",
            #[cfg(feature = "b2")]
            BackendCredentials::B2(_) => "B2",
            #[cfg(feature = "pcloud")]
            BackendCredentials::PCloud(_) => "pCloud",
            #[cfg(feature = "ipfs")]
            BackendCredentials::IPFS(_) => "IPFS",
            _ => unreachable!(),
        };

        let sync_mode = creds.sync_mode();
        let max_upload_rate = creds.max_upload_rate();
        let max_download_rate = creds.max_download_rate();
        let encryption_password = creds.encryption_password();

        let upload_limiter = max_upload_rate
            .map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024))
            .or(global_upload_limiter);
        let download_limiter = max_download_rate
            .map(|rate| crate::rate_limit::TokenBucket::new(rate * 1024))
            .or(global_download_limiter);

        let inner = Self::build(creds.clone());

        let local_sim = local_sim::LocalSimulation::new(sim_root, provider_name.to_string())
            .with_limiters(upload_limiter.clone(), download_limiter.clone());
        let fallback = fallback::SimulatedFallback::new(Some(inner), local_sim, provider_name, sync_mode);

        let rate_limited = crate::rate_limit::RateLimitingBackend::new(
            fallback,
            upload_limiter,
            download_limiter,
        );

        if let Some(password) = encryption_password {
            Arc::new(encryption::EncryptedBackend::new(rate_limited, password))
        } else {
            Arc::new(rate_limited)
        }
    }
}

