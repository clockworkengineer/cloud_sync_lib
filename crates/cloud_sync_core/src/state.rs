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
    pub is_dir: Option<bool>,
    /// Optional checksum of the file.
    pub checksum: Option<alloc::string::String>,
    /// Optional file permissions (Unix mode bits).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_from_bytes() {
        let mut state = SyncState::default();
        let file_state = FileState {
            size: 1024,
            #[cfg(feature = "std")]
            local_modified: std::time::SystemTime::now(),
            #[cfg(not(feature = "std"))]
            local_modified: 0,
            #[cfg(feature = "std")]
            remote_modified: std::time::SystemTime::now(),
            #[cfg(not(feature = "std"))]
            remote_modified: 0,
            is_dir: Some(false),
            checksum: Some("abc".to_string()),
            permissions: Some(0o644),
        };
        state.files.insert("file.txt".to_string(), file_state);

        let bytes = state.to_bytes().unwrap();
        let decoded = SyncState::from_bytes(&bytes).unwrap();
        assert_eq!(state, decoded);
    }

    #[tokio::test]
    #[cfg(feature = "std")]
    async fn test_load_save_state() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.bin");

        // 1. Loading non-existent file
        let state = SyncState::load(&path).await.unwrap();
        assert_eq!(state, SyncState::default());

        // 2. Save new state
        let mut new_state = SyncState::default();
        let file_state = FileState {
            size: 200,
            local_modified: std::time::SystemTime::now(),
            remote_modified: std::time::SystemTime::now(),
            is_dir: None,
            checksum: None,
            permissions: None,
        };
        new_state.files.insert("doc.pdf".to_string(), file_state);
        new_state.save(&path).await.unwrap();

        // 3. Load it back
        // 3. Load it back
        let loaded = SyncState::load(&path).await.unwrap();
        assert_eq!(loaded, new_state);

        // 4. Save identical state (should match fast-path early return)
        new_state.save(&path).await.unwrap();

        // 5. Load JSON fallback
        let json_path = dir.path().join("state.json");
        let json_str = serde_json::to_string(&new_state).unwrap();
        tokio::fs::write(&json_path, json_str).await.unwrap();
        let loaded_json = SyncState::load(&json_path).await.unwrap();
        assert_eq!(loaded_json, new_state);

        // 6. Invalid data loading error
        let bad_path = dir.path().join("bad.txt");
        tokio::fs::write(&bad_path, "invalid data content").await.unwrap();
        assert!(SyncState::load(&bad_path).await.is_err());
    }
}
