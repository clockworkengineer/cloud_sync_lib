//! Standalone diagnostic utility to verify S3 client connection status.

#[cfg(feature = "s3")]
use cloud_sync_lib::S3Provider;

#[cfg(feature = "s3")]
#[path = "../config.rs"]
pub mod config;

#[cfg(feature = "s3")]
#[path = "common.rs"]
pub mod common;

#[cfg(feature = "s3")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("S3 Connection Verifier");
    println!("----------------------");

    let config_file = common::resolve_config_file();
    println!("Loading configuration from: {}", config_file);
    let config = config::load_or_create_config(config_file).await?;

    let credentials = match config.s3_credentials {
        Some(creds) => {
            if creds.bucket.is_empty() {
                eprintln!("Error: S3 Bucket is empty in configuration.");
                std::process::exit(1);
            }
            creds
        }
        None => {
            eprintln!("Error: [s3_credentials] section not found in configuration.");
            std::process::exit(1);
        }
    };

    println!("Target S3 Bucket: {}", credentials.bucket);
    println!("Region: {}", credentials.region);
    println!("Endpoint: {:?}", credentials.endpoint);
    println!("Destination Folder: {:?}", credentials.common.destination_folder);

    println!("\nInitializing S3 provider...");
    let provider = S3Provider::new(credentials);

    common::run_connection_diagnostics(&provider, "s3_test_connection_tmp.txt").await
}

#[cfg(not(feature = "s3"))]
fn main() {
    println!("S3 provider feature is not enabled. Recompile with --features s3 to use this verifier.");
}
