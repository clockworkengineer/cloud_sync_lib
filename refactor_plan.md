# Refactoring Plan: Size Reduction and Performance Optimization

This plan outlines concrete steps to significantly reduce compilation times, binary sizes, and memory usage, while substantially improving the synchronization performance of `cloud_sync_lib`.

---

## 1. Binary Size & Compile-Time Reduction

### A. Fix Feature Gate Leakage in `cloud_sync_lib`
Currently, `crates/cloud_sync_lib/src/providers/mod.rs` includes `pub mod dropbox;` and others without proper conditional compilation (`#[cfg(feature = "...")]`). This means the compiler parses and compiles these providers even when their feature flags are disabled.
- **Action**: Add `#[cfg(feature = "dropbox")]` to `pub mod dropbox;` in `mod.rs`.
- **Action**: Audit all other modules in `crates/cloud_sync_lib/src/providers/mod.rs` to ensure they are strictly gated behind their respective feature flags.

### B. Unify Workspace Dependencies
Currently, the root `Cargo.toml` does not use the `[workspace.dependencies]` table. Individual crates repeat dependency specifications with minor version discrepancies (e.g. `tempfile = "3"` vs `tempfile = "3.8"`), which increases duplicate compilation steps.
- **Action**: Move all shared dependencies (`tokio`, `serde`, `serde_json`, `async-trait`, `tempfile`, `tracing`, `ignore`, `futures-util`) to the root `Cargo.toml` under `[workspace.dependencies]`.
- **Action**: Update all member crates' `Cargo.toml` to inherit these versions using `{ workspace = true }`.

### C. Remove / Prune Unused Dependencies
- **Action**: Use `cargo-udeps` or manually audit imports to ensure heavy libraries like `postcard`, `xmlparser`, `base64`, `aes-gcm`, or others are not compiled unless required by the active feature sets.

---

## 2. Sync Performance Optimization

### A. Increase Buffer Size for Checksum Computation
In [checksum.rs](file:///home/robt/projects/cloud_sync_lib/crates/cloud_sync_std/src/checksum.rs), functions like `compute_sha256`, `compute_md5`, and `compute_sha1` read files using a tiny buffer size of **1 KB** (`let mut buffer = [0; 1024];`).
- **Impact**: Reading large files in 1 KB chunks issues an excessive number of asynchronous I/O system calls and async yield points, severely degrading performance.
- **Action**: Increase the buffer size to **64 KB** (`[0; 65536]`) or **128 KB** to minimize I/O system call overhead.
- **Action**: Offload hash computation for large files to `tokio::task::spawn_blocking` to avoid blocking the main Tokio executor threads during CPU-intensive hashing.

### B. Parallelize Checksum Calculations during Scanner Loop
In [sync_engine.rs](file:///home/robt/projects/cloud_sync_lib/crates/cloud_sync_daemon/src/sync_engine.rs#L941-L955), `backend.compute_local_checksum` is called sequentially inside the scanning loop:
```rust
while let Some((rel_path, item)) = scanner.next().await? {
    let checksum = if item.is_dir { None } else {
        backend.compute_local_checksum(&item.path).await.ok().flatten()
    };
    ...
}
```
- **Impact**: The sync engine is blocked from proceeding until all files are hashed sequentially.
- **Action**: Scan the directory first, gather the list of files, and then compute checksums concurrently using `futures_util::stream::iter` and `.buffer_unordered(max_concurrency)`.

### C. Optimize Memory Allocations and HashMap Cloning
In [sync_engine.rs](file:///home/robt/projects/cloud_sync_lib/crates/cloud_sync_daemon/src/sync_engine.rs#L1206-L1212), `sync_state.files.clone()` is cloned before creating the file synchronization tasks, and each task obtains owned clones of paths/states.
- **Impact**: Cloning large collections of string keys and state structs consumes excessive CPU cycles and memory for large folders.
- **Action**: Keep the directories/files maps in an `Arc` or reference them, and lookup states only when executing the task, or pass only relevant minimal slices to avoid copying the entire lookup tables.
