# DRY Refactor Plan for `cloud_sync_lib`

This document details a concrete refactoring plan to eliminate code duplication (DRY) across the workspace library (`cloud_sync_lib`) and daemon (`cloud_sync_daemon`).

---

## 1. Identified Areas of Duplication

### A. Local Fallback Simulation
Each of the three providers (`GoogleDriveProvider`, `DropboxProvider`, `OneDriveProvider`) repeats identical filesystem logic when operating in simulation mode (`credentials` is `None`):
* **Path Resolution**: `resolve()` maps a remote path to a local simulation directory.
* **Simulated Operations**:
  * `upload`: Directory creation (`fs::create_dir_all`) and file copy (`fs::copy`).
  * `download`: File existence check, directory creation, and file copy.
  * `delete`: Folder or file removal (`fs::remove_dir_all` / `fs::remove_file`).
  * `list`: Directory iteration and file metadata extraction.

### B. OAuth2 Token Refresh Flow
The `get_access_token` method in each provider duplicates:
* Packaging credentials (`client_id`, `client_secret`, `refresh_token`, `grant_type = "refresh_token"`) as form parameters.
* Making a `POST` request to the token URL using `reqwest`.
* Extracting `"access_token"` from the JSON response and formatting authentication error responses.

### C. Backend Enabling Check in Daemon
The daemon (`cloud_sync_daemon`) repeats the helper wrapper mapping for each of the three config credential options to determine if they are enabled:
```rust
let drive_enabled = config.google_credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true));
```

---

## 2. Proposed Concrete Refactoring Steps

### Step 1: Create a Shared `LocalSimulation` Helper Client
We can extract local simulation logic into a reusable struct or helper inside `cloud_sync_lib/src/providers/local_sim.rs`:

```rust
use std::path::{Path, PathBuf};
use crate::traits::{StorageItem, StorageError};
use tokio::fs;

pub struct LocalSimulation {
    root_dir: PathBuf,
    provider_name: String,
}

impl LocalSimulation {
    pub fn new(root_dir: PathBuf, provider_name: String) -> Self {
        Self { root_dir, provider_name }
    }

    pub fn resolve(&self, remote_path: &str) -> PathBuf {
        let normalized = remote_path.trim_start_matches('/');
        self.root_dir.join(normalized)
    }

    pub async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> { ... }
    pub async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> { ... }
    pub async fn delete(&self, remote_path: &str) -> Result<(), StorageError> { ... }
    pub async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> { ... }
}
```

Each provider will delegate to an internal `LocalSimulation` instance when `credentials.is_none()`.

### Step 2: Implement a Common OAuth2 Token Helper
Extract token refreshing into a common utility function in `cloud_sync_lib/src/providers/utils.rs`:

```rust
use crate::traits::StorageError;

pub async fn refresh_oauth2_token(
    client: &reqwest::Client,
    auth_url: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
    provider_name: &str,
) -> Result<String, StorageError> {
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    let res = client.post(auth_url)
        .form(&params)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let token = res["access_token"].as_str().ok_or_else(|| {
        StorageError::Authentication(format!("Failed to retrieve {} access token: {:?}", provider_name, res))
    })?;

    Ok(token.to_string())
}
```

This replaces the redundant `get_access_token` boilerplate inside each client.

### Step 3: Add Config Helper in Daemon
Consolidate toggle verification by adding a simple helper function in `cloud_sync_daemon/src/main.rs`:

```rust
fn is_enabled(credentials: &Option<OAuthCredentials>) -> bool {
    credentials.as_ref().map_or(true, |c| c.enabled.unwrap_or(true))
}
```

This reduces the repetitive `.map_or(...)` invocations in daemon provider setup.

---

## 3. Expected Benefits
* **Maintainability**: Modifying simulation file operations (e.g. adding debouncing or directory syncs) only needs to be updated in one place.
* **Code Size**: Reduces code volume across provider files by approximately ~150-200 lines of repetitive boilerplates.
* **Extensibility**: Adding new OAuth2-based backends (like Box or WebDAV) will be faster, as they can reuse these generic helpers instantly.
