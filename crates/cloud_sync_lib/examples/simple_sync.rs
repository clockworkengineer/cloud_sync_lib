use cloud_sync_lib::{LocalSimulation, SimulatedFallback, StorageBackend, SyncMode};
use std::fs::File;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- Simple Sync Example ---");

    // Create a temporary directory for our mock remote storage
    let temp_dir = std::env::temp_dir().join("cloud_sync_simple_sync_demo");
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    std::fs::create_dir_all(&temp_dir)?;

    println!("Mock remote storage initialized at: {:?}", temp_dir);

    // Initialize LocalSimulation mock backend
    let local_sim = LocalSimulation::new(temp_dir.clone(), "MockProvider".to_string());
    
    // Wrap it in SimulatedFallback (here simulating with no actual cloud connection)
    let backend = SimulatedFallback::new(
        None::<LocalSimulation>, // No real remote provider
        local_sim,
        "MockProvider",
        SyncMode::TwoWay,
    );

    // 1. Create a local file to upload
    let local_file_path = std::env::temp_dir().join("hello_example.txt");
    let mut file = File::create(&local_file_path)?;
    writeln!(file, "Hello from cloud_sync_lib examples!")?;
    println!("Created local file: {:?}", local_file_path);

    // 2. Upload the file to remote path "docs/hello.txt"
    println!("Uploading file to remote: 'docs/hello.txt'...");
    backend.upload(&local_file_path, "docs/hello.txt").await?;
    println!("Upload completed successfully.");

    // 3. List the contents of remote folder
    println!("Listing remote root contents:");
    let items = backend.list("").await?;
    for item in items {
        println!(
            " - Path: {:?}, Size: {} bytes, Is Directory: {}",
            item.path, item.size, item.is_dir
        );
    }

    // 4. Download the file back to a new local path
    let download_file_path = std::env::temp_dir().join("hello_downloaded.txt");
    println!("Downloading remote 'docs/hello.txt' to {:?}", download_file_path);
    backend.download("docs/hello.txt", &download_file_path).await?;
    
    let content = std::fs::read_to_string(&download_file_path)?;
    println!("Downloaded content matches: '{}'", content.trim());

    // Clean up local files
    let _ = std::fs::remove_file(local_file_path);
    let _ = std::fs::remove_file(download_file_path);
    let _ = std::fs::remove_dir_all(temp_dir);

    println!("Example run completed successfully!");
    Ok(())
}
