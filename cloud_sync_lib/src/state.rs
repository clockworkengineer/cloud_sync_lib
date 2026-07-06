use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;
use serde::{Serialize, Deserialize};

/// Represents the synced state of a single file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileState {
    /// Size of the file in bytes.
    pub size: u64,
    /// The last modified time on the local machine.
    pub local_modified: SystemTime,
    /// The last modified time on the remote cloud storage.
    pub remote_modified: SystemTime,
    /// Whether this entry represents a directory rather than a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_dir: Option<bool>,
}

/// Represents the overall catalog state of the synchronization.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SyncState {
    /// Mapping of relative file path string to the corresponding FileState.
    pub files: HashMap<String, FileState>,
}

impl SyncState {
    /// Loads the sync state catalog from the specified file path.
    /// Returns default state if the file does not exist.
    pub async fn load(path: &Path) -> std::io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = tokio::fs::read_to_string(path).await?;
        let state = serde_json::from_str(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(state)
    }

    /// Saves the current sync state catalog to the specified file path.
    pub async fn save(&self, path: &Path) -> std::io::Result<()> {
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        tokio::fs::write(path, data).await?;
        Ok(())
    }
}
