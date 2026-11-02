//! Transactional disk persistence and recovery for SyncState catalog (SRP).

#[cfg(feature = "std")]
use alloc::boxed::Box;
#[cfg(feature = "std")]
use alloc::format;
#[cfg(feature = "std")]
use alloc::string::ToString;
#[cfg(feature = "std")]
use crate::state::{compute_checksum, SyncState};
#[cfg(feature = "std")]
use crate::traits::StorageError;
#[cfg(feature = "std")]
use std::path::Path;

const CSTX_MAGIC: &[u8; 4] = b"CSTX";
const CSTX_VERSION: u8 = 1;

/// Trait defining atomic persistence and loading for synchronization catalog states (SRP).
#[cfg(feature = "std")]
#[async_trait::async_trait]
pub trait StateStore: Send + Sync {
    /// Loads the sync state catalog from disk, recovering from backup if corrupt.
    async fn load(&self, path: &Path) -> Result<SyncState, StorageError>;

    /// Persists the sync state catalog atomically with crash resilience.
    async fn save(&self, path: &Path, state: &SyncState) -> Result<(), StorageError>;
}

/// Transactional state store utilizing Postcard binary encoding, FNV-1a checksums,
/// atomic temporary write-rename, and automatic `.bak` recovery.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Default)]
pub struct PostcardStateStore;

#[cfg(feature = "std")]
impl PostcardStateStore {
    pub fn new() -> Self {
        Self
    }

    async fn load_raw(&self, path: &Path) -> std::io::Result<SyncState> {
        let bytes = tokio::fs::read(path).await?;

        // 1. Check for transactional CSTX header with checksum
        if bytes.len() >= 13 && &bytes[..4] == CSTX_MAGIC {
            let version = bytes[4];
            if version == CSTX_VERSION {
                let mut checksum_bytes = [0u8; 8];
                checksum_bytes.copy_from_slice(&bytes[5..13]);
                let expected_checksum = u64::from_be_bytes(checksum_bytes);
                let payload = &bytes[13..];
                let actual_checksum = compute_checksum(payload);

                if expected_checksum == actual_checksum {
                    if let Ok(state) = SyncState::from_bytes(payload) {
                        return Ok(state);
                    }
                }
            }
        }

        // 2. Legacy postcard binary fallback
        if let Ok(state) = SyncState::from_bytes(&bytes) {
            return Ok(state);
        }

        // 3. Legacy JSON format fallback
        if let Ok(data_str) = core::str::from_utf8(&bytes) {
            if let Ok(state) = serde_json::from_str::<SyncState>(data_str) {
                return Ok(state);
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Failed to deserialize sync state",
        ))
    }
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
impl StateStore for PostcardStateStore {
    async fn load(&self, path: &Path) -> Result<SyncState, StorageError> {
        if !path.exists() {
            let bak_path = path.with_extension("bak");
            if bak_path.exists() {
                if let Ok(recovered) = self.load_raw(&bak_path).await {
                    return Ok(recovered);
                }
            }
            return Ok(SyncState::default());
        }

        match self.load_raw(path).await {
            Ok(state) => Ok(state),
            Err(_) => {
                let bak_path = path.with_extension("bak");
                if bak_path.exists() {
                    if let Ok(recovered) = self.load_raw(&bak_path).await {
                        return Ok(recovered);
                    }
                }
                Err(StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Failed to deserialize sync state (checksum mismatch or corrupt data)",
                )))
            }
        }
    }

    async fn save(&self, path: &Path, state: &SyncState) -> Result<(), StorageError> {
        let payload = state
            .to_bytes()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        let checksum = compute_checksum(&payload);

        let mut framed_bytes = alloc::vec::Vec::with_capacity(13 + payload.len());
        framed_bytes.extend_from_slice(CSTX_MAGIC);
        framed_bytes.push(CSTX_VERSION);
        framed_bytes.extend_from_slice(&checksum.to_be_bytes());
        framed_bytes.extend_from_slice(&payload);

        let temp_filename = format!(
            ".{}_{}.tmp",
            path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "state".to_string()),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );

        let temp_path = match path.parent() {
            Some(parent) => {
                tokio::fs::create_dir_all(parent).await?;
                parent.join(temp_filename)
            }
            None => std::path::PathBuf::from(temp_filename),
        };

        {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::File::create(&temp_path).await?;
            file.write_all(&framed_bytes).await?;
            file.sync_all().await?;
        }

        if path.exists() {
            let bak_path = path.with_extension("bak");
            let _ = tokio::fs::copy(path, &bak_path).await;
        }

        tokio::fs::rename(&temp_path, path).await?;
        Ok(())
    }
}

/// Alternative human-readable JSON state store with atomic save and `.bak` recovery.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Default)]
pub struct JsonStateStore;

#[cfg(feature = "std")]
impl JsonStateStore {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
impl StateStore for JsonStateStore {
    async fn load(&self, path: &Path) -> Result<SyncState, StorageError> {
        match tokio::fs::read(path).await {
            Ok(bytes) => {
                if let Ok(state) = serde_json::from_slice(&bytes) {
                    return Ok(state);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SyncState::default());
            }
            Err(_) => {}
        }

        let bak_path = path.with_extension("bak");
        if bak_path.exists() {
            if let Ok(bytes) = tokio::fs::read(&bak_path).await {
                if let Ok(state) = serde_json::from_slice(&bytes) {
                    return Ok(state);
                }
            }
        }

        Ok(SyncState::default())
    }

