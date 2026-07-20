use serde::{Serialize, Deserialize};
use alloc::string::ToString;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

/// Represents the synced state of a single file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileState {
    /// Size of the file in bytes.
    pub size: u64,
    /// The last modified time on the local machine.
    #[cfg(feature = "std")]
    pub local_modified: std::time::SystemTime,
    #[cfg(not(feature = "std"))]
    pub local_modified: u64,

    /// The last modified time on the remote cloud storage.
    #[cfg(feature = "std")]
    pub remote_modified: std::time::SystemTime,
    #[cfg(not(feature = "std"))]
    pub remote_modified: u64,

    /// Whether this entry represents a directory rather than a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_dir: Option<bool>,
    /// Optional checksum of the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<alloc::string::String>,
    /// Optional file permissions (Unix mode bits).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<u32>,
}

/// Represents the overall catalog state of the synchronization.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SyncState {
    /// Mapping of relative file path string to the corresponding FileState.
    pub files: HashMap<alloc::string::String, FileState>,
}

impl SyncState {
    pub fn to_bytes(&self) -> Result<alloc::vec::Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }

    /// Loads the sync state catalog from the specified file path.
    /// Returns default state if the file does not exist.
    /// Fallback to JSON format is supported for seamless migration.
    #[cfg(feature = "std")]
    pub async fn load(path: &std::path::Path) -> std::io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = tokio::fs::read(path).await?;
        if let Ok(state) = Self::from_bytes(&bytes) {
            return Ok(state);
        }
        if let Ok(data_str) = core::str::from_utf8(&bytes) {
            // We expect standard JSON to parse correctly
            if let Ok(state) = serde_json::from_str::<Self>(data_str) {
                return Ok(state);
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Failed to deserialize sync state (neither valid postcard binary nor JSON)",
        ))
    }

    /// Saves the current sync state catalog to the specified file path.
    /// Only writes to disk if the state has changed.
    #[cfg(feature = "std")]
    pub async fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if path.exists() {
            if let Ok(existing) = Self::load(path).await {
                if existing == *self {
                    return Ok(());
                }
            }
        }
        let data = self.to_bytes()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        tokio::fs::write(path, data).await?;
        Ok(())
    }
}
