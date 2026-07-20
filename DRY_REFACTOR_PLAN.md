# DRY Refactoring Plan: CloudSync Library

This document outlines the concrete plan to reduce duplication (adhering to the **Don't Repeat Yourself** principle) across the various components of the CloudSync workspace.

---

## 1. Identified Areas of Duplication

### A. OAuth Token Retrieval & Cache Management
- **Status**: Duplicate logic in `google_drive.rs`, `dropbox.rs`, `onedrive.rs`, `box_provider.rs`, etc.
- **Details**: Each provider defines a `token_cache: Mutex<Option<(String, Instant)>>` and duplicates the workflow of:
  1. Reading cached tokens.
  2. Checking token lifetime/expiry.
  3. Making a POST refresh request to the OAuth endpoint.
  4. Updating the local token cache.

### B. Storage HTTP Response Error Parsing (`parse_response_error`)
- **Status**: Every HTTP-based provider duplicates a `parse_response_error` function.
- **Details**: `parse_response_error` matches HTTP status codes (e.g. 401, 403, 404, 429) to core `StorageError` variants and reads response bodies. This exists across S3, GCS, Azure Blob, Dropbox, Box, Google Drive, OneDrive, WebDAV, etc.

### C. Provider Construction and Fallback Logic
- **Status**: Duplicate backend builders in `cloud_sync_backup` (`build_backend`) and the daemon backend resolver.
- **Details**: Both packages repeat construction of real backends wrapped in `SimulatedFallback`.

---

## 2. Refactoring Proposal

### Step 1: Centralized OAuth Token Manager
Introduce an `OAuthTokenManager` helper structure inside `crates/cloud_sync_lib/src/providers/utils.rs` or as a standalone module:
```rust
pub struct OAuthTokenManager {
    client: reqwest::Client,
    token_url: String,
    client_id: String,
    client_secret: String,
    refresh_token: String,
    cache: tokio::sync::RwLock<Option<(String, std::time::Instant)>>,
}

impl OAuthTokenManager {
    pub fn new(token_url: &str, client_id: &str, client_secret: &str, refresh_token: &str) -> Self;
    pub async fn get_access_token(&self) -> Result<String, StorageError>;
}
```
All OAuth providers will store an `Arc<OAuthTokenManager>` instead of individual credentials and cached tokens.

### Step 2: Unify HTTP Error Translation
Create a generic `translate_http_error` function in `crates/cloud_sync_lib/src/providers/utils.rs`:
```rust
pub async fn translate_http_error(
    res: reqwest::Response, 
    provider_name: &str, 
    operation: &str
) -> StorageError;
```
This handles text/JSON body extraction and maps error statuses to unified `StorageError` codes (e.g., `Authentication`, `RateLimited`, `NotFound`, etc.).

### Step 3: Centralize Backend Registry & Builder
Expose a unified backend builder inside `cloud_sync_lib` (e.g. `BackendRegistry`) so both `cloud_sync_backup` and `cloud_sync_daemon` call:
```rust
pub fn create_backend(
    provider_name: &str,
    config: &ProviderConfig
) -> Result<Arc<dyn StorageBackend>, Box<dyn std::error::Error>>;
```
This avoids duplicating compile flags (`#[cfg(feature = "...")]`) and matching patterns across multiple crates.

---

## 3. Impact Assessment
- **Lines of Code (LoC) Reduced**: ~400-600 lines of boilerplate removed.
- **Maintenance**: Adding new OAuth-based storage backends will require significantly less code since token refreshing and error mapping will be handled automatically.
