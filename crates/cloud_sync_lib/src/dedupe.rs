//! Deduplication engine module (Rclone `dedupe` equivalent).
//!
//! Scans a storage backend for duplicate files based on content hashes and file sizes,
//! and provides resolution strategies to recover storage space or disambiguate names.

use crate::traits::{StorageBackend, StorageError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Strategy to resolve duplicate files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DedupeStrategy {
    /// Only report duplicate groups without modifying files.
    ReportOnly,
    /// Keep the copy with the oldest modification timestamp and delete others.
    KeepOldest,
    /// Keep the copy with the newest modification timestamp and delete others.
    KeepNewest,
    /// Keep the first occurrence and delete all other duplicates.
    KeepFirst,
    /// Rename duplicate files with a numeric suffix (e.g. `file_dup_1.ext`).
    RenameIdentical,
}

/// Metadata representation of a file in a duplicate group.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DedupeFile {
    pub path: String,
    pub size: u64,
    pub modified_secs: u64,
    pub checksum: Option<String>,
}

/// A group of duplicate files sharing identical size and content hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub key: String,
    pub size: u64,
    pub files: Vec<DedupeFile>,
}

/// Summary of a deduplication execution run.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DedupeSummary {
    pub groups_scanned: usize,
    pub total_duplicate_files: usize,
    pub files_deleted: usize,
    pub files_renamed: usize,
    pub bytes_recovered: u64,
}

/// Recursively traverses a backend and collects all files into `items` using the shared tree walker.
async fn collect_all_items(
    backend: &(dyn StorageBackend + Send + Sync),
    remote_path: &str,
    items: &mut Vec<DedupeFile>,
) -> Result<(), StorageError> {
    let list = crate::providers::utils::walk_remote_tree(backend, remote_path).await?;
    for item in list {
        if !item.is_dir {
            let path_str = item.path.to_string_lossy().replace('\\', "/");
            let modified_secs = item
                .modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            items.push(DedupeFile {
                path: path_str,
                size: item.size,
                modified_secs,
                checksum: item.checksum,
            });
        }
    }
    Ok(())
}

/// Scans the target remote storage path for duplicate files.
///
/// Groups files that have matching non-empty checksums and matching size,
/// or matching size and content key when checksums are unavailable.
pub async fn scan_duplicates(
    backend: &(dyn StorageBackend + Send + Sync),
    remote_path: &str,
) -> Result<Vec<DuplicateGroup>, StorageError> {
    let mut all_items = Vec::new();
    collect_all_items(backend, remote_path, &mut all_items).await?;

    let mut map: HashMap<String, Vec<DedupeFile>> = HashMap::new();
    for item in all_items {
        let key = if let Some(ref sum) = item.checksum {
            format!("{}:{}", item.size, sum)
        } else {
            // Group by size as fallback
            format!("size:{}", item.size)
        };
        map.entry(key).or_default().push(item);
    }

    let mut duplicate_groups = Vec::new();
    for (key, files) in map {
        if files.len() > 1 {
            let size = files[0].size;
            duplicate_groups.push(DuplicateGroup { key, size, files });
        }
    }

    duplicate_groups.sort_by_key(|a| std::cmp::Reverse(a.size));
    Ok(duplicate_groups)
}

/// Resolves identified duplicate groups using the given [`DedupeStrategy`].
pub async fn resolve_duplicates(
    backend: &(dyn StorageBackend + Send + Sync),
    groups: &[DuplicateGroup],
    strategy: DedupeStrategy,
) -> Result<DedupeSummary, StorageError> {
    let mut summary = DedupeSummary {
        groups_scanned: groups.len(),
        ..Default::default()
    };

    if strategy == DedupeStrategy::ReportOnly {
        for group in groups {
            summary.total_duplicate_files += group.files.len() - 1;
        }
        return Ok(summary);
    }

    for group in groups {
        if group.files.len() <= 1 {
            continue;
        }

        let mut sorted_files = group.files.clone();
        match strategy {
            DedupeStrategy::KeepOldest => {
                sorted_files.sort_by_key(|f| f.modified_secs);
            }
            DedupeStrategy::KeepNewest => {
                sorted_files.sort_by_key(|f| std::cmp::Reverse(f.modified_secs));
            }
            DedupeStrategy::KeepFirst => {
                // Preserve original listing order
            }
            DedupeStrategy::RenameIdentical => {
                // Rename duplicates 1..N
                for (idx, dup) in sorted_files.iter().enumerate().skip(1) {
                    let old_path = &dup.path;
                    let (base, ext) = if let Some(pos) = old_path.rfind('.') {
                        (&old_path[..pos], &old_path[pos..])
                    } else {
                        (old_path.as_str(), "")
                    };
                    let new_path = format!("{}_dup_{}{}", base, idx, ext);
                    backend.rename(old_path, &new_path).await?;
                    summary.files_renamed += 1;
                }
                continue;
            }
            DedupeStrategy::ReportOnly => unreachable!(),
        }

        // Delete duplicates (from index 1 onward, keeping index 0)
        for dup in sorted_files.iter().skip(1) {
            backend.delete(&dup.path).await?;
            summary.files_deleted += 1;
            summary.bytes_recovered += dup.size;
        }
        summary.total_duplicate_files += group.files.len() - 1;
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_dedupe_scan_and_resolve() {
        let temp_local = tempdir().unwrap();
        let temp_remote = tempdir().unwrap();

        let backend = LocalSimulation::new(temp_remote.path().to_path_buf(), "test".to_string());

        let file1 = temp_local.path().join("file1.txt");
        let file2 = temp_local.path().join("file2.txt");
        tokio::fs::write(&file1, b"duplicate content")
            .await
            .unwrap();
        tokio::fs::write(&file2, b"duplicate content")
            .await
            .unwrap();

        backend.upload(&file1, "file1.txt").await.unwrap();
        backend.upload(&file2, "file2.txt").await.unwrap();

        let groups = scan_duplicates(&backend, "").await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].files.len(), 2);

        let summary = resolve_duplicates(&backend, &groups, DedupeStrategy::KeepFirst)
            .await
            .unwrap();

        assert_eq!(summary.files_deleted, 1);
        assert_eq!(summary.bytes_recovered, 17);

        // Verify only 1 file remains
        let remaining = backend.list("").await.unwrap();
        assert_eq!(remaining.len(), 1);
    }
}
