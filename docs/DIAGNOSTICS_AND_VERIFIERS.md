# Diagnostics & Verifiers Reference

**Project**: `cloud_sync_lib`  
**Applies to**: `cloud_sync_daemon` diagnostic binaries, provider verification harnesses

---

## 1. Overview

`cloud_sync_lib` provides a dedicated suite of standalone diagnostic binaries and test harnesses to rapidly verify credentials, network connectivity, authentication states, and basic read/write capabilities across all supported cloud storage providers without having to boot the entire daemon or UI stack.

---

## 2. Global Backend Status Checker (`check_backends`)

The `check_backends` diagnostic binary inspects your active `private_config.toml` (or `config.toml`), enumerates all enabled cloud backends, and tests directory listing connectivity against each one.

### Running `check_backends`
```bash
cargo run --bin check_backends -p cloud_sync_daemon
```

### Sample Output
```text
Checking status of configured backends...

Google Drive: YES
Dropbox: YES
OneDrive: NO (Authentication error: 401 Unauthorized)
MEGA: NO (MEGA error: blocked (-16))
S3: YES
```

If a backend fails, the utility prints the exact error description and status code to guide immediate resolution.

---

## 3. Dedicated Provider Verifiers (`test_*`)

For each of the cloud storage providers, a dedicated verification binary is provided in `crates/cloud_sync_daemon/src/bin/`. These utilities execute a comprehensive 3-step diagnostic lifecycle:
1. **Remote Directory Listing**: Fetches root metadata and previews remote files/folders.
2. **Upload Test**: Creates a local timestamped test probe file and uploads it to the remote backend (`test_connectivity_check.txt`).
3. **Deletion & Cleanup**: Removes the remote probe file and purges the temporary local artifact.

### Available Provider Verifier Binaries

| Binary Name | Target Provider | Cargo Command | Required Feature Flag |
| :--- | :--- | :--- | :--- |
| `test_mega` | MEGA Cloud | `cargo run --bin test_mega -p cloud_sync_daemon --features mega` | `mega` |
| `test_box` | Box.com | `cargo run --bin test_box -p cloud_sync_daemon --features box` | `box` |
| `test_s3` | Amazon S3 / MinIO | `cargo run --bin test_s3 -p cloud_sync_daemon --features s3` | `s3` |
| `test_sftp` | SSH / SFTP Server | `cargo run --bin test_sftp -p cloud_sync_daemon --features sftp` | `sftp` |
| `test_webdav` | Generic WebDAV | `cargo run --bin test_webdav -p cloud_sync_daemon --features webdav` | `webdav` |
| `test_nextcloud` | Nextcloud Server | `cargo run --bin test_nextcloud -p cloud_sync_daemon --features nextcloud` | `nextcloud` |

---

## 4. How Verifier Binaries Work (`define_verifier_binary!`)

All dedicated provider verifier binaries are generated using the declarative macro `define_verifier_binary!` in `crates/cloud_sync_daemon/src/bin/common.rs`. This ensures uniform test flows, standardized error formatting, and clean temporary file handling:

```rust,no_run
// Example: crates/cloud_sync_daemon/src/bin/test_mega.rs
#[path = "common.rs"]
pub mod common;

cloud_sync_daemon::define_verifier_binary!(
    "MEGA",
    "mega",
    mega_credentials,
    MegaProvider,
    "mega_test.tmp"
);
```

### Execution Flow:
1. Reads `private_config.toml` (or `config.toml`).
2. Checks whether the provider credentials exist and `enabled = true`.
3. Instantiates the provider with configured credentials.
4. Calls `common::run_connection_diagnostics(provider, temp_file).await`.
5. Prints clear actionable diagnostic tips if authentication or account block errors occur (e.g. `e.is_auth()`).

---

## 5. Contract Compliance Testing Harness

To verify that third-party, custom, or mock providers strictly comply with the domain contract invariants required by `cloud_sync_core`, the test harness exposes:

```rust,no_run
use cloud_sync_core::testing::verify_backend_compliance;
use std::sync::Arc;

#[tokio::test]
async fn test_custom_backend_compliance() {
    let backend = Arc::new(MyCustomProvider::new());
    verify_backend_compliance(backend).await.expect("Provider failed compliance suite");
}
```

The compliance harness verifies:
- Path normalization handling (refusing `..` and `./` sequences).
- Handling of non-existent files during `download()` and `delete()`.
- Idempotent folder creation.
- Checksum integrity after roundtrip uploads.

