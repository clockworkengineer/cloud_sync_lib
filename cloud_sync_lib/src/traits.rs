//! Trait definitions and core storage abstractions for `cloud_sync_lib`.
//!
//! This module defines the common error types, item metadata structure,
//! and the `StorageBackend` trait that all cloud storage providers must implement.

use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;
use crate::providers::SyncMode;

/// Storage errors that can occur when executing storage backend operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Errors originating from local I/O operations.
    #[error("I/O error occurred: {0}")]
    Io(#[from] std::io::Error),

    /// Errors resulting from authorization or access token retrieval issues.
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Errors when the requested remote resource does not exist.
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Errors resulting from provider rate limits / API throttling.
    #[error("Rate limit exceeded. Try again later: {0}")]
    RateLimit(String),

    /// Errors returned from the cloud provider's API.
    #[error("Storage provider error: {0}")]
    Provider(String),

    /// Errors originating from the underlying HTTP client library.
    #[error("HTTP client error: {0}")]
    Reqwest(#[from] reqwest::Error),
}

/// Common metadata returned by listings or query commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageItem {
    /// Relative path representing the remote storage resource.
    pub path: PathBuf,
    /// Exact file size in bytes (set to 0 for directories).
    pub size: u64,
    /// Last modification date from the storage provider.
    pub modified: SystemTime,
    /// Indicates whether the item is a folder or directory.
    pub is_dir: bool,
}

/// Generic trait representing any storage target (e.g. S3, Google Drive, Local Simulation).
///
/// Implements standard REST-like commands for manipulation and query,
/// with support for custom prefixes, directories, and error handling.
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    /// Returns the user-friendly name of the storage backend.
    fn name(&self) -> &str;

    /// Configures the upload and download rate limiters on the backend.
    fn with_limiters(
        self,
        _upload_limiter: Option<crate::rate_limit::TokenBucket>,
        _download_limiter: Option<crate::rate_limit::TokenBucket>,
    ) -> Self
    where
        Self: Sized,
    {
        self
    }

    /// Uploads a file from `local_path` to the cloud's `remote_path`.
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError>;

    /// Downloads a file from the cloud's `remote_path` to `local_path`.
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError>;

    /// Deletes a file or directory at the cloud's `remote_path`.
    async fn delete(&self, remote_path: &str) -> Result<(), StorageError>;

    /// Lists the contents of the cloud's directory at `remote_path`.
    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError>;

    /// Creates a folder/directory at the cloud's `remote_path`.
    async fn create_folder(&self, _remote_path: &str) -> Result<(), StorageError> {
        Ok(())
    }

    /// Returns the sync mode for the backend.
    fn sync_mode(&self) -> SyncMode {
        SyncMode::OneWay
    }

    /// Returns whether the backend should sync deletions.
    fn sync(&self) -> bool {
        match self.sync_mode() {
            SyncMode::TwoWay | SyncMode::OneWay => true,
            SyncMode::OneWayNoDeletions => false,
        }
    }

    /// Returns whether the backend should sync both ways (bidirectionally).
    fn sync_both(&self) -> bool {
        match self.sync_mode() {
            SyncMode::TwoWay => true,
            SyncMode::OneWay | SyncMode::OneWayNoDeletions => false,
        }
    }
}
