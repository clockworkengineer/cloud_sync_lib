# Azure Blob Storage Integration & Expansion Plan

This document outlines a standardized, concrete technical implementation plan for adding **Azure Blob Storage** support to the cloud sync workspace.

---

## 1. Core Library Changes (`cloud_sync_lib`)

### A. Dependencies
Add the official SDK crates to `cloud_sync_lib/Cargo.toml`:
```toml
[dependencies]
azure_core = { version = "0.21", optional = true }
azure_storage = { version = "0.21", optional = true }
azure_storage_blobs = { version = "0.21", optional = true }
```
Add a new cargo feature `azure_blob` that enables these dependencies.

### B. Configuration Struct
Add the credential configuration struct in [`cloud_sync_lib/src/providers/mod.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/mod.rs):
```rust
/// Credentials configuration for Azure Blob Storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureBlobCredentials {
    /// Azure Storage Account name.
    pub account_name: String,
    /// Azure Storage Account Access Key.
    pub account_key: String,
    /// Target Container name.
    pub container: String,
    /// Custom endpoint URL (optional, used for local Azurite emulator).
    pub endpoint: Option<String>,
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional toggle to enable/disable deletion syncing.
    pub sync: Option<bool>,
}
```

Make sure to conditionally compile the module and re-export the provider:
```rust
#[cfg(feature = "azure_blob")]
pub mod azure_blob;
#[cfg(feature = "azure_blob")]
pub use azure_blob::AzureBlobProvider;
```

### C. Provider Implementation
Create a new file [`cloud_sync_lib/src/providers/azure_blob.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/azure_blob.rs):

1. Define the `AzureBlobProvider` struct.
2. Implement the `StorageBackend` trait methods mapping to the `azure_storage_blobs` SDK client:
   * **`upload`**: Connect to container, resolve target blob path, and call `put_block_blob` (or `put_page_blob` / chunked upload).
   * **`download`**: Call `get_blob` and write body stream to `local_path`.
   * **`delete`**: Call `delete_blob` on the specified blob path.
   * **`list`**: Call `list_blobs` to scan matching path prefixes, map the returned properties (content length, last modified) to `StorageItem` structs.

### D. Local Simulation Setup
Update [`local_sim.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/local_sim.rs) and the README to support `./cloud_simulation/azure_blob` as a fallback path when credentials are not configured.

---

## 2. Daemon Integration Changes (`cloud_sync_daemon`)

### A. Configuration Parsing
Update `AppConfig` in `cloud_sync_daemon/src/config.rs` to include the configuration fields:
```rust
pub struct AppConfig {
    // ...
    pub azure_blob_root: PathBuf,
    pub azure_blob_credentials: Option<AzureBlobCredentials>,
}
```

### B. Startup Registration (`main.rs`)
In `cloud_sync_daemon/src/main.rs`, register the provider conditionally:
```rust
if is_azure_blob_enabled(&config.azure_blob_credentials) {
    let sync = config.azure_blob_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
    let inner = config.azure_blob_credentials.clone().map(AzureBlobProvider::new);
    let local_sim = LocalSimulation::new(config.azure_blob_root.clone(), "Azure Blob".to_string());
    backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "Azure Blob", sync)));
}
```
Apply the same instantiation code in `control.rs` to support runtime configuration reloads.

---

## 3. Verification Plan

1. **Unit Tests**:
   - Write tests in `cloud_sync_lib/src/lib.rs` verifying local simulation mode for `AzureBlobProvider` works correctly without network connection.
2. **Integration / Emulator Verification**:
   - Spin up Azurite locally via Docker.
   - Run the integration test suite configured against the local Azurite emulator endpoint (`http://127.0.0.1:10000/devstoreaccount1`).
