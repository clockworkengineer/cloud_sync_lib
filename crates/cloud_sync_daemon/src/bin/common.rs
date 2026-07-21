//! Shared diagnostic test harness functions for verifier binaries.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use cloud_sync_lib::StorageBackend;

/// Resolves the default configuration file path based on existence.
pub fn resolve_config_file() -> &'static str {
    if std::path::Path::new("private_config.toml").exists() {
        "private_config.toml"
    } else {
        "config.toml"
    }
}

/// Runs the standard connection diagnostics flow (List -> Upload -> Delete)
/// for any given storage provider backend.
pub async fn run_connection_diagnostics(
    provider: &dyn StorageBackend,
    temp_filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
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

    println!("\n2. Performing read/write test (Upload -> List -> Delete)...");
    
    // Create local temporary file
    {
        let mut file = File::create(temp_filename)?;
        writeln!(file, "{} connectivity check file. Created at: {:?}", provider.name(), std::time::SystemTime::now())?;
    }

    println!("   -> Uploading temporary file...");
    match provider.upload(Path::new(temp_filename), "test_connectivity_check.txt").await {
        Ok(_) => println!("   -> Upload successful!"),
        Err(e) => {
            eprintln!("\n❌ Upload failed:\n{:?}\n", e);
            let _ = std::fs::remove_file(temp_filename);
            std::process::exit(1);
        }
    }

    println!("   -> Deleting remote temporary file...");
    match provider.delete("test_connectivity_check.txt").await {
        Ok(_) => println!("   -> Deletion successful!"),
        Err(e) => {
            eprintln!("\n❌ Deletion of remote file failed:\n{:?}\n", e);
            let _ = std::fs::remove_file(temp_filename);
            std::process::exit(1);
        }
    }

    // Clean up local temp file
    let _ = std::fs::remove_file(temp_filename);

    println!("\n🎉 All {} connectivity tests passed successfully!", provider.name());
    Ok(())
}

/// Declarative macro generating standalone verifier binaries.
#[macro_export]
macro_rules! define_verifier_binary {
    ($title:expr, $feature:expr, $creds_field:ident, $ProviderStruct:ident, $tmp_file:expr) => {
        #[cfg(feature = $feature)]
        #[path = "../config.rs"]
        pub mod config;

        #[cfg(feature = $feature)]
        #[path = "common.rs"]
        pub mod common;

        #[cfg(feature = $feature)]
        #[tokio::main]
        async fn main() -> Result<(), Box<dyn std::error::Error>> {
            println!("{} Connection Verifier", $title);
            println!("-----------------------------");

            let config_file = common::resolve_config_file();
            println!("Loading configuration from: {}", config_file);
            let config = config::load_or_create_config(config_file).await?;

            let credentials = match config.$creds_field {
                Some(creds) => creds,
                None => {
                    eprintln!("Error: [{}] section not found in configuration.", stringify!($creds_field));
                    std::process::exit(1);
                }
            };

            println!("Destination Folder: {:?}", credentials.common.destination_folder);

            println!("\nInitializing {} provider...", $title);
            let provider = cloud_sync_lib::$ProviderStruct::new(credentials);

            common::run_connection_diagnostics(&provider, $tmp_file).await
        }

        #[cfg(not(feature = $feature))]
        fn main() {
            println!("{} provider feature is not enabled. Recompile with --features {} to use this verifier.", $title, $feature);
        }
    };
}

#[allow(dead_code)]
fn main() {}


