# DRY Refactoring Plan for Daemon Test Verifiers

This document outlines a plan to eliminate code duplication across the standalone diagnostic/verification binaries in the `cloud_sync_daemon/src/bin/` directory.

---

## 1. Identified Areas of Code Duplication

There are currently 6 verification binaries:
*   `test_box.rs`
*   `test_mega.rs`
*   `test_nextcloud.rs`
*   `test_s3.rs`
*   `test_sftp.rs`
*   `test_webdav.rs`

All of these files duplicate the following logic:
1.  **Configuration File Resolution**: Checking if `private_config.toml` exists, otherwise falling back to `config.toml`.
2.  **Listing Test**: Calling `provider.list("")` and printing the first 5 items.
3.  **Read/Write/Delete Test**:
    *   Creating a local temporary file named `{provider}_test_connection_tmp.txt`.
    *   Writing dummy content with a timestamp.
    *   Uploading the file to remote storage.
    *   Deleting the remote temporary file.
    *   Cleaning up the local temporary file.
    *   Gracefully handling any listing/upload/deletion errors and exiting with a status code of `1`.

---

## 2. Proposed DRY Refactoring Architecture

### A. Core Connection Test Harness
We will introduce a shared, generic connection test function. This function can reside in a new module `cloud_sync_daemon/src/bin/common.rs` or in the existing daemon utility module `cloud_sync_daemon/src/utils.rs`.

```rust
use std::fs::File;
use std::io::Write;
use std::path::Path;
use cloud_sync_lib::{StorageBackend, StorageError};

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
```

### B. Simplified Test Verifiers
With the test harness defined, each specific verification binary is simplified to just handling credentials loading and provider instantiation:

```rust
//! Standalone diagnostic utility to verify MEGA client connection status.

#[cfg(feature = "mega")]
use cloud_sync_lib::MegaProvider;

#[cfg(feature = "mega")]
#[path = "../config.rs"]
pub mod config;

#[cfg(feature = "mega")]
#[path = "common.rs"] // or referencing daemon utils
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
```

---

## 3. Step-by-Step Implementation Guide

1.  **Extract Shared Logic**: Create a new module `cloud_sync_daemon/src/bin/common.rs` to host `resolve_config_file` and `run_connection_diagnostics`.
2.  **Refactor Individual Verifiers**:
    *   Import the common module in `test_box.rs`, `test_mega.rs`, `test_nextcloud.rs`, `test_s3.rs`, `test_sftp.rs`, and `test_webdav.rs`.
    *   Call `common::resolve_config_file()` to fetch the config file name.
    *   Delete the duplicated listing, upload, and deletion code in all 6 files, replacing it with a single call to `common::run_connection_diagnostics`.
3.  **Verification**: Compile and run each verifier to ensure output messages, file paths, and exit behavior remain identical.
