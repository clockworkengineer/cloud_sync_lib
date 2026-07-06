# Concrete DRY Refactoring Plan for `cloud_sync_lib`

This document outlines a concrete, actionable plan to refactor the codebase to eliminate significant duplication across storage providers, credential structures, configuration handling, daemon initialization, and verification test scripts.

---

## 1. Identified Areas of Code Duplication

### A. Path Formatting Logic (`format_path`)
*   **Location**: `cloud_sync_lib/src/providers/*.rs`
*   **Duplication**: 10 storage providers (including S3, B2, GCS, OneDrive, Nextcloud, WebDAV, IPFS, pCloud, Dropbox, and Box) duplicate the logic for combining an optional `destination_folder` with the user-provided `remote_path`.
*   **Patterns**:
    1.  **Relative Formatting** (e.g., `dest_folder/path`): S3, B2, GCS, IPFS, Nextcloud, WebDAV.
    2.  **Absolute Formatting** (e.g., `/dest_folder/path`): Dropbox, pCloud, Box, OneDrive (depending on API flavor).

### B. Credentials Fields
*   **Location**: `cloud_sync_lib/src/providers/mod.rs`
*   **Duplication**: 11 credential structs (`OAuthCredentials`, `WebDAVCredentials`, `S3Credentials`, `SFTPCredentials`, `NextcloudCredentials`, `MegaCredentials`, `AzureBlobCredentials`, `GCSCredentials`, `B2Credentials`, `PCloudCredentials`, `IPFSCredentials`) all duplicate the same three optional settings fields:
    ```rust
    pub destination_folder: Option<String>,
    pub enabled: Option<bool>,
    pub sync: Option<bool>,
    ```

### C. Config Enablement Helpers
*   **Location**: `cloud_sync_daemon/src/config.rs`
*   **Duplication**: 11 separate functions (`is_enabled`, `is_webdav_enabled`, `is_s3_enabled`, `is_sftp_enabled`, etc.) exist to check if a provider is enabled. They all execute the exact same mapping logic:
    ```rust
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
    ```

### D. Backend Initialization Boilerplate
*   **Location**: `cloud_sync_daemon/src/main.rs` (inside `init_backends`)
*   **Duplication**: Massive repetitive `#[cfg(feature = "...")]` blocks compile-gate, extract, configure `LocalSimulation`, wrap with `SimulatedFallback`, and push into the `backends` vector for every single supported cloud provider.

### E. Diagnostic Verification Binaries
*   **Location**: `cloud_sync_daemon/src/bin/test_*.rs`
*   **Duplication**: Multiple testing files (e.g. `test_box.rs`, `test_mega.rs`, `test_nextcloud.rs`, `test_s3.rs`, `test_sftp.rs`, `test_webdav.rs`) contain highly repetitive command-line test loops (loading config -> checking credentials -> initializing provider -> listing directory -> uploading mock file -> downloading -> deleting).

---

## 2. Proposed DRY Refactoring Architecture

### A. Shared Path Formatting Utilities
We will add central, reusable path formatting helpers to [utils.rs](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/utils.rs):

```rust
/// Formats a relative remote path, incorporating an optional destination folder prefix.
pub fn format_relative_path(remote_path: &str, destination_folder: Option<&str>) -> String {
    let clean_path = remote_path.trim_start_matches('/');
    if let Some(dest_folder) = destination_folder {
        let clean_dest = dest_folder.trim_matches('/');
        if !clean_dest.is_empty() {
            if clean_path.is_empty() {
                return clean_dest.to_string();
            } else {
                return format!("{}/{}", clean_dest, clean_path);
            }
        }
    }
    clean_path.to_string()
}

/// Formats an absolute remote path starting with a slash, incorporating an optional destination folder prefix.
pub fn format_absolute_path(remote_path: &str, destination_folder: Option<&str>) -> String {
    let clean_path = remote_path.trim_start_matches('/');
    let mut full_path = String::new();
    if let Some(dest_folder) = destination_folder {
        let clean_dest = dest_folder.trim_matches('/');
        if !clean_dest.is_empty() {
            full_path.push('/');
            full_path.push_str(clean_dest);
        }
    }
    if !clean_path.is_empty() {
        full_path.push('/');
        full_path.push_str(clean_path);
    }
    full_path
}
```

### B. Shared Common Settings Struct with Serde Flattening
To clean up credentials structures, we will define a unified settings struct in [mod.rs](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/mod.rs):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonProviderSettings {
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
    /// Optional toggle to enable/disable deletion syncing.
    pub sync: Option<bool>,
}
```

We can then flatten this into all individual credentials structs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    #[serde(flatten)]
    pub common: CommonProviderSettings,
}
```
This preserves the configuration format (keeps it flat in TOML) but eliminates field definition duplication.

