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
    pub async fn save(
        &self,
        sync_state: &SyncState,
        state_file_path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sync_state.save(state_file_path).await?;
        Ok(())
    }

    pub async fn resolve_state_file_path(
        &self,
        state_dir: &Path,
        watch_dir: &Path,
        safe_name: &str,
    ) -> std::path::PathBuf {
        resolve_state_file_path(state_dir, watch_dir, safe_name).await
    }

    pub async fn migrate_legacy_state_files(&self, watch_dir: &Path, state_dir: &Path) {
        migrate_legacy_state_files(watch_dir, state_dir).await
    }
}

/// Migrates any legacy `.sync_state` files found inside the watched directory to the dedicated `state_dir`.
pub async fn migrate_legacy_state_files(watch_dir: &Path, state_dir: &Path) {
    let _ = tokio::fs::create_dir_all(state_dir).await;
    if let Ok(mut entries) = tokio::fs::read_dir(watch_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            if name_str.starts_with(".sync_state") {
                let target_path = state_dir.join(&file_name);
                if !target_path.exists() {
                    let _ = tokio::fs::rename(entry.path(), &target_path).await;
                } else {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }
    }
}

/// Resolves the synchronization state database file path for a backend inside the designated `state_dir`.
/// Also migrates any legacy state file found in `watch_dir`.
pub async fn resolve_state_file_path(
    state_dir: &Path,
    watch_dir: &Path,
    safe_name: &str,
) -> std::path::PathBuf {
    let _ = tokio::fs::create_dir_all(state_dir).await;
    let state_filename = format!(".sync_state_{}.bin", safe_name);
    let state_file_path = state_dir.join(&state_filename);
    let old_state_path = watch_dir.join(&state_filename);

    if old_state_path.exists() {
        if !state_file_path.exists() {
            let _ = tokio::fs::rename(&old_state_path, &state_file_path).await;
        } else {
            let _ = tokio::fs::remove_file(&old_state_path).await;
        }
    }

    let bak_filename = format!(".sync_state_{}.bak", safe_name);
    let old_bak_path = watch_dir.join(&bak_filename);
    let bak_file_path = state_dir.join(&bak_filename);
    if old_bak_path.exists() {
        if !bak_file_path.exists() {
            let _ = tokio::fs::rename(&old_bak_path, &bak_file_path).await;
        } else {
            let _ = tokio::fs::remove_file(&old_bak_path).await;
        }
    }

    state_file_path
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_sync_lib::FileState;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_state_repository_roundtrip_and_recovery() {
        let temp_dir = tempdir().unwrap();
        let state_path = temp_dir.path().join(".sync_state_mock.bin");
        let repo = SyncStateRepository::new();

        // 1. Initial load should return default empty state
        let empty = repo.load(&state_path).await;
        assert!(empty.files.is_empty());

        // 2. Save state
        let mut state = SyncState::default();
        state.files.insert(
            "folder/data.bin".to_string(),
            FileState {
                size: 4096,
                local_modified: std::time::SystemTime::now(),
                remote_modified: std::time::SystemTime::now(),
                is_dir: Some(false),
                checksum: Some("md5checksum".to_string()),
                permissions: Some(0o644),
            },
        );

        repo.save(&state, &state_path).await.unwrap();

        // 3. Load state back
        let loaded = repo.load(&state_path).await;
        assert_eq!(loaded, state);

        // 4. Save updated state (creates .bak)
        let mut updated = state.clone();
        updated.files.insert(
            "folder/data2.bin".to_string(),
            FileState {
                size: 8192,
                local_modified: std::time::SystemTime::now(),
                remote_modified: std::time::SystemTime::now(),
                is_dir: Some(false),
                checksum: Some("md5checksum2".to_string()),
                permissions: Some(0o644),
            },
        );
        repo.save(&updated, &state_path).await.unwrap();

        let loaded_updated = repo.load(&state_path).await;
        assert_eq!(loaded_updated, updated);

        // 5. Corrupt primary file: loading should recover from .bak seamlessly
        let mut corrupted_bytes = tokio::fs::read(&state_path).await.unwrap();
        let len = corrupted_bytes.len();
        corrupted_bytes[len - 1] ^= 0xFF;
        tokio::fs::write(&state_path, corrupted_bytes)
            .await
            .unwrap();

        let recovered = repo.load(&state_path).await;
        assert_eq!(recovered, state);
    }
}
