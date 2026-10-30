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
