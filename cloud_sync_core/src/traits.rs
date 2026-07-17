use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;

#[cfg(feature = "std")]
use std::path::{Path, PathBuf};
#[cfg(feature = "std")]
use std::time::SystemTime;

#[derive(Debug)]
pub enum StorageError {
    #[cfg(feature = "std")]
    Io(std::io::Error),

    Authentication(String),
    NotFound(String),
    
    RateLimit {
        message: String,
        #[cfg(feature = "std")]
        retry_after: Option<std::time::Duration>,
    },

    Provider {
        message: String,
        status: Option<u16>,
    },

    #[cfg(feature = "std")]
    Reqwest(reqwest::Error),

    AuthenticationExpired(String),
    Conflict(String),
    ConnectionFailed(String),
}

#[cfg(feature = "std")]
impl From<std::io::Error> for StorageError {
    fn from(err: std::io::Error) -> Self {
        StorageError::Io(err)
    }
}

#[cfg(feature = "std")]
impl From<reqwest::Error> for StorageError {
    fn from(err: reqwest::Error) -> Self {
        StorageError::Reqwest(err)
    }
}

impl core::fmt::Display for StorageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            #[cfg(feature = "std")]
            StorageError::Io(e) => write!(f, "I/O error occurred: {}", e),
            StorageError::Authentication(s) => write!(f, "Authentication failed: {}", s),
            StorageError::NotFound(s) => write!(f, "Resource not found: {}", s),
            StorageError::RateLimit { message, .. } => write!(f, "Rate limit exceeded. Try again later: {}", message),
            StorageError::Provider { message, status } => {
                write!(f, "Storage provider error: {} (status: {:?})", message, status)
            }
            #[cfg(feature = "std")]
            StorageError::Reqwest(e) => write!(f, "HTTP client error: {}", e),
            StorageError::AuthenticationExpired(s) => write!(f, "Authentication expired: {}", s),
            StorageError::Conflict(s) => write!(f, "Conflict: {}", s),
            StorageError::ConnectionFailed(s) => write!(f, "Connection failed: {}", s),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for StorageError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageItem {
    #[cfg(feature = "std")]
    pub path: PathBuf,
    #[cfg(not(feature = "std"))]
    pub path: String,

    pub size: u64,

    #[cfg(feature = "std")]
    pub modified: SystemTime,
    #[cfg(not(feature = "std"))]
    pub modified: u64,

    pub is_dir: bool,
    pub checksum: Option<String>,
}

#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    fn name(&self) -> &str;

    #[cfg(feature = "std")]
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError>;

    #[cfg(feature = "std")]
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError>;

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError>;

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError>;

    async fn create_folder(&self, _remote_path: &str) -> Result<(), StorageError> {
        Ok(())
    }

    #[cfg(feature = "std")]
    async fn compute_local_checksum(&self, _local_path: &Path) -> Result<Option<String>, StorageError> {
        Ok(None)
    }
}

#[cfg(feature = "std")]
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

#[cfg(feature = "std")]
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

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SyncMode {
    TwoWay,
    #[default]
    OneWay,
    OneWayNoDeletions,
}

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

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    #[default]
    RenameLocal,
    RenameRemote,
    KeepNewer,
    KeepLocal,
    KeepRemote,
}
