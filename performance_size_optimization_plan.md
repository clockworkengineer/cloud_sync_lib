# Size & Performance Optimization Plan for `cloud_sync_lib`

This plan identifies key performance bottlenecks and binary size drivers in the workspace, and outlines concrete refactoring strategies to optimize the library and daemon for resource-constrained environments.

---

## 1. Binary Size Reductions

### A. Cryptographic Library Consolidation
* **Problem**: The codebase currently imports `sha2`, `sha1`, `md5`, and `pbkdf2` separately. Additionally, `reqwest` and `rust-s3` pull in `ring` or other TLS-related crypto libraries. This results in multiple copies of hashing and cipher algorithms compiled into the final binary.
* **Solution**: 
  - Standardize all custom hashing (for checksums, signatures) on a single cryptographic provider (e.g., reuse `ring` which is already pulled in by TLS, or use `rustls`'s underlying provider).
  - Remove redundant crates (`sha2`, `sha1`, `md5`) from dependencies.

### B. Replace `quick-xml` with a Micro-Parser
* **Problem**: The `quick-xml` crate with `serialize` feature pulls in heavy Serde dependencies and complex deserialization routines, which are only used to parse simple WebDAV/S3 XML responses.
* **Solution**:
  - Swap `quick-xml` for a lightweight pull parser like `xmlparser` (zero allocations, extremely small footprint).
  - Alternatively, implement a simple parser for the specific WebDAV `<d:href>` and S3 `<Key>` tags needed.

### C. Large Timestamp Utilities
* **Problem**: The `chrono` crate is imported for basic calendar and timestamp calculations, adding substantial code size.
* **Solution**:
  - Replace `chrono` with the lightweight `time` crate, or perform simple timestamp math directly on epoch seconds using `std::time`.

---

## 2. Memory & Performance Tuning

### A. Buffer Optimization in Checksum Calculations
* **Problem**: In `checksum.rs`, `compute_dropbox_hash` allocates a `4MB` vector (`vec![0; 4 * 1024 * 1024]`) on every call. This causes high allocation latency and memory spikes. Other checksum functions allocate `8KB` on the stack (`[0; 8192]`), which risks stack overflow on microcontroller threads.
* **Solution**:
  - In `compute_dropbox_hash`, stream chunks using a much smaller buffer (e.g., `64KB`) and accumulate hash blocks in memory.
  - In all other checksum functions, reduce stack buffers to `1KB` or less, or use a heap-allocated/reusable buffer pool.

### B. Streaming Directory Traversal
* **Problem**: `watcher::scan_local_directory` reads the entire filesystem tree into a `HashMap` before syncing. For large folders, this consumes significant heap memory.
* **Solution**:
  - Implement a streaming/lazy iterator traversal using `walkdir` or standard `read_dir` streams.
  - Process files one-by-one or in pipeline chunks to keep memory usage flat ($O(1)$ heap overhead relative to directory size).

### C. Eliminate Optional/Unused Backends at Compile-Time
* **Problem**: Providers that are not active are still built into the binary unless explicit features are customized.
* **Solution**:
  - Clean up default features in workspace `Cargo.toml` so that only the necessary target backends are compiled for the embedded build target.
