# pCloud Storage Integration & Expansion Plan

This document outlines a standardized, concrete technical implementation plan for adding **pCloud** support to the cloud sync workspace.

---

## 1. Core Library Changes (`cloud_sync_lib`)

### A. Dependencies
pCloud uses HTTPS REST endpoints with JSON responses. Standard multipart form data is used for file uploads. The existing `reqwest` and `serde_json` crates provide all necessary capabilities.

### B. Configuration Struct
Add the credential configuration struct in [`cloud_sync_lib/src/providers/mod.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/mod.rs):
```rust
/// Credentials configuration for pCloud.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PCloudCredentials {
    /// pCloud OAuth2 Access Token.
    pub access_token: String,
    /// Custom API endpoint (optional, e.g. for European accounts or testing).
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
#[cfg(feature = "pcloud")]
pub mod pcloud;
#[cfg(feature = "pcloud")]
pub use pcloud::PCloudProvider;
```

### C. Provider Implementation
Create a new file `cloud_sync_lib/src/providers/pcloud.rs`:

1. Define the `PCloudProvider` struct.
2. Implement helper methods to:
   * Execute authenticated API requests, passing `Authorization: Bearer {access_token}` header.
3. Implement the `StorageBackend` trait methods:
   * **`upload`**:
     - Construct a multipart/form-data body containing the local file bytes.
     - Call `POST {endpoint}/uploadfile` passing target remote folder `path` and file name in parameters.
   * **`download`**:
     - Call `GET {endpoint}/getfilelink` with the target `path` parameter.
     - Parse the JSON response containing the list of download hosts (`hosts`) and the file path fragment (`path`).
     - Perform a `GET` request on `https://{hosts[0]}{path}` to download the raw bytes.
   * **`delete`**:
     - Call `GET {endpoint}/deletefile` with the `path` parameter.
   * **`list`**:
     - Call `GET {endpoint}/listfolder` with the folder `path` parameter.
     - Parse the files and folders returned in the JSON contents.

### D. Local Simulation Setup
Update `local_sim.rs` and the README to support `./cloud_simulation/pcloud` as a fallback path when credentials are not configured.

---

## 2. Daemon Integration Changes (`cloud_sync_daemon`)

### A. Configuration Parsing
Update `AppConfig` in `cloud_sync_daemon/src/config.rs` to include the configuration fields:
```rust
pub struct AppConfig {
    // ...
    pub pcloud_root: PathBuf,
    pub pcloud_credentials: Option<PCloudCredentials>,
}
```

### B. Startup Registration (`main.rs`)
In `cloud_sync_daemon/src/main.rs`, register the provider conditionally:
```rust
if is_pcloud_enabled(&config.pcloud_credentials) {
    let sync = config.pcloud_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
    let inner = config.pcloud_credentials.clone().map(PCloudProvider::new);
    let local_sim = LocalSimulation::new(config.pcloud_root.clone(), "pCloud".to_string());
    backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "pCloud", sync)));
}
```
Apply the same instantiation code in `control.rs` to support runtime configuration reloads.

---

## 3. Verification Plan

1. **Unit Tests**:
   - Write tests in `cloud_sync_lib/src/lib.rs` verifying local simulation mode for `PCloudProvider` works correctly without network connection.
