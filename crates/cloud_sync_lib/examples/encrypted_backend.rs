use cloud_sync_lib::{EncryptedBackend, LocalSimulation, SimulatedFallback, StorageBackend, SyncMode};
use std::fs::File;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- Encrypted Backend Example ---");

    // Create a temporary directory for our mock remote storage
    let temp_dir = std::env::temp_dir().join("cloud_sync_encryption_demo");
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    std::fs::create_dir_all(&temp_dir)?;

    // Initialize LocalSimulation mock backend
    let local_sim = LocalSimulation::new(temp_dir.clone(), "EncryptedMockProvider".to_string());
    let fallback = SimulatedFallback::new(
        None::<LocalSimulation>,
        local_sim,
        "EncryptedMockProvider",
        SyncMode::TwoWay,
    );

    // Wrap the fallback provider with client-side encryption using a password
    let password = "my_super_secret_encryption_password";
    let encrypted_backend = EncryptedBackend::new(fallback, password);
    println!("Encrypted backend initialized.");

    // 1. Create a local file to upload
    let local_file_path = std::env::temp_dir().join("plain.txt");
    let mut file = File::create(&local_file_path)?;
    writeln!(file, "This is confidential data to be encrypted on upload.")?;
    println!("Created local plain file.");

    // 2. Upload through encrypted backend (should encrypt data on the fly)
    println!("Uploading to remote 'secrets/confidential.txt'...");
    encrypted_backend.upload(&local_file_path, "secrets/confidential.txt").await?;
    println!("Upload completed.");

    // 3. Let's inspect the file directly in the mock remote folder (it should be encrypted)
    let raw_remote_file_path = temp_dir.join("secrets/confidential.txt");
    if raw_remote_file_path.exists() {
        let raw_remote_data = std::fs::read(&raw_remote_file_path)?;
        println!(
            "Inspection of mock remote file: {} bytes (Data starts with non-text bytes: {:?})",
            raw_remote_data.len(),
            &raw_remote_data[..std::cmp::min(10, raw_remote_data.len())]
        );
    }

    // 4. Download through encrypted backend (should decrypt back to original text)
    let decrypted_file_path = std::env::temp_dir().join("decrypted.txt");
    println!("Downloading and decrypting remote file...");
    encrypted_backend.download("secrets/confidential.txt", &decrypted_file_path).await?;

    let content = std::fs::read_to_string(&decrypted_file_path)?;
    println!("Decrypted content: '{}'", content.trim());

    // Clean up
    let _ = std::fs::remove_file(local_file_path);
    let _ = std::fs::remove_file(decrypted_file_path);
    let _ = std::fs::remove_dir_all(temp_dir);

    println!("Example run completed successfully!");
    Ok(())
}
