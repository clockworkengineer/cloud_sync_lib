//! Standalone diagnostic utility to verify MEGA client connection status.

#[cfg(feature = "mega")]
use cloud_sync_lib::MegaProvider;

#[cfg(feature = "mega")]
#[path = "../config.rs"]
pub mod config;

#[cfg(feature = "mega")]
#[path = "common.rs"]
pub mod common;

#[cfg(feature = "mega")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("MEGA Connection Verifier");
    println!("-----------------------");

    let config_file = common::resolve_config_file();
    println!("Loading configuration from: {}", config_file);
    let config = config::load_or_create_config(config_file).await?;

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

    common::run_connection_diagnostics(&provider, "mega_test_connection_tmp.txt").await
}

#[cfg(not(feature = "mega"))]
fn main() {
    println!("MEGA provider feature is not enabled. Recompile with --features mega to use this verifier.");
}
