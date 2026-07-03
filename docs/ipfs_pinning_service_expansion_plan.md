# IPFS / Pinning Service Integration & Expansion Plan

This document outlines a standardized, concrete technical implementation plan for adding **IPFS / Pinning Service** (such as Pinata) support to the cloud sync workspace.

---

## 1. Core Library Changes (`cloud_sync_lib`)

### A. Dependencies
IPFS Pinning API uses HTTPS JSON endpoints. Standard multipart form data is used for pinning uploads. No extra crates are required, as `reqwest` and `serde` are fully capable.

### B. Configuration Struct
Add the credential configuration struct in [`cloud_sync_lib/src/providers/mod.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/mod.rs):
```rust
/// Credentials configuration for IPFS Pinning Service (e.g. Pinata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IPFSCredentials {
    /// JWT Bearer Token for authorization.
    pub jwt_token: String,
    /// Custom API endpoint (optional, defaults to Pinata's API https://api.pinata.cloud).
    pub endpoint: Option<String>,
    /// Gateway URL to resolve pinned CIDs (optional, defaults to https://gateway.pinata.cloud/ipfs/).
    pub gateway_url: Option<String>,
    /// Optional prefix folder/label for sync mapping.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional toggle to enable/disable deletion (unpinning) syncing.
    pub sync: Option<bool>,
}
```

Make sure to conditionally compile the module and re-export the provider:
```rust
#[cfg(feature = "ipfs")]
pub mod ipfs;
#[cfg(feature = "ipfs")]
pub use ipfs::IPFSProvider;
```

### C. Provider Implementation
Create a new file `cloud_sync_lib/src/providers/ipfs.rs`:

1. Define the `IPFSProvider` struct.
2. Implement helper methods to:
   * Perform authenticated requests passing `Authorization: Bearer {jwt_token}`.
   * Query the CID of a remote path using `GET {endpoint}/data/pinList?status=pinned&metadata[name]={remote_path}`.
3. Implement the `StorageBackend` trait methods:
   * **`upload`**:
     - Construct a multipart/form-data body.
     - Include the file bytes under part `file`.
     - Include a JSON string under part `pinataMetadata` containing `{"name": "{remote_path}"}`.
     - Send `POST {endpoint}/pinning/pinFileToIPFS`.
   * **`download`**:
     - Retrieve the `IpfsHash` (CID) of the target file path via the `/data/pinList` query helper.
     - Fetch the file bytes via `GET {gateway_url}/{IpfsHash}`.
   * **`delete`**:
     - Retrieve the `IpfsHash` (CID) of the target file path.
     - Call `DELETE {endpoint}/pinning/unpin/{IpfsHash}`.
   * **`list`**:
     - Call `GET {endpoint}/data/pinList?status=pinned`.
     - Extract filenames, sizes, and pin dates from JSON.

### D. Local Simulation Setup
Update `local_sim.rs` and the README to support `./cloud_simulation/ipfs` as a fallback path when credentials are not configured.

---

## 2. Daemon Integration Changes (`cloud_sync_daemon`)

### A. Configuration Parsing
Update `AppConfig` in `cloud_sync_daemon/src/config.rs` to include the configuration fields:
```rust
pub struct AppConfig {
    // ...
    pub ipfs_root: PathBuf,
    pub ipfs_credentials: Option<IPFSCredentials>,
}
```

### B. Startup Registration (`main.rs`)
In `cloud_sync_daemon/src/main.rs`, register the provider conditionally:
```rust
if is_ipfs_enabled(&config.ipfs_credentials) {
    let sync = config.ipfs_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
    let inner = config.ipfs_credentials.clone().map(IPFSProvider::new);
    let local_sim = LocalSimulation::new(config.ipfs_root.clone(), "IPFS".to_string());
    backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "IPFS", sync)));
}
```
Apply the same instantiation code in `control.rs` to support runtime configuration reloads.

---

## 3. Verification Plan

1. **Unit Tests**:
   - Write tests in `cloud_sync_lib/src/lib.rs` verifying local simulation mode for `IPFSProvider` works correctly without network connection.
