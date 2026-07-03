# Google Cloud Storage (GCS) Integration & Expansion Plan

This document outlines a standardized, concrete technical implementation plan for adding **Google Cloud Storage (GCS)** support to the cloud sync workspace.

---

## 1. Core Library Changes (`cloud_sync_lib`)

### A. Dependencies
Google Cloud Storage uses JSON API over HTTPS with Service Account OAuth2 credentials. We can use `yup-oauth2` for service account token generation:
```toml
[dependencies]
yup-oauth2 = { version = "8.3", optional = true }
```
Add a new cargo feature `gcs` that enables this dependency.

### B. Configuration Struct
Add the credential configuration struct in [`cloud_sync_lib/src/providers/mod.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/mod.rs):
```rust
/// Credentials configuration for Google Cloud Storage (GCS).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCSCredentials {
    /// Target Google Cloud Storage bucket name.
    pub bucket: String,
    /// Absolute path to the Service Account JSON credentials key file.
    pub service_account_key_path: String,
    /// Custom endpoint URL (optional, used for local fake-gcs-server emulator).
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
#[cfg(feature = "gcs")]
pub mod gcs;
#[cfg(feature = "gcs")]
pub use gcs::GCSProvider;
```

### C. Provider Implementation
Create a new file `cloud_sync_lib/src/providers/gcs.rs`:

1. Define the `GCSProvider` struct.
2. If `service_account_key_path` is not empty, use `yup-oauth2::ServiceAccountAuthenticator` to retrieve short-lived OAuth access tokens for the scope `https://www.googleapis.com/auth/devstorage.full_control`.
3. Implement the `StorageBackend` trait methods mapping to GCS JSON REST API operations:
   * **`upload`**: Call simple upload `POST /upload/storage/v1/b/{bucket}/o?uploadType=media&name={object_path}` with the OAuth `Authorization: Bearer <token>` header.
   * **`download`**: Call `GET /storage/v1/b/{bucket}/o/{object_path}?alt=media`.
   * **`delete`**: Call `DELETE /storage/v1/b/{bucket}/o/{object_path}`.
   * **`list`**: Call `GET /storage/v1/b/{bucket}/o?prefix={prefix}` and parse the JSON response items array (containing size and updated time) to map to `StorageItem` structs.

### D. Local Simulation Setup
Update `local_sim.rs` and the README to support `./cloud_simulation/gcs` as a fallback path when credentials are not configured.

---

## 2. Daemon Integration Changes (`cloud_sync_daemon`)

### A. Configuration Parsing
Update `AppConfig` in `cloud_sync_daemon/src/config.rs` to include the configuration fields:
```rust
pub struct AppConfig {
    // ...
    pub gcs_root: PathBuf,
    pub gcs_credentials: Option<GCSCredentials>,
}
```

### B. Startup Registration (`main.rs`)
In `cloud_sync_daemon/src/main.rs`, register the provider conditionally:
```rust
if is_gcs_enabled(&config.gcs_credentials) {
    let sync = config.gcs_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
    let inner = config.gcs_credentials.clone().map(GCSProvider::new);
    let local_sim = LocalSimulation::new(config.gcs_root.clone(), "GCS".to_string());
    backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "GCS", sync)));
}
```
Apply the same instantiation code in `control.rs` to support runtime configuration reloads.

---

## 3. Verification Plan

1. **Unit Tests**:
   - Write tests in `cloud_sync_lib/src/lib.rs` verifying local simulation mode for `GCSProvider` works correctly without network connection.
2. **Integration / Emulator Verification**:
   - Spin up `fake-gcs-server` locally via Docker.
   - Run the integration test suite configured against the local fake-gcs-server emulator endpoint (`http://127.0.0.1:4443`).
