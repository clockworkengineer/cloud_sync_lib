# Operational Tools, Migration & Disaster Recovery Toolkit

This guide provides technical and operational documentation for the advanced management tools included in the `cloud_sync_lib` ecosystem, providing full functional equivalence to industry tools such as **Rclone** (`copy`, `sync`, `dedupe`, `size`, `tree`) and **Kopia/Restic** (content-addressable snapshots, retention pruning).

---

## 1. Direct Cloud-to-Cloud Transfer & Migration (`cloud_transfer`)

Located in [`crates/cloud_sync_lib/src/cloud_transfer.rs`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_lib/src/cloud_transfer.rs), this engine replicates or synchronizes data directly between any two `StorageBackend` instances without requiring a permanent local mirror directory.

### 1.1 Modes of Operation (`CloudTransferOptions`)
- **Copy Mode (`delete_extraneous = false`)**: Copies new and modified files from source to destination. Existing destination files not present on the source remain untouched.
- **Sync Mode (`delete_extraneous = true`)**: Mirror sync ensuring the destination reflects the source. Files that do not exist on the source are deleted on the destination.
- **Smart Matching (`skip_matching = true`)**: Skips transferring files if destination size and checksum (SHA-256 or MD5) already match.
- **Dry-Run Mode (`dry_run = true`)**: Computes and returns the planned transfer report without modifying the destination backend.

### 1.2 Programmatic Usage

```rust,no_run
use cloud_sync_lib::cloud_transfer::{transfer_between_backends, CloudTransferOptions};
use cloud_sync_core::StorageBackend;
use std::sync::Arc;

pub async fn replicate_clouds(
    source: Arc<dyn StorageBackend>,
    dest: Arc<dyn StorageBackend>,
) -> Result<(), cloud_sync_core::StorageError> {
    let options = CloudTransferOptions {
        delete_extraneous: false, // Copy mode
        skip_matching: true,
        dry_run: false,
    };

    let report = transfer_between_backends(
        source.as_ref(),
        dest.as_ref(),
        "source_folder/documents",
        "backup_folder/documents",
        options,
    ).await?;

    println!("Transferred {} files ({} bytes) in {} ms.",
        report.files_transferred, report.bytes_transferred, report.duration_ms);
    println!("Skipped {} existing matching files.", report.files_skipped);

    Ok(())
}
```

---

## 2. Content-Addressable Cloud Snapshots (`SnapshotEngine`)

Located in [`crates/cloud_sync_lib/src/snapshot.rs`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_lib/src/snapshot.rs), `SnapshotEngine` creates immutable, content-addressable point-in-time backups of local directories on any remote storage backend:

```
<Remote Backend Root>/
└── _snapshots/
    ├── blobs/
    │   ├── 3a7bd3e2360a3d29eea436f...   <-- Deduplicated content blob
    │   └── 9f86d081884c7d659a2feaa...   <-- Stored once by SHA-256 hash
    └── manifests/
        ├── 20261124_120000_snapshot1.json
        └── 20261124_130000_snapshot2.json
```

### 2.1 Key Capabilities
- **Global Deduplication**: If multiple files (or snapshots over time) contain identical content, the blob is uploaded only once to `_snapshots/blobs/<sha256>`.
- **Manifest Tracking**: Each snapshot records file paths, sizes, modification timestamps, and content hashes in a JSON manifest.
- **Verified Restoration**: Downloads blobs, verifies SHA-256 integrity on the fly, and restores directory hierarchy.
- **Retention Pruning & Garbage Collection (`prune_snapshots`)**: Automatically prunes older snapshot manifests according to a retention policy and garbage-collects orphaned blobs unreferenced by any surviving manifest.

```rust,no_run
use cloud_sync_lib::snapshot::{create_snapshot, list_snapshots, prune_snapshots};
use cloud_sync_core::StorageBackend;
use std::path::Path;
use std::sync::Arc;

pub async fn run_backup_snapshot(backend: Arc<dyn StorageBackend>) -> Result<(), cloud_sync_core::StorageError> {
    // 1. Create an incremental snapshot on backend
    let manifest = create_snapshot(
        backend.as_ref(),
        Path::new("./watched_folder"),
        "Daily Backup",
    ).await?;

    println!("Snapshot ID: {}, Uploaded: {} bytes, Saved by Dedup: {} bytes",
        manifest.id, manifest.bytes_uploaded, manifest.bytes_saved_by_dedup);

    // 2. List all snapshots
    let all = list_snapshots(backend.as_ref()).await?;
    println!("Total snapshots on backend: {}", all.len());

    // 3. Prune snapshots, keeping only the 5 most recent
    let pruned = prune_snapshots(backend.as_ref(), 5).await?;
    println!("Pruned {} old snapshots, deleted {} orphaned blobs.",
        pruned.snapshots_deleted, pruned.orphan_blobs_deleted);

    Ok(())
}
```

Alternatively, when using `CloudSyncClient`, high-level snapshot helper methods are exposed directly:
```rust,no_run
use cloud_sync_lib::CloudSyncClient;
use std::path::Path;

async fn client_snapshots(client: &CloudSyncClient) -> Result<(), cloud_sync_core::StorageError> {
    let snap = client.create_snapshot(Path::new("./data"), "v1.0").await?;
    let all = client.list_snapshots().await?;
    client.restore_snapshot(&snap.id, Path::new("./restored")).await?;
    client.prune_snapshots(5).await?;
    Ok(())
}
```

