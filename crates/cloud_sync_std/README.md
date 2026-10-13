# `cloud_sync_std`

Standard library extensions, cryptographic hashing utilities, and exclusion matchers for `cloud_sync_lib`.

---

## 1. Overview

`cloud_sync_std` builds upon `cloud_sync_core` by providing concrete implementations that depend on the Rust standard library (`std`), asynchronous I/O (`tokio`), and cryptographic hashing (`ring`, `md5`).

It is used primarily by `cloud_sync_lib` and `cloud_sync_daemon` for file hashing, checksum verification, `.gitignore` / `.syncignore` pattern filtering, and filesystem traversal.

---

## 2. Key Modules & Utilities

### 2.1 Cryptographic Checksums (`checksum`)
Computes file hashes without stalling the asynchronous Tokio executor by delegating sequential disk reads and digest updates to `tokio::task::spawn_blocking`:

- **`compute_sha256(path: &Path) -> io::Result<String>`**: Computes standard 256-bit hexadecimal SHA-256 digests using `ring`. Used for internal file change detection and Google Drive checksum verification.
- **`compute_md5(path: &Path) -> io::Result<String>`**: Computes MD5 digests for compatibility with Dropbox and AWS S3 etags.

```rust
use cloud_sync_std::checksum::compute_sha256;
use std::path::Path;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let hash = compute_sha256(Path::new("watched_folder/document.pdf")).await?;
    println!("SHA-256: {}", hash);
    Ok(())
}
```

### 2.2 Ignore & Exclusion Engine (`SyncIgnore`)
Wraps the `ignore` crate (the same engine powering Ripgrep) to provide performant, hierarchy-aware pattern matching against `.syncignore` or `.gitignore` rules:

```rust
use cloud_sync_std::SyncIgnore;
use std::path::Path;

let mut matcher = SyncIgnore::new(Path::new("watched_folder"));
matcher.add_custom_patterns(&["*.tmp", "target/", ".git/"]);

if matcher.is_ignored(Path::new("watched_folder/target/debug/build.log")) {
    println!("Skipping ignored file!");
}
```

Features:
- Respects nested `.gitignore` and `.syncignore` files inside subdirectories.
- Automatically ignores OS system files (e.g., `desktop.ini`, `.DS_Store`, `Thumbs.db`).
- Supports glob wildcards, directory-only matches, and negation patterns (`!pattern`).

---

## 3. Dependencies

- **`cloud_sync_core`**: Trait definitions and core models.
- **`ring`**: High-performance cryptographic hashing (SHA-256).
- **`md5`**: MD5 digest calculation for cloud providers.
- **`ignore`**: Regex-free fast gitignore path matcher.
- **`tokio`**: Asynchronous filesystem and blocking threadpool execution.
