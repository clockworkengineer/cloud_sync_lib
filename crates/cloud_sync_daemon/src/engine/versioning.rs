//! File versioning and trash can retention engine (Syncthing/Rclone-style).
//!
//! Archives modified or deleted files into a versioning trash directory before
//! destructive overwrites, preventing accidental data loss and allowing rollbacks.

use chrono::Utc;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct VersioningConfig {
    pub enabled: bool,
    pub trash_dir: PathBuf,
    pub retention_days: u32,
    pub max_versions_per_file: u32,
}

impl Default for VersioningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            trash_dir: PathBuf::from("sync_state").join("trash"),
            retention_days: 30,
            max_versions_per_file: 5,
        }
    }
}

pub struct TrashManager {
    config: VersioningConfig,
}

impl TrashManager {
    pub fn new(config: VersioningConfig) -> Self {
        Self { config }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Archives a file into the versioning trash directory before an overwrite or deletion.
    pub async fn archive_file(
        &self,
        source_path: &Path,
    ) -> Result<Option<PathBuf>, std::io::Error> {
        if !self.config.enabled || !source_path.exists() {
            return Ok(None);
        }

        tokio::fs::create_dir_all(&self.config.trash_dir).await?;

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let filename = source_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed_file".to_string());

        let trash_filename = format!("{}_{}", timestamp, filename);
        let trash_dest = self.config.trash_dir.join(trash_filename);

        info!(
            "Archiving '{}' to trash directory at '{}'",
            source_path.display(),
            trash_dest.display()
        );

        tokio::fs::copy(source_path, &trash_dest).await?;
        Ok(Some(trash_dest))
    }

    /// Purges archived versions older than the configured retention period.
    pub async fn cleanup_expired(&self) -> Result<usize, std::io::Error> {
        if !self.config.trash_dir.exists() {
            return Ok(0);
        }

        let mut purged = 0;
        let mut entries = tokio::fs::read_dir(&self.config.trash_dir).await?;
        let cutoff = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(
                (self.config.retention_days as u64) * 86400,
            ))
            .unwrap_or(std::time::SystemTime::now());

        while let Some(entry) = entries.next_entry().await? {
            if let Ok(meta) = entry.metadata().await {
                if let Ok(modified) = meta.modified() {
                    if modified < cutoff {
                        if let Err(e) = tokio::fs::remove_file(entry.path()).await {
                            warn!(
                                "Failed to purge expired trash file {:?}: {}",
                                entry.path(),
                                e
                            );
                        } else {
                            purged += 1;
                        }
                    }
                }
            }
        }

        Ok(purged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_trash_manager_archive() {
        let temp_dir = tempdir().unwrap();
        let trash_dir = temp_dir.path().join("trash");
        let source_file = temp_dir.path().join("document.txt");
        tokio::fs::write(&source_file, "original version")
            .await
            .unwrap();

        let mgr = TrashManager::new(VersioningConfig {
            enabled: true,
            trash_dir: trash_dir.clone(),
            retention_days: 7,
            max_versions_per_file: 3,
        });

        let archived = mgr.archive_file(&source_file).await.unwrap();
        assert!(archived.is_some());
        let archived_path = archived.unwrap();
        assert!(archived_path.exists());
        let content = tokio::fs::read_to_string(archived_path).await.unwrap();
        assert_eq!(content, "original version");
    }
}