---

## 3. Storage Deduplication Engine (`DedupeEngine`)

Located in [`crates/cloud_sync_lib/src/dedupe.rs`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_lib/src/dedupe.rs), the deduplication module scans remote storage backends for duplicate files and applies automated resolution strategies:

### 3.1 Deduplication Strategies (`DedupeStrategy`)
- **`ReportOnly`**: Enumerates duplicate clusters without mutating files.
- **`KeepOldest`**: Retains the copy with the oldest modification time and deletes duplicates.
- **`KeepNewest`**: Retains the copy with the newest modification time and deletes duplicates.
- **`KeepFirst`**: Retains the first encountered instance in traversal order.
- **`RenameIdentical`**: Disambiguates duplicate file names by appending numeric suffixes (`file_dup_1.ext`).

```rust,no_run
use cloud_sync_lib::dedupe::{find_duplicates, resolve_duplicates, DedupeStrategy};
use cloud_sync_core::StorageBackend;
use std::sync::Arc;

pub async fn cleanup_duplicates(backend: Arc<dyn StorageBackend>) -> Result<(), cloud_sync_core::StorageError> {
    // 1. Scan remote path
    let duplicates = find_duplicates(backend.as_ref(), "cloud_folder").await?;
    println!("Found {} duplicate groups.", duplicates.len());

    // 2. Resolve duplicates by keeping the newest file
    let summary = resolve_duplicates(backend.as_ref(), &duplicates, DedupeStrategy::KeepNewest).await?;
    println!("Deleted {} duplicates, recovered {} bytes.",
        summary.files_deleted, summary.bytes_recovered);

    Ok(())
}
```

---

## 4. Storage Analytics & Directory Tree Visualization

Located in [`crates/cloud_sync_lib/src/analytics.rs`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_lib/src/analytics.rs), this tool calculates detailed storage consumption metrics with $O(1)$ top-K bounded memory:

### 4.1 Storage Metrics (`StorageAnalytics`)
- **Global Totals**: Total file count, folder count, and aggregate byte size.
- **Extension Breakdown**: Aggregates file count and byte usage grouped by file extension (e.g. `.png`, `.pdf`, `.mp4`).
- **Top-10 Largest Files**: Tracks the largest files across the hierarchy using a bounded min-heap.
- **Hierarchical Directory Tree (`TreeNode`)**: Builds an in-memory recursive tree suitable for JSON serialization or ASCII terminal rendering.

```rust,no_run
use cloud_sync_lib::analytics::compute_storage_analytics;
use cloud_sync_core::StorageBackend;
use std::sync::Arc;

pub async fn analyze_storage(backend: Arc<dyn StorageBackend>) -> Result<(), cloud_sync_core::StorageError> {
    let report = compute_storage_analytics(backend.as_ref(), "").await?;
    println!("Total Files: {}, Total Size: {:.2} MB",
        report.total_files, report.total_bytes as f64 / (1024.0 * 1024.0));

    for (ext, stats) in report.extension_breakdown {
        println!("  - {}: {} files ({} bytes)", ext, stats.count, stats.bytes);
    }
    Ok(())
}
```

---

## 5. Staggered File Versioning & Trash Manager (`TrashManager`)

Located in [`crates/cloud_sync_daemon/src/engine/versioning.rs`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_daemon/src/engine/versioning.rs):

### 5.1 Architecture & Archive Syntax
Whenever a file is updated or deleted locally during synchronization, `TrashManager` intercepts the action and archives a copy to `sync_state/trash/`:

```
sync_state/trash/
├── 20261124_091530_document.pdf
├── 20261124_114512_document.pdf
└── 20261124_140022_notes.txt
```

- **Naming Scheme**: `YYYYMMDD_HHMMSS_<original_filename>` (or `YYYYMMDD_HHMMSS_NNN_<original_filename>` if multiple archives occur in the same second).
- **Retention Limits**:
  - `retention_days`: Files older than the configured threshold (default: 30 days) are pruned.
  - `max_versions_per_file`: Prunes the oldest versions if archives for a specific file exceed the limit (default: 5).

### 5.2 Point-in-Time File Restoration
The `TrashManager` provides helper methods to enumerate past versions and restore any archived copy back to its original path:
- `list_versions(&original_filename)`
- `restore_version(&archived_version_id, &target_path)`

---

## 6. Heuristic Rename & Move Detection (`RenameDetector`)

Located in [`crates/cloud_sync_daemon/src/engine/rename_detector.rs`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_daemon/src/engine/rename_detector.rs):

### 6.1 Move Detection Algorithm
When files are renamed or moved between subfolders, naive sync engines execute a destructive remote deletion followed by a re-upload of the entire file.

`RenameDetector` eliminates this waste:
1. Gathers all pending deletions ($D$) and additions ($A$) for the reconciliation cycle.
2. Matches deleted and added candidates using:
   - **Exact Content Hash**: SHA-256 / MD5 equality.
   - **File Size & Basename Heuristic**: Identical byte size and matching file name across directories.
3. Successfully matched pairs are emitted as `DetectedMove { from, to, size }`.
4. The executor leverages server-side move operations (`ServerSideOperations::rename`) where supported, reducing network transfer to zero bytes.
