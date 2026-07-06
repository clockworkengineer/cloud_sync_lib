//! Standalone diagnostic utility to verify MEGA client connection status.

#[cfg(feature = "mega")]
use std::fs::File;
#[cfg(feature = "mega")]
use std::io::Write;
#[cfg(feature = "mega")]
use std::path::Path;
#[cfg(feature = "mega")]
use cloud_sync_lib::{MegaProvider, StorageBackend};

#[cfg(feature = "mega")]
#[path = "../config.rs"]
pub mod config;

#[cfg(feature = "mega")]
use config::load_or_create_config;

#[cfg(feature = "mega")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("MEGA Connection Verifier");
    println!("-----------------------");

    let config_file = if std::path::Path::new("private_config.toml").exists() {
        "private_config.toml"
    } else {
        "config.toml"
    };

    println!("Loading configuration from: {}", config_file);
    let config = load_or_create_config(config_file).await?;

    let credentials = match config.mega_credentials {
        Some(creds) => {
            if creds.email.is_empty() {
                eprintln!("Error: MEGA email is empty in configuration.");
                std::process::exit(1);
            }
            creds
        }
        None => {
            eprintln!("Error: [mega_credentials] section not found in configuration.");
            std::process::exit(1);
        }
    };

    println!("Email: {}", credentials.email);
    println!("Destination Folder: {:?}", credentials.common.destination_folder);

    println!("\nInitializing MEGA provider...");
    let provider = MegaProvider::new(credentials);

    // 1. Try listing remote folder contents (verifies connection & credentials)
    println!("1. Fetching remote directory listing...");
    match provider.list("").await {
        Ok(items) => {
            println!("   -> Success! Found {} items.", items.len());
            for item in items.iter().take(5) {
                println!("      - {} ({})", item.path.display(), if item.is_dir { "Directory" } else { "File" });
            }
            if items.len() > 5 {
                println!("      - ... and {} more items", items.len() - 5);
            }
        }
        Err(e) => {
            eprintln!("\n❌ Connection Failed during listing:\n{:?}\n", e);
            std::process::exit(1);
        }
    }

    // 2. Try creating, uploading, and deleting a temporary test file
    let temp_file_path = "mega_test_connection_tmp.txt";
    println!("\n2. Performing read/write test (Upload -> List -> Delete)...");
    
    // Create a local temporary file
    {
        let mut file = File::create(temp_file_path)?;
        writeln!(file, "MEGA connectivity check file. Created at: {:?}", std::time::SystemTime::now())?;
    }

    println!("   -> Uploading temporary file...");
    match provider.upload(Path::new(temp_file_path), "test_connectivity_check.txt").await {
        Ok(_) => println!("   -> Upload successful!"),
        Err(e) => {
            eprintln!("\n❌ Upload failed:\n{:?}\n", e);
            let _ = std::fs::remove_file(temp_file_path);
            std::process::exit(1);
        }
    }

    println!("   -> Deleting remote temporary file...");
    match provider.delete("test_connectivity_check.txt").await {
        Ok(_) => println!("   -> Deletion successful!"),
        Err(e) => {
            eprintln!("\n❌ Deletion of remote file failed:\n{:?}\n", e);
            let _ = std::fs::remove_file(temp_file_path);
            std::process::exit(1);
        }
    }

    // Clean up local temp file
    let _ = std::fs::remove_file(temp_file_path);

    println!("\n🎉 All MEGA connectivity tests passed successfully!");
    Ok(())
}

#[cfg(not(feature = "mega"))]
fn main() {
    print!("MEGA provider feature is not enabled. Recompile with --features mega to use this verifier.");
}
