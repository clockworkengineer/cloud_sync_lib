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

### Phase 4 [COMPLETED]
1. **Unify HTTP Request Authorization Signing:** Defined `apply_bearer_auth` helper in `providers::utils` and refactored all HTTP providers to sign requests uniformly (`commit 437c0da`).
2. **Consolidate Parent & Filename Extraction:** Extracted `get_parent_and_filename` helper to `cloud_sync_core::path` and refactored Box provider and sync engine (`commit 6dad969`).

---

## Phase 5 Refactor Plan

### 1. Macroize Remote Path Formatting (`format_path`)
- **Problem:** Ten storage provider implementations (`webdav.rs`, `s3.rs`, `pcloud.rs`, `onedrive.rs`, `nextcloud.rs`, `ipfs.rs`, `gcs.rs`, `dropbox.rs`, `b2.rs`, `azure_blob.rs`) define duplicate private `format_path` helper methods to resolve `remote_path` with their respective `destination_folder` config setting, duplicating ~150 lines of boilerplate across files.
- **Refactor Plan:**
  1. Extend `impl_provider_builder!` macro in `crates/cloud_sync_lib/src/providers/utils.rs` to optionally accept path formatting behavior (`absolute` or `relative`) and generate the `format_path` helper block dynamically.
  2. Refactor all 10 providers to delegate `format_path` generation to the expanded macro.
