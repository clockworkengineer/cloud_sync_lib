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
    #[error("Rate limit exceeded. Try again later: {message}")]
    RateLimit {
        message: String,
        retry_after: Option<std::time::Duration>,
    },

    /// Errors returned from the cloud provider's API.
    #[error("Storage provider error: {message} (status: {status:?})")]
    Provider {
        message: String,
        status: Option<u16>,
    },

    /// Errors originating from the underlying HTTP client library.
    #[error("HTTP client error: {0}")]
    Reqwest(#[from] reqwest::Error),

    /// Authentication expired or invalid credentials.
    #[error("Authentication expired: {0}")]
    AuthenticationExpired(String),

    /// A conflict occurred (e.g. duplicate folders or resource state mismatch).
    #[error("Conflict: {0}")]
    Conflict(String),

    /// A connection failure occurred with the remote host.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
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
    /// Optional checksum of the file (typically SHA-256, MD5, or provider-specific hash).
    pub checksum: Option<String>,
}

/// Generic trait representing any storage target (e.g. S3, Google Drive, Local Simulation).
///
/// Implements standard REST-like commands for manipulation and query,
/// with support for custom prefixes, directories, and error handling.
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    /// Returns the user-friendly name of the storage backend.
    fn name(&self) -> &str;

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

    /// Computes the local checksum of a file matching the provider's expected format.
    /// Returns `Ok(None)` if the provider does not support checksum verification.
    async fn compute_local_checksum(&self, _local_path: &Path) -> Result<Option<String>, StorageError> {
        Ok(None)
    }
}

#[async_trait::async_trait]
impl<B: StorageBackend + ?Sized> StorageBackend for Box<B> {
    fn name(&self) -> &str {
        (**self).name()
    }
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        (**self).upload(local_path, remote_path).await
    }
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        (**self).download(remote_path, local_path).await
    }
    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        (**self).delete(remote_path).await
    }
    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        (**self).list(remote_path).await
    }
    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        (**self).create_folder(remote_path).await
    }
    async fn compute_local_checksum(&self, local_path: &Path) -> Result<Option<String>, StorageError> {
        (**self).compute_local_checksum(local_path).await
    }
}

#[async_trait::async_trait]
impl<B: StorageBackend + ?Sized> StorageBackend for std::sync::Arc<B> {
    fn name(&self) -> &str {
        (**self).name()
    }
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        (**self).upload(local_path, remote_path).await
    }
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        (**self).download(remote_path, local_path).await
    }
    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        (**self).delete(remote_path).await
    }
    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        (**self).list(remote_path).await
    }
    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        (**self).create_folder(remote_path).await
    }
    async fn compute_local_checksum(&self, local_path: &Path) -> Result<Option<String>, StorageError> {
        (**self).compute_local_checksum(local_path).await
    }
}

/// Sync policy details indicating directionality and deletion behavior.
///
/// # Examples
///
/// ```
/// use cloud_sync_lib::{SyncPolicy, SyncMode};
/// let policy = SyncPolicy::new(SyncMode::TwoWay);
/// assert!(policy.sync_deletions());
/// assert!(policy.sync_both());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncPolicy {
    pub mode: SyncMode,
}

impl SyncPolicy {
    pub fn new(mode: SyncMode) -> Self {
        Self { mode }
    }

    pub fn sync_deletions(&self) -> bool {
        match self.mode {
            SyncMode::TwoWay | SyncMode::OneWay => true,
            SyncMode::OneWayNoDeletions => false,
        }
    }

    pub fn sync_both(&self) -> bool {
        match self.mode {
            SyncMode::TwoWay => true,
            SyncMode::OneWay | SyncMode::OneWayNoDeletions => false,
        }
    }
}
