//! Standalone diagnostic utility to verify SFTP client connection status.

#[cfg(feature = "sftp")]
use cloud_sync_lib::SFTPProvider;

#[cfg(feature = "sftp")]
#[path = "../config.rs"]
pub mod config;

#[cfg(feature = "sftp")]
#[path = "common.rs"]
pub mod common;

#[cfg(feature = "sftp")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("SFTP Connection Verifier");
    println!("------------------------");

    let config_file = common::resolve_config_file();
    println!("Loading configuration from: {}", config_file);
    let config = config::load_or_create_config(config_file).await?;

    let credentials = match config.sftp_credentials {
        Some(creds) => {
            if creds.host.is_empty() {
                eprintln!("Error: SFTP Host is empty in configuration.");
                std::process::exit(1);
            }
            creds
        }
        None => {
            eprintln!("Error: [sftp_credentials] section not found in configuration.");
            std::process::exit(1);
        }
    };

    println!("Target SFTP Host: {}:{}", credentials.host, credentials.port.unwrap_or(22));
    println!("Username: {}", credentials.username);
    println!("Destination Folder: {:?}", credentials.common.destination_folder);

    println!("\nInitializing SFTP provider...");
    let provider = SFTPProvider::new(credentials);

    common::run_connection_diagnostics(&provider, "sftp_test_connection_tmp.txt").await
}

#[cfg(not(feature = "sftp"))]
fn main() {
    println!("SFTP provider feature is not enabled. Recompile with --features sftp to use this verifier.");
}
