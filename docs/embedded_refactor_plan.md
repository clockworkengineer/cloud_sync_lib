# Embedded Systems Optimization & Refactor Plan

This plan details a concrete roadmap for refactoring the `cloud_sync` workspace to run efficiently on resource-constrained embedded systems (such as the Raspberry Pi Zero, compute modules, or router/NAS devices).

---

## 1. Current Architecture Analysis

Currently, the workspace compiles into three main components:
- **`cloud_sync_lib`**: A library crate compiling all 7 cloud providers (Dropbox, GDrive, OneDrive, WebDAV, S3, SFTP, Nextcloud) and bringing in heavy async dependencies (`tokio`, `reqwest`, `ssh2`, `rust-s3`).
- **`cloud_sync_daemon`**: A background process that monitors directory changes using `notify` and runs continuous background synchronization loops.
- **`cloud_sync_ui`**: A separate Axum-based web server serving a dashboard and communicating with the daemon via local TCP loopback.

### Embedded System Pain Points:
1. **Memory Overhead**: Running two distinct binaries (Daemon + UI Web Server) doubles the base memory footprint.
2. **Binary Size**: Compiling all 7 cloud providers, their respective SDKs, cryptographic engines (`ring`), and TLS backends results in a large binary size (50MB+ unstripped).
3. **CPU/Disk Starvation**: Continuous watch loops and concurrent multi-provider uploads can spike CPU and block I/O, starving core embedded OS services.

---

## 2. Refactoring Proposals

### Proposal 1: Cargo Feature Flags for Provider Exclusions
Introduce Cargo features in `cloud_sync_lib/Cargo.toml` so users can compile only the exact backends they need (e.g., compile a build that *only* supports SFTP to back up to a local NAS, stripping out S3 and OAuth clients).

```toml
[features]
default = ["sftp", "webdav"]
s3 = ["dep:rust-s3", "dep:aws-creds"]
sftp = ["dep:ssh2"]
dropbox = []
google_drive = []
onedrive = []
nextcloud = []
```

### Proposal 2: Single-Binary Consolidation (Merged Daemon & UI)
Instead of running a separate `cloud_sync_ui` process, consolidate the Axum HTTP router directly inside the `cloud_sync_daemon` as an optional background thread. 
* This reduces the memory footprint by running a single process.
* Shared state can be accessed directly in memory (via `Arc<Mutex<DaemonState>>`) rather than sending string commands over a TCP loopback socket, eliminating socket overhead.

### Proposal 3: Dynamic I/O Throttling and CPU Niceness
- **Niceness Setting**: Allow setting process niceness (`libc::setpriority`) on startup so that the operating system prioritizes core system loops over sync tasks.
- **Throttled Uploads**: Limit read buffer sizes and insert micro-sleeps during the upload stream loop in providers (WebDAV, SFTP, S3) to limit network and disk bandwidth usage.

### Proposal 4: Low-Power Wake-on-Sync Mode (Cron-like Sync)
On low-power battery-operated embedded systems, continuous folder watching (`notify`) prevents the CPU from entering deep sleep states. 
- Implement a **Scheduled/Interval Sync Mode** that disables file-system watching completely.
- The daemon wakes up on a set interval (e.g., every 6 hours), runs a one-shot sync across backends, and immediately sleeps or exits.

---

## 3. Concrete Action Items

### Phase 1: Dependency & Binary Shrinking
- [ ] Implement cargo feature flags in `cloud_sync_lib/Cargo.toml` to gate S3, OAuth, and SFTP dependencies.
- [ ] Conditionally compile provider instantiations in `main.rs` and `control.rs` based on active feature flags (`#[cfg(feature = "s3")]`).

### Phase 2: Single-Process Consolidation
- [ ] Move the Axum route definitions and `serve_index` handler from `cloud_sync_ui` into a new module in `cloud_sync_daemon` (e.g., `cloud_sync_daemon::web_ui`).
- [ ] Provide an `--enable-ui` flag to the daemon. If present, run the Axum server concurrently in a `tokio::spawn` task.
- [ ] Deprecate the standalone `cloud_sync_ui` crate.

### Phase 3: Energy & Resource Controls
- [ ] Add an `interval_sync_mins` option in `config.toml`. If configured, disable the `notify` watcher and run task loops using `tokio::time::interval`.
- [ ] Add resource control parameters (`max_cpu_percent`, `upload_kbps_limit`) to configurations and apply limiters during transfer chunks.
