//! Demonstrates building a `CloudSyncClient` with transparent payload compression (`CompressionLayer`),
//! TTL metadata caching (`CacheLayer`), large-file splitting (`ChunkerBackend`), content-addressable
//! deduplicated cloud snapshots (`SnapshotEngine`), and integrity verification (`verify_integrity`).

use cloud_sync_lib::CloudSyncClient;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp_root = tempfile::tempdir()?;
    let local_workspace = temp_root.path().join("workspace");
    let sim_cloud = temp_root.path().join("cloud");
    let restore_dir = temp_root.path().join("restored");
    tokio::fs::create_dir_all(&local_workspace).await?;

    // 1. Build a layered CloudSyncClient with Compression, Chunker (4 KB), and Cache (30s)
    let client = CloudSyncClient::builder()
        .with_local_sim(&sim_cloud, "LayeredCloud")
        .with_compression()
        .with_chunker(4096)
        .with_cache(Duration::from_secs(30))
        .with_logging()
        .build()?;

    // 2. Write sample files into local workspace
    let doc_text = "CloudSync quality attributes refactoring verification payload.\n".repeat(120);
    let doc_path = local_workspace.join("architecture.txt");
    tokio::fs::write(&doc_path, doc_text.as_bytes()).await?;

    // 3. Upload and verify integrity
    client.upload(&doc_path, "architecture.txt").await?;
    let report = client.verify_integrity(&local_workspace, "", false).await?;
    println!(
        "Integrity check identical: {} ({} file matched)",
        report.is_identical(),
        report.matched.len()
    );

    // 4. Create an immutable deduplicated point-in-time snapshot
    let snap = client
        .create_snapshot(&local_workspace, "v1-release")
        .await?;
    println!(
        "Created snapshot '{}' ({} files, {} new blobs uploaded)",
        snap.id, snap.total_files, snap.new_blobs_uploaded
    );

    // 5. Restore snapshot into a clean directory
    let restored = client.restore_snapshot(&snap.id, &restore_dir).await?;
    println!(
        "Restored {} file(s) ({} bytes) from snapshot '{}'",
        restored.files_restored, restored.bytes_restored, restored.snapshot_id
    );

    Ok(())
}
