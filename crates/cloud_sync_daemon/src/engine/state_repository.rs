use cloud_sync_lib::SyncState;
use std::path::Path;

/// Single Responsibility repository managing atomic persistence and snapshot loading for sync state catalog.
#[derive(Debug, Clone, Default)]
pub struct SyncStateRepository;

impl SyncStateRepository {
    pub fn new() -> Self {
        Self
    }

    /// Loads sync state snapshot from disk or returns default if file does not exist.
    pub async fn load(&self, state_file_path: &Path) -> SyncState {
        SyncState::load(state_file_path).await.unwrap_or_default()
    }

    /// Saves updated sync state catalog atomically to disk.
    pub async fn save(&self, sync_state: &SyncState, state_file_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        sync_state.save(state_file_path).await?;
        Ok(())
    }
}
