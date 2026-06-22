use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("I/O error occurred: {0}")]
    Io(#[from] std::io::Error),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Rate limit exceeded. Try again later: {0}")]
    RateLimit(String),

    #[error("Storage provider error: {0}")]
    Provider(String),
}

#[derive(Debug, Clone)]
pub struct StorageItem {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
    pub is_dir: bool,
}

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
}
