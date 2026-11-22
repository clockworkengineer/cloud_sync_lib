//! Direct Cloud-to-Cloud Transfer & Migration Engine (Rclone `copy` / `sync` equivalent).
//!
//! Enables migrating or replicating files directly between any two [`StorageBackend`]
//! implementations (e.g., Google Drive to Amazon S3, Dropbox to Nextcloud, SFTP to B2)
//! without maintaining a permanent local mirror directory.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

/// Configuration options for a direct cloud-to-cloud transfer operation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CloudTransferOptions {
    /// If true, deletes files on the destination that do not exist on the source (`rclone sync` behavior).
    /// If false, only copies new/updated files (`rclone copy` behavior).
    pub delete_extraneous: bool,
    /// Skip transferring files whose size (and checksum, if present on both) already matches on destination.
    pub skip_matching: bool,
    /// Simulate the transfer and report planned operations without modifying the destination.
    pub dry_run: bool,
}

impl Default for CloudTransferOptions {
    fn default() -> Self {
        Self {
            delete_extraneous: false,
            skip_matching: true,
            dry_run: false,
        }
    }
}

/// Summary report of a completed cloud-to-cloud transfer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudTransferReport {
    pub source_backend: String,
    pub destination_backend: String,
    pub files_transferred: usize,
    pub files_skipped: usize,
    pub files_deleted_on_dest: usize,
    pub bytes_transferred: u64,
    pub transferred_paths: Vec<String>,
    pub deleted_paths: Vec<String>,
    pub duration_ms: u64,
}

async fn collect_backend_tree(
    backend: &(dyn StorageBackend + Send + Sync),
    base_prefix: &str,
    current_path: &str,
    out: &mut HashMap<String, StorageItem>,
) -> Result<(), StorageError> {
    let items = crate::providers::utils::walk_remote_tree(backend, current_path).await?;
    let clean_base = base_prefix.trim_matches('/');

    for item in items {
        if !item.is_dir {
            let item_path = item.path.to_string_lossy().replace('\\', "/");
            let trimmed = item_path.trim_start_matches('/');
            let rel = if !clean_base.is_empty() && trimmed.starts_with(clean_base) {
                trimmed[clean_base.len()..]
                    .trim_start_matches('/')
                    .to_string()
            } else {
                trimmed.to_string()
            };
            out.insert(rel, item);
        }
    }
    Ok(())
}

/// Transfers or synchronizes files directly from `source` backend to `dest` backend.
pub async fn transfer_between_backends(
    source: &(dyn StorageBackend + Send + Sync),
    dest: &(dyn StorageBackend + Send + Sync),
    source_prefix: &str,
    dest_prefix: &str,
    options: CloudTransferOptions,
) -> Result<CloudTransferReport, StorageError> {
    let start = Instant::now();
    let clean_src = source_prefix.trim_matches('/');
    let clean_dst = dest_prefix.trim_matches('/');

    let mut src_files = HashMap::new();
    collect_backend_tree(source, clean_src, clean_src, &mut src_files).await?;

    let mut dst_files = HashMap::new();
    let _ = collect_backend_tree(dest, clean_dst, clean_dst, &mut dst_files).await;

    let mut report = CloudTransferReport {
        source_backend: source.name().to_string(),
        destination_backend: dest.name().to_string(),
        ..Default::default()
    };

    let mut src_keys: Vec<String> = src_files.keys().cloned().collect();
    src_keys.sort();

    for rel_path in src_keys {
        let src_item = &src_files[&rel_path];
        if options.skip_matching {
            if let Some(dst_item) = dst_files.get(&rel_path) {
                let checksum_matches = match (&src_item.checksum, &dst_item.checksum) {
                    (Some(s), Some(d)) => s == d,
                    _ => true,
                };
                if src_item.size == dst_item.size && checksum_matches {
                    report.files_skipped += 1;
                    continue;
                }
            }
        }

        let src_remote = if clean_src.is_empty() {
            rel_path.clone()
        } else {
            format!("{}/{}", clean_src, rel_path)
        };
        let dst_remote = if clean_dst.is_empty() {
            rel_path.clone()
        } else {
            format!("{}/{}", clean_dst, rel_path)
        };

        if !options.dry_run {
            let temp_guard = crate::providers::utils::TempFileGuard::new("c2c_stage");
            source.download(&src_remote, temp_guard.path()).await?;
            dest.upload(temp_guard.path(), &dst_remote).await?;
        }

        report.files_transferred += 1;
        report.bytes_transferred += src_item.size;
        report.transferred_paths.push(rel_path);
    }

    if options.delete_extraneous {
        let mut dst_keys: Vec<String> = dst_files.keys().cloned().collect();
        dst_keys.sort();

        for rel_path in dst_keys {
            if !src_files.contains_key(&rel_path) {
                let dst_remote = if clean_dst.is_empty() {
                    rel_path.clone()
                } else {
                    format!("{}/{}", clean_dst, rel_path)
                };
                if !options.dry_run {
                    dest.delete(&dst_remote).await?;
                }
                report.files_deleted_on_dest += 1;
                report.deleted_paths.push(rel_path);
            }
        }
    }

    report.duration_ms = start.elapsed().as_millis() as u64;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_cloud_to_cloud_transfer_and_sync() {
        let dir_src = tempdir().unwrap();
        let dir_dst = tempdir().unwrap();
        let dir_stage = tempdir().unwrap();

        let src_backend = LocalSimulation::new(dir_src.path().to_path_buf(), "GoogleDrive".into());
        let dst_backend = LocalSimulation::new(dir_dst.path().to_path_buf(), "AmazonS3".into());

        let f1 = dir_stage.path().join("a.txt");
        let f2 = dir_stage.path().join("b.txt");
        let f_stale = dir_stage.path().join("stale.txt");
        tokio::fs::write(&f1, b"alpha data").await.unwrap();
        tokio::fs::write(&f2, b"bravo data").await.unwrap();
        tokio::fs::write(&f_stale, b"old file").await.unwrap();

        src_backend.upload(&f1, "docs/a.txt").await.unwrap();
        src_backend.upload(&f2, "docs/b.txt").await.unwrap();
        dst_backend.upload(&f_stale, "stale.txt").await.unwrap();

        // 1. Copy mode (does not delete stale.txt)
        let copy_report = transfer_between_backends(
            &src_backend,
            &dst_backend,
            "",
            "",
            CloudTransferOptions {
                delete_extraneous: false,
                skip_matching: true,
                dry_run: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(copy_report.files_transferred, 2);
        assert_eq!(copy_report.files_deleted_on_dest, 0);
        assert_eq!(copy_report.bytes_transferred, 20);

        // 2. Sync mode (skips matching a.txt and b.txt, deletes stale.txt)
        let sync_report = transfer_between_backends(
            &src_backend,
            &dst_backend,
            "",
            "",
            CloudTransferOptions {
                delete_extraneous: true,
                skip_matching: true,
                dry_run: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(sync_report.files_transferred, 0);
        assert_eq!(sync_report.files_skipped, 2);
        assert_eq!(sync_report.files_deleted_on_dest, 1);
        assert_eq!(sync_report.deleted_paths, vec!["stale.txt"]);
    }
}