    async fn save(&self, path: &Path, state: &SyncState) -> Result<(), StorageError> {
        let payload = serde_json::to_vec_pretty(state)
            .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;

        let temp_filename = format!(
            ".{}_{}.tmp",
            path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "state".to_string()),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );

        let temp_path = match path.parent() {
            Some(parent) => {
                tokio::fs::create_dir_all(parent).await?;
                parent.join(temp_filename)
            }
            None => std::path::PathBuf::from(temp_filename),
        };

        {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::File::create(&temp_path).await?;
            file.write_all(&payload).await?;
            file.sync_all().await?;
        }

        if path.exists() {
            let bak_path = path.with_extension("bak");
            let _ = tokio::fs::copy(path, &bak_path).await;
        }

        tokio::fs::rename(&temp_path, path).await?;
        Ok(())
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::state::FileState;
    use tempfile::tempdir;

    fn make_test_file_state(size: u64, hash: Option<&str>) -> FileState {
        FileState {
            size,
            local_modified: std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1700000000),
            remote_modified: std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1700000000),
            is_dir: Some(false),
            checksum: hash.map(|h| h.into()),
            permissions: Some(0o644),
        }
    }

    #[tokio::test]
    async fn test_postcard_state_store_roundtrip() {
        let dir = tempdir().unwrap();
        let state_path = dir.path().join("state.bin");
        let store = PostcardStateStore::new();

        let mut state = SyncState::default();
        state.files.insert(
            "docs/report.pdf".into(),
            make_test_file_state(1024, Some("sha256:abc123")),
        );

        // 1. Initial save
        store.save(&state_path, &state).await.expect("save should succeed");
        assert!(state_path.exists());

        // 2. Load
        let loaded = store.load(&state_path).await.expect("load should succeed");
        assert_eq!(loaded.files.len(), 1);
        let meta = loaded.files.get("docs/report.pdf").expect("file should be present");
        assert_eq!(meta.size, 1024);
        assert_eq!(meta.checksum.as_deref(), Some("sha256:abc123"));
    }

    #[tokio::test]
    async fn test_postcard_state_store_corrupt_recovers_from_backup() {
        let dir = tempdir().unwrap();
        let state_path = dir.path().join("catalog.bin");
        let store = PostcardStateStore::new();

        let mut state_v1 = SyncState::default();
        state_v1.files.insert(
            "v1.txt".into(),
            make_test_file_state(10, None),
        );

        // Save v1 -> creates state_path
        store.save(&state_path, &state_v1).await.unwrap();

        let mut state_v2 = SyncState::default();
        state_v2.files.insert(
            "v2.txt".into(),
            make_test_file_state(20, None),
        );

        // Save v2 -> copies state_path to .bak and writes new state_path
        store.save(&state_path, &state_v2).await.unwrap();
        let bak_path = state_path.with_extension("bak");
        assert!(bak_path.exists());

        // Corrupt main state file
        tokio::fs::write(&state_path, b"CORRUPTED_GARBAGE_DATA_12345").await.unwrap();

        // Load should detect corruption and gracefully recover from .bak
        let recovered = store.load(&state_path).await.expect("should recover from .bak");
        assert!(recovered.files.contains_key("v1.txt"));
    }

    #[tokio::test]
    async fn test_postcard_state_store_non_existent_returns_default() {
        let dir = tempdir().unwrap();
        let non_existent = dir.path().join("does_not_exist.bin");
        let store = PostcardStateStore::new();

        let loaded = store.load(&non_existent).await.expect("missing file should yield default");
        assert!(loaded.files.is_empty());
    }

    #[tokio::test]
    async fn test_json_state_store_roundtrip_and_bak() {
        let dir = tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        let store = JsonStateStore::new();

        let mut state = SyncState::default();
        state.files.insert(
            "notes.txt".into(),
            make_test_file_state(55, Some("md5:999")),
        );

        // Save
        store.save(&state_path, &state).await.expect("save should succeed");
        assert!(state_path.exists());

        // Load
        let loaded = store.load(&state_path).await.expect("load should succeed");
        assert_eq!(loaded.files.len(), 1);
        assert_eq!(loaded.files.get("notes.txt").unwrap().size, 55);

        // Corrupt main file and verify .bak recovery
        let bak_path = state_path.with_extension("bak");
        tokio::fs::copy(&state_path, &bak_path).await.unwrap();
        tokio::fs::write(&state_path, b"{ invalid json ...").await.unwrap();

        let recovered = store.load(&state_path).await.expect("should recover from json .bak");
        assert_eq!(recovered.files.get("notes.txt").unwrap().size, 55);
    }

    #[tokio::test]
    async fn test_json_state_store_non_existent_returns_default() {
        let dir = tempdir().unwrap();
        let store = JsonStateStore::new();
        let loaded = store.load(&dir.path().join("missing.json")).await.unwrap();
        assert!(loaded.files.is_empty());
    }
}
