//! Standalone diagnostic utility to verify Nextcloud client connection status.

#[cfg(feature = "nextcloud")]
use cloud_sync_lib::NextcloudProvider;

#[cfg(feature = "nextcloud")]
#[path = "../config.rs"]
pub mod config;

#[cfg(feature = "nextcloud")]
#[path = "common.rs"]
pub mod common;

#[cfg(feature = "nextcloud")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Nextcloud Connection Verifier");
    println!("-----------------------------");

    let config_file = common::resolve_config_file();
    println!("Loading configuration from: {}", config_file);
    let config = config::load_or_create_config(config_file).await?;

    let credentials = match config.nextcloud_credentials {
        Some(creds) => {
            if creds.url.is_empty() {
                eprintln!("Error: Nextcloud URL is empty in configuration.");
                std::process::exit(1);
            }
            creds
        }
        None => {
            eprintln!("Error: [nextcloud_credentials] section not found in configuration.");
            std::process::exit(1);
        }
    };

    println!("Target Nextcloud URL: {}", credentials.url);
    println!("Username: {}", credentials.username);
    println!("Destination Folder: {:?}", credentials.common.destination_folder);

    println!("\nInitializing Nextcloud provider...");
    let provider = NextcloudProvider::new(credentials);

    common::run_connection_diagnostics(&provider, "nextcloud_test_connection_tmp.txt").await
}

#[cfg(not(feature = "nextcloud"))]
fn main() {
    println!("Nextcloud provider feature is not enabled. Recompile with --features nextcloud to use this verifier.");
}
