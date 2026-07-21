# DRY Refactor Plan for Cloud Sync Library

This document outlines the multi-phase DRY (Don't Repeat Yourself) refactoring roadmap across the `cloud_sync_lib` workspace to eliminate code duplication, streamline backend provider implementations, and improve long-term code maintainability.

---

## Completed Phases

### Phase 1 [COMPLETED]
1. **Unified `ProviderConfig` Trait:** Implemented on `BackendCredentials` across all providers (`commit 03801c2`).
2. **HTTP Client Pooling & Options:** Plumbed `timeout` and `custom_headers` across all HTTP storage providers (`commit c11f7db`).
3. **Box OAuth Token Refresh:** Centralized token refresh & rotation callback logic into `OAuthTokenManager` (`commit 650a504`).
4. **Rate Limiting Wrapper:** Created generic `RateLimitingBackend` wrapper and integrated it across backends (`commit 06623fb`).

### Phase 2 [COMPLETED]
1. **Unify Path Prefix Stripping:** Added `strip_destination_prefix()` to `cloud_sync_core::path` and applied across all providers (`commit f2b73be`).
2. **Consolidate URL Encoding Utilities:** Added RFC 3986 `url_encode()` and `url_encode_path()` to `cloud_sync_core::path` and removed duplicate private helpers (`commit 3342f3a`).
3. **Macroize Provider Builder & Constructor Boilerplate:** Implemented `impl_provider_builder!` macro across all 14 providers (`commit c8e5b33`).
4. **Consolidate Error Response Translation:** Added `translate_status_code_error()` helper and refactored S3 / HTTP status mappings (`commit ade88e8`).

### Phase 3 [COMPLETED]
1. **Unify Configuration Models & Parsing:** Embedded `ProviderCredentialsConfig` and `ProviderRootsConfig` in `AppConfig` and `BackupConfig` using `#[serde(flatten)]` (`commit 52695ce`).
2. **Macroize Standalone CLI Verifiers:** Created `define_verifier_binary!` macro to reduce CLI boilerplates down to single declarative invocations (`commit 0071d25`).
3. **Centralize Storage Backend Factory:** Implemented `cloud_sync_lib::create_backend` factory function and integrated it into backup runner (`commit 2211772`).
4. **Standardize Async File Upload Buffer Streams:** Extracted `copy_buffered` helper function and replaced manual copy loops in SFTP provider (`commit 9fc87a6`).

---

## Phase 4 Refactor Plan

### 1. Unify HTTP Request Authorization Signing
- **Problem:** Many HTTP storage providers (`dropbox.rs`, `google_drive.rs`, `onedrive.rs`, `box_provider.rs`) repeat similar code to request/apply OAuth Access Tokens or custom authorization headers to their outgoing HTTP request builders.
- **Refactor Plan:**
  1. Define a centralized helper `apply_auth_header(req: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder` in `providers::utils`.
  2. Refactor OAuth providers to use this helper for signing requests uniformly.

### 2. Consolidate Directory/Parent Path Retrieval
- **Problem:** Multiple providers and test binaries duplicate logic to parse parent folder paths from a given file path string or determine the file name (e.g. `path.parent().map(...).unwrap_or("")`).
- **Refactor Plan:**
  1. Extract a common helper `get_parent_and_filename(path: &str) -> (String, String)` in `cloud_sync_core::path`.
  2. Refactor sync engines and backends to consume this helper.
