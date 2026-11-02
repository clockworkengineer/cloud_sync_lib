//! # Basic Synchronization Example
//!
//! Demonstrates the simplest way to configure a `CloudSyncClient` and perform
//! file upload, listing, and download operations using the unified client API.
//!
//! Run with:
//! ```bash
//! cargo run --example basic_sync -p cloud_sync_lib
//! ```

use cloud_sync_lib::{CloudSyncClient, StorageError};
use std::time::Duration;
use tempfile::tempdir;

#[tokio::main]
async fn main() -> Result<(), StorageError> {
    println!("=== CloudSync: Basic Sync Example ===");

    // Create a temporary sandbox simulating local folder and cloud destination
    let sandbox = tempdir().map_err(StorageError::Io)?;
    let cloud_sim_dir = sandbox.path().join("remote_cloud");
    let local_work_dir = sandbox.path().join("local_work");
    tokio::fs::create_dir_all(&cloud_sim_dir)
        .await
        .map_err(StorageError::Io)?;
    tokio::fs::create_dir_all(&local_work_dir)
        .await
        .map_err(StorageError::Io)?;

    // 1. Build a CloudSyncClient with fluent options (Attribute 1: Intuitive API)
    let client = CloudSyncClient::builder()
        .with_local_sim(&cloud_sim_dir, "local_backup")
        .with_retry(3, Duration::from_millis(100), Duration::from_secs(2))
        .with_logging()
        .with_metrics()
        .build()?;

    println!(
        "Initialized client for provider: {}",
        client.provider_name()
    );

    // 2. Prepare a local sample file to upload
    let sample_file = local_work_dir.join("quarterly_report.txt");
    tokio::fs::write(
        &sample_file,
        b"Quarterly Cloud Sync Financial Report - 2026",
    )
    .await
    .map_err(StorageError::Io)?;

    // 3. Upload to cloud
    println!("Uploading {:?} -> 'reports/quarterly.txt'...", sample_file);
    client.upload(&sample_file, "reports/quarterly.txt").await?;
    println!("Upload completed successfully!");

    // 4. List remote items
    println!("\nListing cloud contents:");
    let items = client.list("reports").await?;
    for item in &items {
        println!(
            " - Remote item: {:?} (size: {} bytes)",
            item.path, item.size
        );
    }

    // 5. Download back to a different local file
    let restored_file = local_work_dir.join("restored_report.txt");
    println!(
        "\nDownloading 'reports/quarterly.txt' -> {:?}...",
        restored_file
    );
    client
        .download("reports/quarterly.txt", &restored_file)
        .await?;

    let downloaded_content = tokio::fs::read_to_string(&restored_file)
        .await
        .map_err(StorageError::Io)?;
    println!("Downloaded verification content: '{}'", downloaded_content);

    println!("\n=== Basic Sync Example finished cleanly! ===");
    Ok(())
}