### C. Unified `ProviderConfig` Trait
We will declare a trait to access the common fields in a generic manner, solving both the configuration check duplication and backend initialization boilerplate:

```rust
pub trait ProviderConfig {
    fn common_settings(&self) -> &CommonProviderSettings;

    fn is_enabled(&self) -> bool {
        self.common_settings().enabled.unwrap_or(true)
    }

    fn sync_deletions(&self) -> bool {
        self.common_settings().sync.unwrap_or(true)
    }

    fn destination_folder(&self) -> Option<&str> {
        self.common_settings().destination_folder.as_deref()
    }
}
```

Implement this trait for all credentials structs:
```rust
impl ProviderConfig for OAuthCredentials {
    fn common_settings(&self) -> &CommonProviderSettings {
        &self.common
    }
}
// Repeat for WebDAVCredentials, S3Credentials, SFTPCredentials, etc.
```

Now, configuration check functions in [config.rs](file:///home/robt/projects/cloud_sync_lib/cloud_sync_daemon/src/config.rs) can be completely removed and replaced with a single generic function:

```rust
pub fn is_provider_enabled<C: ProviderConfig>(credentials: &Option<C>) -> bool {
    credentials.as_ref().map_or(true, |c| c.is_enabled())
}
```

### D. Simplified Backend Initialization in `main.rs`
With the unified traits, we can define a generic helper function in [main.rs](file:///home/robt/projects/cloud_sync_lib/cloud_sync_daemon/src/main.rs) to reduce boilerplate for each provider block:

```rust
fn try_add_backend<C, P, F>(
    backends: &mut Vec<Arc<dyn StorageBackend>>,
    creds_option: &Option<C>,
    sim_root: std::path::PathBuf,
    provider_name: &str,
    upload_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    download_limiter: Option<cloud_sync_lib::rate_limit::TokenBucket>,
    builder: F,
) where
    C: ProviderConfig + Clone + 'static,
    P: StorageBackend + 'static,
    F: FnOnce(C) -> P,
{
    if is_provider_enabled(creds_option) {
        let sync = creds_option.as_ref().map(|c| c.sync_deletions()).unwrap_or(true);
        let inner = creds_option.clone().map(builder);
        let local_sim = LocalSimulation::new(sim_root, provider_name.to_string())
            .with_limiters(upload_limiter, download_limiter);
        backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, provider_name, sync)));
    } else {
        info!("{} provider is disabled in configuration.", provider_name);
    }
}
```

Each backend configuration block in `init_backends` then shrinks down to a single clean call:

```rust
    #[cfg(feature = "google_drive")]
    try_add_backend(
        &mut backends,
        &config.google_credentials,
        config.google_drive_root.clone(),
        "Google Drive",
        upload_limiter.clone(),
        download_limiter.clone(),
        GoogleDriveProvider::new,
    );
```

### E. Reusable Diagnostic Test Harness
Create a helper function in a test support module or inside `cloud_sync_daemon/src/utils.rs` that accepts a `StorageBackend` and runs the standard suite of validation steps:
1. Directory listing.
2. Local temp file generation and upload.
3. Fetching and comparing directory lists.
4. Downloading the file and validating content hash/match.
5. Deleting the file and verifying cleanup.

This will reduce the files `test_sftp.rs`, `test_mega.rs`, `test_s3.rs`, etc. down to simple entry points.

---

## 3. Step-by-Step Implementation Guide

1.  **Refactor Credentials**: Update `cloud_sync_lib/src/providers/mod.rs` to declare `CommonProviderSettings` and embed it in all credential structs with `#[serde(flatten)]`.
2.  **Declare `ProviderConfig`**: Implement `ProviderConfig` for all credential structs in `mod.rs`.
3.  **Implement Path Formatting Helpers**: Add `format_relative_path` and `format_absolute_path` to `utils.rs` and update each provider to use them.
4.  **Simplify Config Helpers**: In `cloud_sync_daemon/src/config.rs`, delete all 11 repetitive `is_*_enabled` functions and add the generic `is_provider_enabled`.
5.  **Simplify Backend Init**: Refactor `cloud_sync_daemon/src/main.rs` to use the `try_add_backend` generic helper.
6.  **Create Test Harness**: Create the reusable validation sequence helper and refactor the test binaries.

---

## 4. Verification Plan

*   **Compilation**: Run `cargo check` and `cargo test` to ensure everything compiles correctly and all features (or feature subsets) build without issue.
*   **Integration Tests**: Run the existing sync suite to ensure the path resolution behavior remains exactly identical.
*   **Backward Compatibility**: Ensure configuration loading continues to parse standard `config.toml` files properly (verifying `#[serde(flatten)]` behaves as expected).
