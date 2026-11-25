//! Content-Addressable Incremental Cloud Snapshot Engine (Kopia / Restic / BorgBackup equivalent).
//!
//! Creates immutable, deduplicated point-in-time snapshots of a local directory on any
//! [`StorageBackend`]:
//! - Content blobs are stored once by SHA-256 digest under `_snapshots/blobs/<sha256>`.
//! - Point-in-time manifests are stored under `_snapshots/manifests/<id>.json`.
//! - Supports incremental snapshot creation, listing, SHA-256-verified restoration, and
//!   retention pruning with unreferenced blob garbage collection (`prune_snapshots`).

use crate::providers::utils::TempFileGuard;
use crate::traits::{StorageBackend, StorageError};
use cloud_sync_core::NormalizedPath;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const SNAPSHOT_ROOT: &str = "_snapshots";
const BLOBS_DIR: &str = "_snapshots/blobs";
const MANIFESTS_DIR: &str = "_snapshots/manifests";

/// A single file entry recorded inside a point-in-time [`SnapshotManifest`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotFileEntry {
    pub relative_path: String,
    pub size: u64,
    pub modified_secs: u64,
    pub sha256: String,
}

/// Immutable point-in-time snapshot manifest stored on the remote cloud backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotManifest {
    pub id: String,
    pub label: String,
    pub created_at_secs: u64,
    pub total_files: usize,
    pub total_bytes: u64,
    pub new_blobs_uploaded: usize,
    pub deduped_blobs_reused: usize,
    pub bytes_uploaded: u64,
    pub bytes_saved_by_dedup: u64,
    pub files: Vec<SnapshotFileEntry>,
}

/// Summary report returned after restoring a snapshot to a local directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotRestoreReport {
    pub snapshot_id: String,
    pub files_restored: usize,
    pub bytes_restored: u64,
}

/// Summary report returned after pruning old snapshots and garbage-collecting blobs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotPruneReport {
    pub snapshots_deleted: usize,
    pub orphan_blobs_deleted: usize,
}

async fn walk_local_files(
    root: &Path,
    current: &Path,
    entries: &mut Vec<SnapshotFileEntry>,
) -> Result<(), StorageError> {
    let mut dir = tokio::fs::read_dir(current).await?;
    while let Some(entry) = dir.next_entry().await? {
        let meta = entry.metadata().await?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with(".sync_state") || file_name == "sync_state" {
            continue;
        }
        if meta.is_dir() {
            Box::pin(walk_local_files(root, &path, entries)).await?;
        } else if meta.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                // Stream SHA-256 in O(1) memory instead of loading entire file into RAM
                let sha256 = crate::checksum::compute_sha256(&path).await?;
                let modified_secs = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                entries.push(SnapshotFileEntry {
                    relative_path: rel_str,
                    size: meta.len(),
                    modified_secs,
                    sha256,
                });
            }
        }
    }
    Ok(())
}

/// Creates a new content-addressable deduplicated point-in-time snapshot of `local_root` on `backend`.
pub async fn create_snapshot(
    backend: &(dyn StorageBackend + Send + Sync),
    local_root: &Path,
    label: &str,
) -> Result<SnapshotManifest, StorageError> {
    let _ = backend.create_folder(SNAPSHOT_ROOT).await;
    let _ = backend.create_folder(BLOBS_DIR).await;
    let _ = backend.create_folder(MANIFESTS_DIR).await;

    // Discover already-stored content blobs on the remote backend
    let mut existing_blobs = HashSet::new();
    if let Ok(blob_items) = backend.list(BLOBS_DIR).await {
        for item in blob_items {
            if let Some(name) = item.path.file_name().and_then(|n| n.to_str()) {
                existing_blobs.insert(name.to_string());
            }
        }
    }

    let mut files = Vec::new();
    if local_root.exists() {
        walk_local_files(local_root, local_root, &mut files).await?;
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    let mut total_bytes = 0u64;
    let mut new_blobs_uploaded = 0usize;
    let mut deduped_blobs_reused = 0usize;
    let mut bytes_uploaded = 0u64;
    let mut bytes_saved_by_dedup = 0u64;

    for entry in &files {
        total_bytes += entry.size;
        if existing_blobs.contains(&entry.sha256) {
            deduped_blobs_reused += 1;
            bytes_saved_by_dedup += entry.size;
        } else {
            let local_file = local_root.join(&entry.relative_path);
            let blob_remote = format!("{}/{}", BLOBS_DIR, entry.sha256);
            backend.upload(&local_file, &blob_remote).await?;
            existing_blobs.insert(entry.sha256.clone());
            new_blobs_uploaded += 1;
            bytes_uploaded += entry.size;
        }
    }

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let now_secs = now_ms / 1000;
    let clean_label: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let id = if clean_label.is_empty() {
        format!("snap_{}", now_ms)
    } else {
        format!("snap_{}_{}", now_ms, clean_label)
    };

    let manifest = SnapshotManifest {
        id: id.clone(),
        label: label.to_string(),
        created_at_secs: now_secs,
        total_files: files.len(),
        total_bytes,
        new_blobs_uploaded,
        deduped_blobs_reused,
        bytes_uploaded,
        bytes_saved_by_dedup,
        files,
    };

    let mf_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| StorageError::Provider {
        message: format!("Failed to serialize snapshot manifest: {}", e),
        status: None,
    })?;
    let temp_mf = TempFileGuard::new("snap_mf");
    tokio::fs::write(temp_mf.path(), &mf_bytes).await?;
    let mf_remote = format!("{}/{}.json", MANIFESTS_DIR, id);
    backend.upload(temp_mf.path(), &mf_remote).await?;

    Ok(manifest)
}

