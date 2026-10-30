//! # Resumable Upload and Rate Limiting Example
//!
//! Demonstrates configuring bandwidth rate limits and monitoring upload metrics
//! for large file transfers.
//!
//! Run with:
//! ```bash
//! cargo run --example resumable_upload -p cloud_sync_lib
//! ```

use cloud_sync_lib::{CloudSyncClient, StorageError};
use tempfile::tempdir;

#[tokio::main]
async fn main() -> Result<(), StorageError> {
    println!("=== CloudSync: Resumable Upload & Rate Limiting Example ===");

    let sandbox = tempdir().map_err(StorageError::Io)?;
    let cloud_sim_dir = sandbox.path().join("cloud_storage");
    let local_dir = sandbox.path().join("local_cache");
    tokio::fs::create_dir_all(&cloud_sim_dir).await.map_err(StorageError::Io)?;
    tokio::fs::create_dir_all(&local_dir).await.map_err(StorageError::Io)?;

    // Create a 2MB test payload
    let large_file = local_dir.join("large_archive.bin");
    let payload = vec![0xABu8; 2 * 1024 * 1024];
    tokio::fs::write(&large_file, &payload)
        .await
        .map_err(StorageError::Io)?;
    println!("Generated 2 MB test archive at {:?}", large_file);

    // 1. Build client configured with bandwidth rate limits (e.g., 5 MB/s upload)
    let client = CloudSyncClient::builder()
        .with_local_sim(&cloud_sim_dir, "throttled_sim")
        .with_rate_limits(5 * 1024 * 1024, 10 * 1024 * 1024)
        .with_metrics()
        .with_logging()
        .build()?;

    let start = std::time::Instant::now();
    println!("Initiating rate-bounded upload to 'backups/archive.bin'...");
    client.upload(&large_file, "backups/archive.bin").await?;
    let elapsed = start.elapsed();

    println!("Upload completed in {:.2?}", elapsed);

    // 2. Verify stored size
    let items = client.list("backups").await?;
    for item in &items {
        println!(" - Confirmed remote file {:?}: {} bytes", item.path, item.size);
    }

    println!("=== Resumable Upload & Rate Limiting Example completed! ===");
    Ok(())
}

