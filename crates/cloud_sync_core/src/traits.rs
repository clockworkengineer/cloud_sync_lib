use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

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
            StorageError::RateLimit { message, .. } => {
                write!(f, "Rate limit exceeded. Try again later: {}", message)
            }
            StorageError::Provider { message, status } => {
                write!(
                    f,
                    "Storage provider error: {} (status: {:?})",
                    message, status
                )
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
pub struct RemoteChange {
    #[cfg(feature = "std")]
    pub path: PathBuf,
    #[cfg(not(feature = "std"))]
    pub path: String,

    pub is_deleted: bool,
    pub item: Option<StorageItem>,
}

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
    pub permissions: Option<u32>,
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
pub trait StorageReader: Send + Sync {
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError>;
    async fn compute_local_checksum(
        &self,
        _local_path: &Path,
    ) -> Result<Option<String>, StorageError> {
        Ok(None)
    }
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
pub trait StorageWriter: Send + Sync {
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError>;
    async fn delete(&self, remote_path: &str) -> Result<(), StorageError>;
}

#[async_trait::async_trait]
pub trait DirectoryLister: Send + Sync {
    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError>;
    async fn create_folder(&self, _remote_path: &str) -> Result<(), StorageError> {
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait ServerSideOperations: Send + Sync {
    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError>;
    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError>;
}

#[async_trait::async_trait]
pub trait ChangeFeed: Send + Sync {
    async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError>;
    async fn fetch_changes(
        &self,
        cursor: &str,
    ) -> Result<Option<(Vec<RemoteChange>, String)>, StorageError>;
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
pub trait ResumableUploader: Send + Sync {
    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn ResumableUploadSession>, StorageError>;
}

#[async_trait::async_trait]
pub trait FolderOps: Send + Sync {
    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError>;
    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError>;
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
pub trait ChecksumOps: Send + Sync {
    async fn compute_local_checksum(
        &self,
        local_path: &Path,
    ) -> Result<Option<String>, StorageError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct StorageCapabilities(pub u32);

impl StorageCapabilities {
    pub const NONE: Self = Self(0);
    pub const RESUMABLE_UPLOAD: Self = Self(1 << 0);
    pub const SERVER_SIDE_MOVE: Self = Self(1 << 1);
    pub const SERVER_SIDE_COPY: Self = Self(1 << 2);
    pub const STREAMING_READ: Self = Self(1 << 3);
    pub const BATCH_DELETE: Self = Self(1 << 4);
    pub const CHECKSUM_MD5: Self = Self(1 << 5);
    pub const CHECKSUM_SHA256: Self = Self(1 << 6);
    pub const CHANGE_CURSOR: Self = Self(1 << 7);

    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn bits(&self) -> u32 {
        self.0
    }

    pub const fn from_bits_truncate(bits: u32) -> Self {
        Self(bits)
    }
}

impl std::ops::BitOr for StorageCapabilities {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for StorageCapabilities {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for StorageCapabilities {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::BitAndAssign for StorageCapabilities {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl std::ops::Not for StorageCapabilities {
    type Output = Self;
    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
pub trait ResumableUploadSession: Send + Sync {
    /// Returns total file size in bytes.
    fn total_bytes(&self) -> u64;

    /// Returns the number of bytes successfully uploaded so far.
    fn uploaded_bytes(&self) -> u64;

    /// Uploads the next slice/chunk and returns (uploaded_so_far, total_bytes).
    async fn upload_next_chunk(&mut self) -> Result<(u64, u64), StorageError>;

    /// Returns true if the session has uploaded all parts.
    fn is_complete(&self) -> bool;

    /// Finalizes the upload and commits the file remotely, optionally returning remote checksum.
    async fn finish(&mut self) -> Result<Option<String>, StorageError>;
}

#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    fn name(&self) -> &str;

    /// Returns the advertised capabilities of this backend.
    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::NONE
    }

    /// Accessor for server-side move/copy operations if supported adhering to LSP.
    fn as_server_side_ops(&self) -> Option<&dyn ServerSideOperations> {
        None
    }

    /// Accessor for incremental change feed operations if supported adhering to LSP.
    fn as_change_feed(&self) -> Option<&dyn ChangeFeed> {
        None
    }

    /// Accessor for chunked resumable uploader operations if supported adhering to LSP.
    #[cfg(feature = "std")]
    fn as_resumable_uploader(&self) -> Option<&dyn ResumableUploader> {
        None
    }

    #[cfg(feature = "std")]
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError>;

    #[cfg(feature = "std")]
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError>;

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError>;

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError>;

    async fn create_folder(&self, _remote_path: &str) -> Result<(), StorageError> {
        Ok(())
    }

    async fn rename(&self, _from: &str, _to: &str) -> Result<(), StorageError> {
        Err(StorageError::Conflict(String::from("Rename not supported")))
    }

    async fn copy(&self, _from: &str, _to: &str) -> Result<(), StorageError> {
        Err(StorageError::Conflict(String::from("Copy not supported")))
    }

    #[cfg(feature = "std")]
    async fn start_resumable_upload(
        &self,
        _local_path: &Path,
        _remote_path: &str,
        _chunk_size: usize,
    ) -> Result<Box<dyn ResumableUploadSession>, StorageError> {
        Err(StorageError::Conflict(String::from(
            "Resumable upload not supported",
        )))
    }

    #[cfg(feature = "std")]
    async fn compute_local_checksum(
        &self,
        _local_path: &Path,
    ) -> Result<Option<String>, StorageError> {
        Ok(None)
    }

    /// Fetches the latest change-feed cursor token for incremental change detection, if supported.
    async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError> {
        Ok(None)
    }

    /// Fetches remote changes since the provided cursor token, if supported.
    async fn fetch_changes(
        &self,
        _cursor: &str,
    ) -> Result<Option<(Vec<RemoteChange>, String)>, StorageError> {
        Ok(None)
    }

    /// Returns true if this backend is currently operating in local simulated mode or fallback.
    fn is_simulated(&self) -> bool {
        false
    }

    /// Returns the reason or description if operating in simulated mode.
    fn simulation_reason(&self) -> Option<String> {
        None
    }
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
impl<B: StorageBackend + ?Sized> StorageBackend for Box<B> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn capabilities(&self) -> StorageCapabilities {
        (**self).capabilities()
    }
    fn as_server_side_ops(&self) -> Option<&dyn ServerSideOperations> {
        (**self).as_server_side_ops()
    }
    fn as_change_feed(&self) -> Option<&dyn ChangeFeed> {
        (**self).as_change_feed()
    }
    fn as_resumable_uploader(&self) -> Option<&dyn ResumableUploader> {
        (**self).as_resumable_uploader()
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
    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        (**self).rename(from, to).await
    }
    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        (**self).copy(from, to).await
    }
    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn ResumableUploadSession>, StorageError> {
        (**self)
            .start_resumable_upload(local_path, remote_path, chunk_size)
            .await
    }
    async fn compute_local_checksum(
        &self,
        local_path: &Path,
    ) -> Result<Option<String>, StorageError> {
        (**self).compute_local_checksum(local_path).await
    }
    async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError> {
        (**self).get_latest_cursor().await
    }
    async fn fetch_changes(
        &self,
        cursor: &str,
    ) -> Result<Option<(Vec<RemoteChange>, String)>, StorageError> {
        (**self).fetch_changes(cursor).await
    }
    fn is_simulated(&self) -> bool {
        (**self).is_simulated()
    }
    fn simulation_reason(&self) -> Option<String> {
        (**self).simulation_reason()
    }
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
impl<B: StorageBackend + ?Sized> StorageBackend for std::sync::Arc<B> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn capabilities(&self) -> StorageCapabilities {
        (**self).capabilities()
    }
    fn as_server_side_ops(&self) -> Option<&dyn ServerSideOperations> {
        (**self).as_server_side_ops()
    }
    fn as_change_feed(&self) -> Option<&dyn ChangeFeed> {
        (**self).as_change_feed()
    }
    fn as_resumable_uploader(&self) -> Option<&dyn ResumableUploader> {
        (**self).as_resumable_uploader()
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
    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        (**self).rename(from, to).await
    }
    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        (**self).copy(from, to).await
    }
    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn ResumableUploadSession>, StorageError> {
        (**self)
            .start_resumable_upload(local_path, remote_path, chunk_size)
            .await
    }
    async fn compute_local_checksum(
        &self,
        local_path: &Path,
    ) -> Result<Option<String>, StorageError> {
        (**self).compute_local_checksum(local_path).await
    }
    async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError> {
        (**self).get_latest_cursor().await
    }
    async fn fetch_changes(
        &self,
        cursor: &str,
    ) -> Result<Option<(Vec<RemoteChange>, String)>, StorageError> {
        (**self).fetch_changes(cursor).await
    }
    fn is_simulated(&self) -> bool {
        (**self).is_simulated()
    }
    fn simulation_reason(&self) -> Option<String> {
        (**self).simulation_reason()
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
    Manual,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::sync::Arc;
    use alloc::vec;

    struct DummyBackend;

    #[async_trait::async_trait]
    impl StorageBackend for DummyBackend {
        fn name(&self) -> &str {
            "dummy"
        }
        #[cfg(feature = "std")]
        async fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), StorageError> {
            Ok(())
        }
        #[cfg(feature = "std")]
        async fn download(
            &self,
            _remote_path: &str,
            _local_path: &Path,
        ) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete(&self, _remote_path: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list(&self, _remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_sync_policy() {
        let policy_two_way = SyncPolicy::new(SyncMode::TwoWay);
        assert!(policy_two_way.sync_deletions());
        assert!(policy_two_way.sync_both());

        let policy_one_way = SyncPolicy::new(SyncMode::OneWay);
        assert!(policy_one_way.sync_deletions());
        assert!(!policy_one_way.sync_both());

        let policy_no_del = SyncPolicy::new(SyncMode::OneWayNoDeletions);
        assert!(!policy_no_del.sync_deletions());
        assert!(!policy_no_del.sync_both());
    }

    #[tokio::test]
    #[cfg(feature = "std")]
    async fn test_backend_delegations() {
        let boxed: Box<dyn StorageBackend> = Box::new(DummyBackend);
        assert_eq!(boxed.name(), "dummy");
        boxed.upload(Path::new("local"), "remote").await.unwrap();
        boxed.download("remote", Path::new("local")).await.unwrap();
        boxed.delete("remote").await.unwrap();
        assert!(boxed.list("").await.unwrap().is_empty());
        boxed.create_folder("folder").await.unwrap();
        assert!(boxed.rename("from", "to").await.is_err());
        assert!(boxed.copy("from", "to").await.is_err());
        assert!(boxed
            .start_resumable_upload(Path::new("local"), "remote", 1024)
            .await
            .is_err());
        assert_eq!(boxed.capabilities(), StorageCapabilities::NONE);
        assert!(boxed
            .compute_local_checksum(Path::new("local"))
            .await
            .unwrap()
            .is_none());

        let arc: Arc<dyn StorageBackend> = Arc::new(DummyBackend);
        assert_eq!(arc.name(), "dummy");
        arc.upload(Path::new("local"), "remote").await.unwrap();
        arc.download("remote", Path::new("local")).await.unwrap();
        arc.delete("remote").await.unwrap();
        assert!(arc.list("").await.unwrap().is_empty());
        arc.create_folder("folder").await.unwrap();
        assert!(arc.rename("from", "to").await.is_err());
        assert!(arc.copy("from", "to").await.is_err());
        assert!(arc
            .start_resumable_upload(Path::new("local"), "remote", 1024)
            .await
            .is_err());
        assert_eq!(arc.capabilities(), StorageCapabilities::NONE);
        assert!(arc
            .compute_local_checksum(Path::new("local"))
            .await
            .unwrap()
            .is_none());
        assert!(arc.as_server_side_ops().is_none());
        assert!(arc.as_change_feed().is_none());
        assert!(arc.as_resumable_uploader().is_none());
    }

    struct AdvancedBackend;

    #[async_trait::async_trait]
    impl ServerSideOperations for AdvancedBackend {
        async fn copy(&self, _from: &str, _to: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn rename(&self, _from: &str, _to: &str) -> Result<(), StorageError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ChangeFeed for AdvancedBackend {
        async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError> {
            Ok(Some("cursor_123".to_string()))
        }
        async fn fetch_changes(
            &self,
            _cursor: &str,
        ) -> Result<Option<(Vec<RemoteChange>, String)>, StorageError> {
            Ok(Some((vec![], "cursor_456".to_string())))
        }
    }

    #[async_trait::async_trait]
    impl StorageBackend for AdvancedBackend {
        fn name(&self) -> &str {
            "advanced"
        }
        fn capabilities(&self) -> StorageCapabilities {
            StorageCapabilities::SERVER_SIDE_MOVE | StorageCapabilities::SERVER_SIDE_COPY
        }
        fn as_server_side_ops(&self) -> Option<&dyn ServerSideOperations> {
            Some(self)
        }
        fn as_change_feed(&self) -> Option<&dyn ChangeFeed> {
            Some(self)
        }
        #[cfg(feature = "std")]
        async fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), StorageError> {
            Ok(())
        }
        #[cfg(feature = "std")]
        async fn download(
            &self,
            _remote_path: &str,
            _local_path: &Path,
        ) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete(&self, _remote_path: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list(&self, _remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_segregated_capabilities_and_lsp() {
        let backend: Arc<dyn StorageBackend> = Arc::new(AdvancedBackend);

        // LSP verification: capability discovery via typed interface query
        let server_ops = backend.as_server_side_ops();
        assert!(server_ops.is_some());
        assert!(server_ops.unwrap().rename("old", "new").await.is_ok());
        assert!(server_ops.unwrap().copy("src", "dst").await.is_ok());

        let change_feed = backend.as_change_feed();
        assert!(change_feed.is_some());
        let cur = change_feed.unwrap().get_latest_cursor().await.unwrap();
        assert_eq!(cur.as_deref(), Some("cursor_123"));

        let resumable = backend.as_resumable_uploader();
        assert!(resumable.is_none());
    }

    #[test]
    fn test_storage_capabilities() {
        let mut caps = StorageCapabilities::NONE;
        assert_eq!(caps.bits(), 0);
        assert!(!caps.contains(StorageCapabilities::RESUMABLE_UPLOAD));

        caps.insert(StorageCapabilities::RESUMABLE_UPLOAD);
        caps.insert(StorageCapabilities::SERVER_SIDE_MOVE);
        assert!(caps.contains(StorageCapabilities::RESUMABLE_UPLOAD));
        assert!(caps.contains(StorageCapabilities::SERVER_SIDE_MOVE));
        assert!(!caps.contains(StorageCapabilities::SERVER_SIDE_COPY));

        caps.remove(StorageCapabilities::RESUMABLE_UPLOAD);
        assert!(!caps.contains(StorageCapabilities::RESUMABLE_UPLOAD));
        assert!(caps.contains(StorageCapabilities::SERVER_SIDE_MOVE));

        let union_caps = caps.union(StorageCapabilities::SERVER_SIDE_COPY);
        assert!(union_caps.contains(StorageCapabilities::SERVER_SIDE_MOVE));
        assert!(union_caps.contains(StorageCapabilities::SERVER_SIDE_COPY));
    }

    #[test]
    fn test_storage_error_display() {
        let err = StorageError::Authentication("failed".to_string());
        assert!(err.to_string().contains("Authentication failed"));

        let err = StorageError::NotFound("lost".to_string());
        assert!(err.to_string().contains("Resource not found"));

        let err = StorageError::RateLimit {
            message: "slow down".to_string(),
            #[cfg(feature = "std")]
            retry_after: None,
        };
        assert!(err.to_string().contains("Rate limit exceeded"));

        let err = StorageError::Provider {
            message: "fail".to_string(),
            status: Some(500),
        };
        assert!(err.to_string().contains("status: Some(500)"));

        let err = StorageError::AuthenticationExpired("expired".to_string());
        assert!(err.to_string().contains("Authentication expired"));

        let err = StorageError::Conflict("conflict".to_string());
        assert!(err.to_string().contains("Conflict"));

        let err = StorageError::ConnectionFailed("disconnect".to_string());
        assert!(err.to_string().contains("Connection failed"));

        #[cfg(feature = "std")]
        {
            let io_err = StorageError::from(std::io::Error::other("iofail"));
            assert!(io_err.to_string().contains("I/O error occurred"));
        }
    }

    #[tokio::test]
    #[cfg(feature = "std")]
    async fn test_reqwest_error_conversion() {
        let err = reqwest::get("http://[::1]:0").await.unwrap_err();
        let storage_err = StorageError::from(err);
        assert!(storage_err.to_string().contains("HTTP client error"));
    }
}