/// Lists all point-in-time [`SnapshotManifest`] records stored on `backend`, newest first.
pub async fn list_snapshots(
    backend: &(dyn StorageBackend + Send + Sync),
) -> Result<Vec<SnapshotManifest>, StorageError> {
    let items = match backend.list(MANIFESTS_DIR).await {
        Ok(list) => list,
        Err(StorageError::NotFound(_)) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut manifests = Vec::new();
    for item in items {
        if item.is_dir {
            continue;
        }
        let remote_path = item.path.to_string_lossy().replace('\\', "/");
        if !remote_path.ends_with(".json") {
            continue;
        }
        let temp_mf = TempFileGuard::new("snap_list");
        if backend.download(&remote_path, temp_mf.path()).await.is_ok() {
            if let Ok(bytes) = tokio::fs::read(temp_mf.path()).await {
                if let Ok(mf) = serde_json::from_slice::<SnapshotManifest>(&bytes) {
                    manifests.push(mf);
                }
            }
        }
    }

    manifests.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(manifests)
}

/// Restores the specified `snapshot_id` from `backend` into `target_dir`, verifying SHA-256
/// integrity of every restored file.
pub async fn restore_snapshot(
    backend: &(dyn StorageBackend + Send + Sync),
    snapshot_id: &str,
    target_dir: &Path,
) -> Result<SnapshotRestoreReport, StorageError> {
    let clean_id = snapshot_id.trim_end_matches(".json");
    let mf_remote = format!("{}/{}.json", MANIFESTS_DIR, clean_id);
    let temp_mf = TempFileGuard::new("snap_rest_mf");
    backend.download(&mf_remote, temp_mf.path()).await?;
    let mf_bytes = tokio::fs::read(temp_mf.path()).await?;

    let manifest: SnapshotManifest =
        serde_json::from_slice(&mf_bytes).map_err(|e| StorageError::Provider {
            message: format!("Invalid snapshot manifest '{}': {}", clean_id, e),
            status: None,
        })?;

    tokio::fs::create_dir_all(target_dir).await?;
    let mut bytes_restored = 0u64;

    for entry in &manifest.files {
        // Enforce path traversal defense on relative_path
        let norm_path = NormalizedPath::new(&entry.relative_path).map_err(|e| {
            StorageError::InvalidPath(format!(
                "Malicious or invalid snapshot relative path '{}': {}",
                entry.relative_path, e
            ))
        })?;

        // Enforce valid SHA-256 hexadecimal hash formatting
        if entry.sha256.len() != 64 || !entry.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(StorageError::InvalidData(format!(
                "Invalid SHA-256 digest in snapshot manifest for '{}': {}",
                entry.relative_path, entry.sha256
            )));
        }

        let blob_remote = format!("{}/{}", BLOBS_DIR, entry.sha256);
        let dest_path = target_dir.join(norm_path.as_str());

        // Stage download into temporary file and verify SHA-256 BEFORE writing to destination
        let temp_blob = TempFileGuard::new("snap_blob_dl");
        backend.download(&blob_remote, temp_blob.path()).await?;
        let actual_sha = crate::checksum::compute_sha256(temp_blob.path()).await?;
        if actual_sha != entry.sha256 {
            return Err(StorageError::Provider {
                message: format!(
                    "Snapshot integrity mismatch for '{}': expected {}, got {}",
                    entry.relative_path, entry.sha256, actual_sha
                ),
                status: None,
            });
        }

        if let Some(parent) = dest_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(temp_blob.path(), &dest_path).await?;
        bytes_restored += entry.size;
    }

    Ok(SnapshotRestoreReport {
        snapshot_id: manifest.id,
        files_restored: manifest.files.len(),
        bytes_restored,
    })
}

