//! File versioning and trash can retention engine (Syncthing/Rclone-style).
//!
//! Archives modified or deleted files into a versioning trash directory before
//! destructive overwrites, enforces per-file version limits, lists archived
//! versions, and supports point-in-time file restoration.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Metadata record representing an archived file version in the trash directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchivedVersion {
    /// Archive filename identifier (e.g. `20261121_120000_document.txt`).
    pub id: String,
    /// Original filename (e.g. `document.txt`).
    pub original_filename: String,
    /// Timestamp prefix (`YYYYMMDD_HHMMSS`).
    pub archived_at: String,
    /// File size in bytes.
    pub size: u64,
    /// Full local path inside the trash directory.
    pub trash_path: PathBuf,
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

    pub fn trash_dir(&self) -> &Path {
        &self.config.trash_dir
    }

    /// Parses an archive filename (`YYYYMMDD_HHMMSS_NNN_<original>` or `YYYYMMDD_HHMMSS_<original>`)
    /// into `(archived_at, original_filename)`.
    fn parse_archive_name(name: &str) -> (String, String) {
        let bytes = name.as_bytes();
        // Format 1: 19 chars timestamp with sequence `YYYYMMDD_HHMMSS_NNN_<original>`
        if bytes.len() > 20 && bytes[8] == b'_' && bytes[15] == b'_' && bytes[19] == b'_' {
            let ts = &name[..19];
            let orig = &name[20..];
            if ts.chars().all(|c| c.is_ascii_digit() || c == '_') {
                return (ts.to_string(), orig.to_string());
            }
        }
        // Format 2: 15 chars timestamp `YYYYMMDD_HHMMSS_<original>`
        if bytes.len() > 16 && bytes[8] == b'_' && bytes[15] == b'_' {
            let ts = &name[..15];
            let orig = &name[16..];
            if ts.chars().all(|c| c.is_ascii_digit() || c == '_') {
                return (ts.to_string(), orig.to_string());
            }
        }
        ("unknown".to_string(), name.to_string())
    }

    /// Archives a file into the versioning trash directory before an overwrite or deletion,
    /// and enforces `max_versions_per_file` by pruning the oldest archived copies.
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

        let mut counter = 0u32;
        let mut trash_filename = format!("{}_{:03}_{}", timestamp, counter, filename);
        let mut trash_dest = self.config.trash_dir.join(&trash_filename);

        // Disambiguate if multiple versions are archived within the same second
        while trash_dest.exists() {
            counter += 1;
            trash_filename = format!("{}_{:03}_{}", timestamp, counter, filename);
            trash_dest = self.config.trash_dir.join(&trash_filename);
        }

        info!(
            "Archiving '{}' to trash directory at '{}'",
            source_path.display(),
            trash_dest.display()
        );

        tokio::fs::copy(source_path, &trash_dest).await?;

        if self.config.max_versions_per_file > 0 {
            let _ = self.enforce_max_versions(&filename).await;
        }

        Ok(Some(trash_dest))
    }

    /// Lists all archived file versions currently stored in the trash directory, newest first.
    pub async fn list_versions(&self) -> Result<Vec<ArchivedVersion>, std::io::Error> {
        if !self.config.trash_dir.exists() {
            return Ok(Vec::new());
        }

        let mut versions = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.config.trash_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            if let Ok(meta) = entry.metadata().await {
                if meta.is_file() {
                    let id = entry.file_name().to_string_lossy().to_string();
                    let (archived_at, original_filename) = Self::parse_archive_name(&id);
                    versions.push(ArchivedVersion {
                        id,
                        original_filename,
                        archived_at,
                        size: meta.len(),
                        trash_path: entry.path(),
                    });
                }
            }
        }

        versions.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(versions)
    }

    /// Restores an archived version identified by `archive_id` into `target_dir`
    /// (using its original filename or `custom_rel_path` if specified).
    pub async fn restore_version(
        &self,
        archive_id: &str,
        target_dir: &Path,
        custom_rel_path: Option<&str>,
    ) -> Result<PathBuf, std::io::Error> {
        // Prevent path traversal in archive_id
        let clean_id = Path::new(archive_id)
            .file_name()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid archive_id")
            })?
            .to_string_lossy()
            .to_string();

        let archive_path = self.config.trash_dir.join(&clean_id);
        if !archive_path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Archived version '{}' not found", clean_id),
            ));
        }

        let (_, orig_name) = Self::parse_archive_name(&clean_id);
        let rel = custom_rel_path.unwrap_or(&orig_name);
        let dest_path = target_dir.join(rel);

        if let Some(parent) = dest_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::copy(&archive_path, &dest_path).await?;
        info!(
            "Restored archived version '{}' to '{}'",
            clean_id,
            dest_path.display()
        );
        Ok(dest_path)
    }

    /// Enforces `max_versions_per_file` for a given `original_filename`, deleting oldest excess versions.
    pub async fn enforce_max_versions(
        &self,
        original_filename: &str,
    ) -> Result<usize, std::io::Error> {
        if self.config.max_versions_per_file == 0 || !self.config.trash_dir.exists() {
            return Ok(0);
        }

        let all = self.list_versions().await?;
        let matching: Vec<ArchivedVersion> = all
            .into_iter()
            .filter(|v| v.original_filename == original_filename)
            .collect();

        let limit = self.config.max_versions_per_file as usize;
        let mut pruned = 0;
        if matching.len() > limit {
            for old in matching.into_iter().skip(limit) {
                if tokio::fs::remove_file(&old.trash_path).await.is_ok() {
                    pruned += 1;
                }
            }
        }
        Ok(pruned)
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
            max_versions_per_file: 2,
        });

        let archived = mgr.archive_file(&source_file).await.unwrap();
        assert!(archived.is_some());
        let archived_path = archived.unwrap();
        assert!(archived_path.exists());
        let content = tokio::fs::read_to_string(&archived_path).await.unwrap();
        assert_eq!(content, "original version");

        // Archive 2 more versions and verify max_versions_per_file (2) is enforced
        tokio::fs::write(&source_file, "version 2").await.unwrap();
        mgr.archive_file(&source_file).await.unwrap();

        tokio::fs::write(&source_file, "version 3").await.unwrap();
        mgr.archive_file(&source_file).await.unwrap();

        let listed = mgr.list_versions().await.unwrap();
        assert_eq!(listed.len(), 2);

        // Restore the latest version into a restore folder
        let restore_dir = temp_dir.path().join("restored");
        let restored_path = mgr
            .restore_version(&listed[0].id, &restore_dir, None)
            .await
            .unwrap();
        let restored_text = tokio::fs::read_to_string(&restored_path).await.unwrap();
        assert_eq!(restored_text, "version 3");
    }
}
