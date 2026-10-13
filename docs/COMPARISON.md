# Product Comparison: Cloud Sync Daemon vs. Alternatives

A detailed comparison of this Rust-based Cloud Sync Workspace against established industry alternatives, both free/open-source and commercial.

---

## 1. Industry Alternatives

| Software | License / Cost | Core Focus | Primary Interface |
| :--- | :--- | :--- | :--- |
| **Rclone** | Free & Open Source | Multi-backend syncing | CLI / Web GUI proxy |
| **FreeFileSync** | Free & Open Source | Desktop folder mirroring | GUI (Desktop App) |
| **Syncthing** | Free & Open Source | Peer-to-peer folder sync | Web GUI |
| **GoodSync** | Paid (Subscription) | Enterprise replication | GUI / Service |
| **Insync** | Paid (One-time license) | Desktop cloud clients | GUI (Desktop App) |

---

## 2. Strengths & Advantages of This Project

- **Ultra-Low Memory Footprint (Rust)**: Written in Rust, the daemon consumes minimal memory and system resources, making it ideal for running on NAS systems, routers, and lightweight home servers.
- **Decoupled Architecture**: Separation between the core CLI daemon (via TCP socket control) and the Axum-based Web UI proxy allows running the daemon headless while controlling it remotely.
- **Immediate Event-Driven Sync**: Uses the native OS filesystem event loop (via the `notify` crate) to propagate changes the millisecond a write completes, rather than relying on heavy scheduled polling cycles.
- **Simple, Modular Codebase**: The generic `StorageBackend` trait makes introducing new providers (or mock fallbacks) simple, keeping the codebase easily maintainable.
- **Two-Way Synchronization**: Fully supports bidirectional synchronization with automated collision resolution, keeping local directories and cloud backends in lockstep.
- **Zero-Knowledge Encryption**: Optional client-side AES-256-GCM encryption ensures data is encrypted locally before being transmitted to the cloud.
- **Bandwidth Rate-Limiting**: Flexible upload and download rate limiting via Token Buckets helps prevent network congestion.
- **Broad Backend Support**: Integrates out-of-the-box with a wide array of cloud, enterprise, object, and distributed protocols (14 distinct providers).

---

## 3. Current Limitations

- **Chunked Uploads**: Lacks native support for multi-gigabyte chunked/resumable uploads on some providers (files are uploaded in one pass).
- **Move/Rename Detection**: File rename or move actions are handled as a delete followed by a new upload/download, rather than a single server-side move command.
- **Conflict UI**: Conflict resolution (local-conflict copy creation) is fully automated; there is no interactive prompt or diff viewer to allow manual merge operations from the UI.