/// Retains the newest `keep_last_n` snapshots, deletes older snapshot manifests, and
/// garbage-collects any unreferenced content blobs on the remote backend.
pub async fn prune_snapshots(
    backend: &(dyn StorageBackend + Send + Sync),
    keep_last_n: usize,
) -> Result<SnapshotPruneReport, StorageError> {
    let all = list_snapshots(backend).await?;
    if all.len() <= keep_last_n {
        return Ok(SnapshotPruneReport {
            snapshots_deleted: 0,
            orphan_blobs_deleted: 0,
        });
    }

    let (retained, expired) = all.split_at(keep_last_n);
    let mut referenced_blobs = HashSet::new();
    for snap in retained {
        for f in &snap.files {
            referenced_blobs.insert(f.sha256.clone());
        }
    }

    let mut snapshots_deleted = 0usize;
    for snap in expired {
        let mf_remote = format!("{}/{}.json", MANIFESTS_DIR, snap.id);
        if backend.delete(&mf_remote).await.is_ok() {
            snapshots_deleted += 1;
        }
    }

    let mut orphan_blobs_deleted = 0usize;
    if let Ok(blob_items) = backend.list(BLOBS_DIR).await {
        for item in blob_items {
            if let Some(sha) = item.path.file_name().and_then(|n| n.to_str()) {
                if !referenced_blobs.contains(sha) {
                    let blob_remote = format!("{}/{}", BLOBS_DIR, sha);
                    if backend.delete(&blob_remote).await.is_ok() {
                        orphan_blobs_deleted += 1;
                    }
                }
            }
        }
    }

    Ok(SnapshotPruneReport {
        snapshots_deleted,
        orphan_blobs_deleted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_snapshot_deduplication_restore_and_prune() {
        let local_dir = tempdir().unwrap();
        let remote_dir = tempdir().unwrap();
        let restore_dir = tempdir().unwrap();

        let backend = LocalSimulation::new(remote_dir.path().to_path_buf(), "SnapCloud".into());

        // Snapshot 1: two files
        tokio::fs::write(local_dir.path().join("a.txt"), b"alpha content")
            .await
            .unwrap();
        tokio::fs::write(local_dir.path().join("b.txt"), b"bravo content")
            .await
            .unwrap();

        let snap1 = create_snapshot(&backend, local_dir.path(), "daily-1")
            .await
            .unwrap();
        assert_eq!(snap1.total_files, 2);
        assert_eq!(snap1.new_blobs_uploaded, 2);
        assert_eq!(snap1.deduped_blobs_reused, 0);

        // Ensure monotonic millisecond separation for Snapshot 2
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        // Snapshot 2: a.txt unchanged (reused!), c.txt added
        tokio::fs::write(local_dir.path().join("c.txt"), b"charlie content")
            .await
            .unwrap();

        let snap2 = create_snapshot(&backend, local_dir.path(), "daily-2")
            .await
            .unwrap();
        assert_eq!(snap2.total_files, 3);
        assert_eq!(snap2.new_blobs_uploaded, 1);
        assert_eq!(snap2.deduped_blobs_reused, 2);
        assert!(snap2.bytes_saved_by_dedup > 0);

        // List snapshots -> 2 snapshots, newest first
        let snaps = list_snapshots(&backend).await.unwrap();
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[0].id, snap2.id);

        // Restore Snapshot 1 into restore_dir (should have only a.txt and b.txt)
        let rep = restore_snapshot(&backend, &snap1.id, restore_dir.path())
            .await
            .unwrap();
        assert_eq!(rep.files_restored, 2);
        assert!(restore_dir.path().join("a.txt").exists());
        assert!(!restore_dir.path().join("c.txt").exists());

        // Prune keeping only 1 latest snapshot (snap2)
        let prune_rep = prune_snapshots(&backend, 1).await.unwrap();
        assert_eq!(prune_rep.snapshots_deleted, 1);
        assert_eq!(list_snapshots(&backend).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_snapshot_path_traversal_defense() {
        let temp_dir = tempdir().unwrap();
        let restore_dir = tempdir().unwrap();
        let backend = LocalSimulation::new(temp_dir.path().to_path_buf(), "SnapCloud".into());

        // Construct a fraudulent manifest containing a directory traversal path
        let bad_manifest = SnapshotManifest {
            id: "fake_snap_01".to_string(),
            label: "malicious".to_string(),
            created_at_secs: 1000,
            total_files: 1,
            total_bytes: 10,
            new_blobs_uploaded: 1,
            deduped_blobs_reused: 0,
            bytes_uploaded: 10,
            bytes_saved_by_dedup: 0,
            files: vec![SnapshotFileEntry {
                relative_path: "../../evil.sh".to_string(),
                size: 10,
                modified_secs: 1000,
                sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            }],
        };

        let mf_bytes = serde_json::to_vec_pretty(&bad_manifest).unwrap();
        let mf_path = temp_dir
            .path()
            .join("_snapshots/manifests/fake_snap_01.json");
        tokio::fs::create_dir_all(mf_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&mf_path, mf_bytes).await.unwrap();

        let res = restore_snapshot(&backend, "fake_snap_01", restore_dir.path()).await;
        assert!(
            res.is_err(),
            "Restoring manifest with path traversal must be rejected"
        );
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains("Invalid path") || err_msg.contains("traversal"),
            "Error must specify path traversal rejection, got: {}",
            err_msg
        );
    }
}
