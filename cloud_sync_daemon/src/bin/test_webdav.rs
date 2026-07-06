//! Standalone diagnostic utility to verify WebDAV client connection status.

#[cfg(feature = "webdav")]
use cloud_sync_lib::WebDAVProvider;

#[cfg(feature = "webdav")]
#[path = "../config.rs"]
pub mod config;

#[cfg(feature = "webdav")]
#[path = "common.rs"]
pub mod common;

#[cfg(feature = "webdav")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("WebDAV Connection Verifier");
    println!("--------------------------");

    let config_file = common::resolve_config_file();
    println!("Loading configuration from: {}", config_file);
    let config = config::load_or_create_config(config_file).await?;

    let credentials = match config.webdav_credentials {
        Some(creds) => {
            if creds.url.is_empty() {
                eprintln!("Error: WebDAV URL is empty in configuration.");
                std::process::exit(1);
            }
            creds
        }
        None => {
            eprintln!("Error: [webdav_credentials] section not found in configuration.");
            std::process::exit(1);
        }
    };

    println!("Target WebDAV URL: {}", credentials.url);
    println!("Username: {}", credentials.username);
    println!("Destination Folder: {:?}", credentials.common.destination_folder);

    println!("\nInitializing WebDAV provider...");
    let provider = WebDAVProvider::new(credentials);

    common::run_connection_diagnostics(&provider, "webdav_test_connection_tmp.txt").await
}

#[cfg(not(feature = "webdav"))]
fn main() {
    println!("WebDAV provider feature is not enabled. Recompile with --features webdav to use this verifier.");
}
