use cloud_sync_lib::{LocalSimulation, SimulatedFallback, StorageBackend, SyncMode};
use cloud_sync_lib::rate_limit::TokenBucket;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- Rate Limiting Example ---");

    // Create a temporary directory for our mock remote storage
    let temp_dir = std::env::temp_dir().join("cloud_sync_rate_limiting_demo");
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    std::fs::create_dir_all(&temp_dir)?;

    // Configure token buckets for rate limiting (e.g. 100 bytes/sec limits for visible delays)
    let upload_limiter = TokenBucket::new(100);
    let download_limiter = TokenBucket::new(100);

    // Initialize LocalSimulation mock backend with rate limiters
    let local_sim = LocalSimulation::new(temp_dir.clone(), "MockRateLimitProvider".to_string())
        .with_limiters(Some(upload_limiter), Some(download_limiter));
    
    let backend = SimulatedFallback::new(
        None::<LocalSimulation>,
        local_sim,
        "MockRateLimitProvider",
        SyncMode::TwoWay,
    );

    // Create a local file with 300 bytes of data (should take ~3 seconds to upload/download)
    let local_file_path = std::env::temp_dir().join("rate_limit_demo.txt");
    let mut file = File::create(&local_file_path)?;
    file.write_all(&vec![b'A'; 300])?;
    println!("Created local file with 300 bytes of data.");

    // Upload with rate limiting
    let start_upload = Instant::now();
    println!("Uploading at 100 bytes/sec limit...");
    backend.upload(&local_file_path, "large_file.txt").await?;
    let upload_duration = start_upload.elapsed();
    println!("Upload finished in {:.2?}", upload_duration);

    // Download with rate limiting
    let download_file_path = std::env::temp_dir().join("rate_limit_downloaded.txt");
    let start_download = Instant::now();
    println!("Downloading at 100 bytes/sec limit...");
    backend.download("large_file.txt", &download_file_path).await?;
    let download_duration = start_download.elapsed();
    println!("Download finished in {:.2?}", download_duration);

    // Clean up
    let _ = std::fs::remove_file(local_file_path);
    let _ = std::fs::remove_file(download_file_path);
    let _ = std::fs::remove_dir_all(temp_dir);

    println!("Example run completed successfully!");
    Ok(())
}
