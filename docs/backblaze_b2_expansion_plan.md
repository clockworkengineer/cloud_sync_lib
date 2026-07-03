# Backblaze B2 Storage Integration & Expansion Plan

This document outlines a standardized, concrete technical implementation plan for adding **Backblaze B2** support to the cloud sync workspace.

---

## 1. Core Library Changes (`cloud_sync_lib`)

### A. Dependencies
Backblaze B2 uses JSON API over HTTPS with standard OAuth2 / basic credential exchange. No extra crates are required, as `reqwest` and `serde_json` are already available. We can compute SHA1 hash using the existing `sha1` or `sha2` crates if necessary, or pass `do_not_verify` in `X-Bz-Content-Sha1` to keep it simple.

### B. Configuration Struct
Add the credential configuration struct in [`cloud_sync_lib/src/providers/mod.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/mod.rs):
```rust
/// Credentials configuration for Backblaze B2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct B2Credentials {
    /// Target Backblaze B2 bucket name.
    pub bucket: String,
    /// Backblaze B2 Key ID.
    pub key_id: String,
    /// Backblaze B2 Application Key.
    pub application_key: String,
    /// Custom endpoint URL (optional, used for mocking / alternate endpoints).
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
#[cfg(feature = "b2")]
pub mod b2;
#[cfg(feature = "b2")]
pub use b2::B2Provider;
```

### C. Provider Implementation
Create a new file `cloud_sync_lib/src/providers/b2.rs`:

1. Define the `B2Provider` struct.
2. Implement helper methods to:
   * Authenticate against `b2_authorize_account` using Basic authentication (Key ID and Application Key). Keep the authorization response cached (including `authorizationToken`, `apiUrl`, `downloadUrl`, and `accountId`).
   * Retrieve the `bucketId` by querying `b2_list_buckets` if not already cached.
3. Implement the `StorageBackend` trait methods:
   * **`upload`**:
     - Call `b2_get_upload_url` to get an upload endpoint and token.
     - Send a `POST` request to the upload endpoint with `X-Bz-File-Name` (URL encoded), `X-Bz-Content-Sha1` set to `do_not_verify` (or computed SHA1), and the file body.
   * **`download`**:
     - Call `GET {downloadUrl}/file/{bucketName}/{fileName}` using the cached `authorizationToken`.
   * **`delete`**:
     - B2 API deletion requires both `fileName` and `fileId`.
     - Query `b2_list_file_names` with `startFileName = {fileName}` and `maxFileCount = 1`.
     - Extract the matching `fileId` and call `b2_delete_file_version` with `fileName` and `fileId`.
   * **`list`**:
     - Call `b2_list_file_names` with `prefix` parameter and extract sizes and upload timestamps.

### D. Local Simulation Setup
Update `local_sim.rs` and the README to support `./cloud_simulation/b2` as a fallback path when credentials are not configured.

---

## 2. Daemon Integration Changes (`cloud_sync_daemon`)

### A. Configuration Parsing
Update `AppConfig` in `cloud_sync_daemon/src/config.rs` to include the configuration fields:
```rust
pub struct AppConfig {
    // ...
    pub b2_root: PathBuf,
    pub b2_credentials: Option<B2Credentials>,
}
```

### B. Startup Registration (`main.rs`)
In `cloud_sync_daemon/src/main.rs`, register the provider conditionally:
```rust
if is_b2_enabled(&config.b2_credentials) {
    let sync = config.b2_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
    let inner = config.b2_credentials.clone().map(B2Provider::new);
    let local_sim = LocalSimulation::new(config.b2_root.clone(), "B2".to_string());
    backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "B2", sync)));
}
```
Apply the same instantiation code in `control.rs` to support runtime configuration reloads.

---

## 3. Verification Plan

1. **Unit Tests**:
   - Write tests in `cloud_sync_lib/src/lib.rs` verifying local simulation mode for `B2Provider` works correctly without network connection.
