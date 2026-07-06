//! Trait definitions and core storage abstractions for `cloud_sync_lib`.
//!
//! This module defines the common error types, item metadata structure,
//! and the `StorageBackend` trait that all cloud storage providers must implement.

use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

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

/// Metadata describing a single file or folder in a storage backend.
#[derive(Debug, Clone)]
pub struct StorageItem {
    /// Relative path of the item within the remote storage root.
    pub path: PathBuf,
    /// Size of the item in bytes.
    pub size: u64,
    /// Last modified timestamp of the item.
    pub modified: SystemTime,
    /// True if the item is a folder, false if it is a file.
    pub is_dir: bool,
}

/// The core trait defining capabilities of a cloud storage provider.
///
/// Any provider implementing this trait can be used by the daemon to sync files
/// bidirectionally or unidirectionally.
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    /// Returns the user-friendly name of the storage backend (e.g. "Google Drive").
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

    /// Returns whether the backend should sync deletions.
    fn sync(&self) -> bool {
        true
    }
}
