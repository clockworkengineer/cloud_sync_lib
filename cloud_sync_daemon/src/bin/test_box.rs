//! Standalone diagnostic utility to verify Box client connection status.

#[cfg(feature = "box")]
use cloud_sync_lib::BoxProvider;

#[cfg(feature = "box")]
#[path = "../config.rs"]
pub mod config;

#[cfg(feature = "box")]
#[path = "common.rs"]
pub mod common;

#[cfg(feature = "box")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Box Connection Verifier");
    println!("-----------------------");

    let config_file = common::resolve_config_file();
    println!("Loading configuration from: {}", config_file);
    let config = config::load_or_create_config(config_file).await?;

    let credentials = match config.box_credentials {
        Some(creds) => {
            if creds.client_id.is_empty() {
                eprintln!("Error: Box client_id is empty in configuration.");
                std::process::exit(1);
            }
            creds
        }
        None => {
            eprintln!("Error: [box_credentials] section not found in configuration.");
            std::process::exit(1);
        }
    };

    println!("Client ID: {}", credentials.client_id);
    println!("Destination Folder: {:?}", credentials.common.destination_folder);

    println!("\nInitializing Box provider...");
    let provider = BoxProvider::new(credentials);

    common::run_connection_diagnostics(&provider, "box_test_connection_tmp.txt").await
}

#[cfg(not(feature = "box"))]
fn main() {
    println!("Box provider feature is not enabled. Recompile with --features box to use this verifier.");
}
