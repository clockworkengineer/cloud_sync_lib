# Phase 2 DRY Refactor Plan

This document details the second phase of the concrete refactoring plan to eliminate remaining code duplication (DRY) in `cloud_sync_lib` and `cloud_sync_daemon`.

---

## 1. Identified Duplications

### A. Path Normalization & Prefix Stripping in Daemon
In `cloud_sync_daemon/src/main.rs`, the `handle_event` function listens to filesystem events and extracts relative paths for synchronization. Currently, the prefix stripping and path separator normalization (`.to_string_lossy().replace('\\', "/")`) are duplicated between:
- Create/Modify event handler
- Delete/Remove event handler

This can be unified into a single helper function:
```rust
fn get_remote_path(path: &Path, watch_dir: &Path) -> Option<String>
```

### B. HTTP Response Error Handling in Providers
Each provider (`GoogleDriveProvider`, `DropboxProvider`, and `OneDriveProvider`) contains duplicate code to check if an API HTTP request succeeded, read the response body or status code, and map it to a `StorageError::Provider(...)`:
- **OneDrive**:
  ```rust
  if !res.status().is_success() {
      let body = res.text().await.unwrap_or_default();
      return Err(StorageError::Provider(format!("Failed to upload to OneDrive: {}", body)));
  }
  ```
- **Dropbox**:
  ```rust
  if !res.status().is_success() {
      let body = res.text().await.unwrap_or_default();
      return Err(StorageError::Provider(format!("Failed to upload to Dropbox: {}", body)));
  }
  ```
- **Google Drive**:
  ```rust
  if !res.status().is_success() {
      return Err(StorageError::Provider(format!("Failed to download from Google Drive: {}", res.status())));
  }
  ```

We can extract a shared async helper function to convert non-success responses into `StorageError` inside `cloud_sync_lib/src/providers/utils.rs`:
```rust
pub async fn parse_response_error(res: reqwest::Response, provider: &str) -> StorageError
```

---

## 2. Proposed Changes

### Component: `cloud_sync_lib`
- **[`utils.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/utils.rs)**: Add `parse_response_error` function.
- **[`google_drive.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/google_drive.rs)**: Delegate REST API failure mapping to `parse_response_error`.
- **[`dropbox.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/dropbox.rs)**: Delegate REST API failure mapping to `parse_response_error`.
- **[`onedrive.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_lib/src/providers/onedrive.rs)**: Delegate REST API failure mapping to `parse_response_error`.

### Component: `cloud_sync_daemon`
- **[`main.rs`](file:///home/robt/projects/cloud_sync_lib/cloud_sync_daemon/src/main.rs)**: Add `get_remote_path` helper function and replace duplicated path extraction logic.

---

## 3. Verification Plan
- Run `cargo test --all` to verify that all mock and simulated tests continue to pass.
