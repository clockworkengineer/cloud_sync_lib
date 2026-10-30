use alloc::string::ToString;
use serde::{Deserialize, Serialize};

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

/// Represents an atomic state mutation for write-ahead logging and incremental state updates.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum StateMutation {
    Upsert(alloc::string::String, FileState),
    Delete(alloc::string::String),
}

/// Computes a fast 64-bit FNV-1a checksum to verify state file integrity without external dependencies.
pub fn compute_checksum(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

impl SyncState {
    pub fn to_bytes(&self) -> Result<alloc::vec::Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }

    /// Applies a mutation directly to the in-memory state catalog.
    pub fn apply_mutation(&mut self, mutation: StateMutation) {
        match mutation {
            StateMutation::Upsert(path, state) => {
                self.files.insert(path, state);
            }
            StateMutation::Delete(path) => {
                self.files.remove(&path);
            }
        }
    }

    /// Loads the sync state catalog from the specified file path utilizing the SRP `StateStore`.
    #[cfg(feature = "std")]
    pub async fn load(path: &std::path::Path) -> std::io::Result<Self> {
        use crate::state_io::StateStore;
        crate::state_io::PostcardStateStore::new()
            .load(path)
            .await
            .map_err(|e| match e {
                crate::traits::StorageError::Io(io_err) => io_err,
                other => std::io::Error::other(other.to_string()),
            })
    }

    /// Saves the sync state catalog to disk utilizing the SRP `StateStore`.
    #[cfg(feature = "std")]
    pub async fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        use crate::state_io::StateStore;
        crate::state_io::PostcardStateStore::new()
            .save(path, self)
            .await
            .map_err(|e| match e {
                crate::traits::StorageError::Io(io_err) => io_err,
                other => std::io::Error::other(other.to_string()),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use std::println;

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

        /*
        4. Save identical state (should match fast-path early return)
        */
        new_state.save(&path).await.unwrap();

        // 5. Load JSON fallback
        let json_path = dir.path().join("state.json");
        let json_str = serde_json::to_string(&new_state).unwrap();
        tokio::fs::write(&json_path, json_str).await.unwrap();
        let loaded_json = SyncState::load(&json_path).await.unwrap();
        assert_eq!(loaded_json, new_state);

        // 6. Invalid data loading error
        let bad_path = dir.path().join("bad.txt");
        tokio::fs::write(&bad_path, "invalid data content")
            .await
            .unwrap();
        assert!(SyncState::load(&bad_path).await.is_err());
    }

    #[tokio::test]
    #[cfg(feature = "std")]
    async fn test_corrupted_state_recovers_from_backup() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.bin");

        let mut initial_state = SyncState::default();
        initial_state.files.insert(
            "file_a.txt".to_string(),
            FileState {
                size: 100,
                local_modified: std::time::SystemTime::now(),
                remote_modified: std::time::SystemTime::now(),
                is_dir: Some(false),
                checksum: Some("hash1".to_string()),
                permissions: None,
            },
        );

        // 1. Save initial state (creates primary file)
        initial_state.save(&path).await.unwrap();

        // 2. Save modified state (creates .bak of initial_state and saves new primary)
        let mut second_state = initial_state.clone();
        second_state.files.insert(
            "file_b.txt".to_string(),
            FileState {
                size: 200,
                local_modified: std::time::SystemTime::now(),
                remote_modified: std::time::SystemTime::now(),
                is_dir: Some(false),
                checksum: Some("hash2".to_string()),
                permissions: None,
            },
        );
        second_state.save(&path).await.unwrap();

        // 3. Corrupt primary file by flipping bytes in payload
        let mut raw_bytes = tokio::fs::read(&path).await.unwrap();
        let len = raw_bytes.len();
        raw_bytes[len - 2] ^= 0xFF; // corrupt payload
        tokio::fs::write(&path, &raw_bytes).await.unwrap();

        // 4. Load should detect checksum failure and seamlessly recover from .bak
        let recovered = SyncState::load(&path).await.unwrap();
        assert_eq!(recovered, initial_state);
    }

    #[test]
    fn test_mutation_apply() {
        let mut state = SyncState::default();
        let fs = FileState {
            size: 50,
            #[cfg(feature = "std")]
            local_modified: std::time::SystemTime::now(),
            #[cfg(not(feature = "std"))]
            local_modified: 0,
            #[cfg(feature = "std")]
            remote_modified: std::time::SystemTime::now(),
            #[cfg(not(feature = "std"))]
            remote_modified: 0,
            is_dir: Some(false),
            checksum: None,
            permissions: None,
        };

        state.apply_mutation(StateMutation::Upsert("item.txt".to_string(), fs.clone()));
        assert_eq!(state.files.get("item.txt"), Some(&fs));

        state.apply_mutation(StateMutation::Delete("item.txt".to_string()));
        assert_eq!(state.files.get("item.txt"), None);
    }
}
