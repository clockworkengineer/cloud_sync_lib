# Workspace Architecture

This document describes the modular architecture of the Cloud Sync ecosystem, detailing the 9-crate workspace organization, the OpenDAL-style layered middleware pipeline, the desktop and mobile interfaces (`cloud_sync_egui`, `cloud_sync_tauri`), the Axum Web UI gateway (`cloud_sync_ui`), the background daemon (`cloud_sync_daemon`), the media hub (`cloud_sync_photos`), the unidirectional backup daemon (`cloud_sync_backup`), the standard utility library (`cloud_sync_std`), and the isolated state catalog.

---

## Workspace Crate Topology

The repository is structured as a 9-crate Cargo workspace strictly enforcing separation of concerns, Single Responsibility, and Dependency Inversion:

```mermaid
graph TD
    subgraph Presentation_Layer [Presentation & Management Layer]
        egui_app["cloud_sync_egui<br/>(Immediate-mode GPU Desktop GUI)"]
        tauri_app["cloud_sync_tauri<br/>(Tauri 2.0 Desktop & Android Shell)"]
        ui_gw["cloud_sync_ui<br/>(Axum HTTP & SSE Gateway :8082)"]
    end

    subgraph Service_And_Applications [Daemons & Application Engines]
        daemon["cloud_sync_daemon<br/>(Background 3-Way Core Daemon :8081)"]
        backup["cloud_sync_backup<br/>(Periodic & One-Shot Backup Client)"]
        photos["cloud_sync_photos<br/>(Google Photos Equivalence Hub & CLI)"]
    end

    subgraph Library_Facade_And_Middleware [Client Facade, Virtual Backends & Middleware]
        lib_facade["cloud_sync_lib<br/>(Unified Facade, 17 Providers, Union, Chunker, Layers, Snapshots, Dedupe)"]
    end

    subgraph Core_Abstractions_And_Utilities [Foundational Abstractions & System Utilities]
        std_crate["cloud_sync_std<br/>(Async Checksums & Ripgrep SyncIgnore)"]
        core_crate["cloud_sync_core<br/>(Domain Models, Role Traits, Error Taxonomy, NormalizedPath, StateStore)"]
    end

    %% Presentation dependencies
    egui_app -->|TCP Socket Commands| daemon
    tauri_app -->|In-Process Engine / IPC Commands| daemon
    tauri_app -->|Embeds Web Dashboard| ui_gw
    tauri_app -->|Native Photos Hub Bridge| photos
    ui_gw -->|TCP Socket Bridge| daemon

    %% Service dependencies
    daemon --> lib_facade
    daemon --> std_crate
    daemon --> core_crate
    backup --> lib_facade
    photos --> lib_facade

    %% Library facade dependencies
    lib_facade --> core_crate
    lib_facade --> std_crate
    std_crate --> core_crate
```

### Crate Roles and Responsibilities

| Crate | Responsibility | Key Exports & Traits |
| :--- | :--- | :--- |
| **[`cloud_sync_core`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_core/README.md)** | Zero-dependency, `no_std`-compatible domain types, granular storage role traits, error taxonomy, `NormalizedPath` security newtype, and `StateStore` persistence trait. | `StorageBackend`, `StorageReader`, `StorageWriter`, `DirectoryLister`, `FolderOps`, `ServerSideOperations`, `ResumableUploader`, `NormalizedPath`, `StorageError`, `StorageItem`, `StorageCapabilities`, `StateStore`, `PostcardStateStore`, `JsonStateStore` |
| **[`cloud_sync_std`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_std/README.md)** | Standard library utilities, non-blocking asynchronous cryptographic checksum hashers, and Ripgrep-based ignore pattern engine. | `compute_sha256`, `compute_md5`, `compute_crc32`, `SyncIgnore` |
| **[`cloud_sync_lib`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_lib/README.md)** | High-level consumer client facade (`CloudSyncClient`), 17 concrete cloud storage providers, virtual backends (`UnionBackend`, `ChunkerBackend`, `EncryptedBackend`), composable middleware layers, zero-copy buffer pool, snapshot engine, deduplication, analytics, and cloud-to-cloud transfer. | `CloudSyncClient`, `CloudSyncBuilder`, `DynamicBackendRegistry`, `UnionBackend`, `ChunkerBackend`, `CompressionLayer`, `CacheLayer`, `RetryLayer`, `CircuitBreakerLayer`, `RateLimitLayer`, `MetricsLayer`, `LoggingLayer`, `SnapshotEngine`, `transfer_between_backends`, `BufferPool` |
| **[`cloud_sync_daemon`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_daemon/README.md)** | Long-running OS daemon watching local directories with `notify`, 3-way synchronization orchestrator (`SyncPlanner`, `SyncExecutor`), rename/move detection (`RenameDetector`), staggered file versioning (`TrashManager`), and TCP control socket (`127.0.0.1:8081`). | `daemon::run`, `control::handle_control_command`, `SyncPlanner`, `SyncExecutor`, `RenameDetector`, `TrashManager`, `check_backends` CLI |
| **[`cloud_sync_photos`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_photos/README.md)** | Self-contained Google Photos equivalence engine with pure-Rust EXIF metadata extraction, chronological timeline organization, SHA-256 deduplication, Haversine GPS search, virtual albums, and standalone CLI. | `PhotoLibrary`, `OrganizeStrategy`, `PhotoMetadata`, `ExifData`, `GeoLocation`, `SearchFilter`, `cloud_sync_photos` CLI |
| **[`cloud_sync_backup`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_backup/README.md)** | Dedicated background service and CLI for unidirectional periodic or one-shot backups across all 17 storage providers with smart change detection. | `backup_main`, `BackupConfig`, `run_backup_pass` |
| **[`cloud_sync_ui`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_ui/README.md)** | Decoupled Axum HTTP REST and Server-Sent Events (SSE) server listening on `:8082`, serving embedded SPA management dashboard and bridging web actions to daemon TCP socket. | `start_ui_server`, `UiState`, `/api/status`, `/api/events`, `/api/logs`, `/api/conflicts` |
| **[`cloud_sync_egui`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_egui/README.md)** | Native GPU-accelerated desktop GUI built on `egui` and `eframe` with 60 FPS update loop, Tokio background runtime, log viewer modal, and offline self-start. | `CloudSyncApp`, `run_native_gui` |
| **[`cloud_sync_tauri`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_tauri/README.md)** | Cross-platform desktop and mobile (Android) application shell powered by Tauri 2.0, supporting both desktop sidecar management and mobile in-process sync. | Tauri IPC commands (`status`, `sync`, `pause`, `resume`, `conflicts`), mobile entry points, Photos Hub view |

---

## Layered Middleware Pipeline Architecture

`cloud_sync_lib` uses an OpenDAL-style composable onion architecture (`StorageLayer`) that wraps any storage backend without requiring inheritance or modifying provider code:

```mermaid
sequenceDiagram
    autonumber
    actor Caller as Library Consumer / SyncEngine
    participant Client as CloudSyncClient Facade
    participant Log as LoggingLayer
    participant Met as MetricsLayer
    participant Cache as CacheLayer (TTL Listing Cache)
    participant Comp as CompressionLayer (CLZ1)
    participant RL as RateLimitLayer
    participant Retry as RetryLayer
    participant CB as CircuitBreakerLayer
    participant Backend as StorageBackend (e.g. S3 / Mega / GoogleDrive)

    Caller->>Client: upload(local_path, remote_path)
    Client->>Log: upload()
    Note over Log: Emits tracing span: storage.upload { path, provider }
    Log->>Met: upload()
    Note over Met: Starts latency timer & increments op counters
    Met->>Cache: upload()
    Note over Cache: Invalidates parent listing cache (write-through)
    Cache->>Comp: upload()
    Note over Comp: Encodes payload with CLZ1 header (LZ77+RLE)
    Comp->>RL: upload()
    Note over RL: TokenBucket acquires bandwidth permits
    RL->>Retry: upload()
    loop Up to max_attempts (with jittered backoff)
        Retry->>CB: upload()
        alt Circuit OPEN
            CB-->>Retry: Err(CircuitBreakerOpen) [Fast Fail]
        else Circuit CLOSED / HALF-OPEN
            CB->>Backend: upload()
            alt Backend Transient Error (503 / 429)
                Backend-->>CB: Err(StorageError::ConnectionFailed)
                CB-->>Retry: Err(transient)
                Note over Retry: Computes Exponential Jittered Sleep
            else Backend Success
                Backend-->>CB: Ok(())
                CB-->>Retry: Ok(())
            end
        end
    end
    Retry-->>RL: Ok(())
    RL-->>Comp: Ok(())
    Comp-->>Cache: Ok(())
    Cache-->>Met: Ok(())
    Note over Met: Records total duration & bytes transferred
    Met-->>Log: Ok(())
    Note over Log: Closes span with duration_ms
    Log-->>Client: Ok(())
    Client-->>Caller: Ok(())
```

### Execution Order & Rationale
When building via `CloudSyncBuilder` or `LayeredBackendExt`:
1. **CircuitBreakerLayer (Innermost before Provider)**: Fails immediately when the backend is experiencing sustained outages, preventing unnecessary retry storms.
2. **RetryLayer**: Retries transient errors (`is_transient()`) with truncated exponential backoff and randomized full-jitter math ($Sleep = \text{random}(0, \min(M, B \cdot 2^{\text{attempt}}))$).
3. **RateLimitLayer**: Token-bucket bandwidth regulation governing inbound and outbound bytes per second.
4. **CompressionLayer**: Transparent `CLZ1` lossless compression for uploaded files and automatic decompression on download.
5. **CacheLayer**: In-memory metadata and directory listing cache with configurable TTL and write-through invalidation.
6. **MetricsLayer**: Aggregates latency distributions, transfer volumes, and error rates via thread-safe atomic counters.
7. **LoggingLayer (Outermost)**: Emits structured OpenTelemetry-compatible tracing spans around the entire request lifecycle.

---

## High-Level System Architecture Diagram

```mermaid
graph TD
    subgraph UI_Clients [Frontend User Interfaces]
        egui_app[cloud_sync_egui - Native eframe GUI]
        tauri_app[cloud_sync_tauri - Native Desktop & Android Shell]
        web_dashboard[Web Dashboard - Browser SPA]
        photos_cli[cloud_sync_photos CLI]
    end

    subgraph cloud_sync_ui [cloud_sync_ui - Decoupled Axum Gateway :8082]
        ui_server[Axum HTTP & SSE Server]
        ui_logs[GET /api/logs Handler]
        ui_events[GET /api/events SSE]
        ui_handlers[Control & Conflict Handlers]
    end

    subgraph cloud_sync_daemon [cloud_sync_daemon - Background Core Service :8081]
        daemon_socket[TCP Control Listener :8081]
        control_loop[control::handle_control_command]
        main_loop[Watcher & Scheduler Loop]
        sync_engine[3-Way Differential Engine]
        rename_det[RenameDetector Heuristics]
        trash_mgr[TrashManager Staggered Versioning]
        tracing_appender[tracing-appender Non-blocking Worker]
    end

    subgraph File_System [Local Filesystem & State Isolation]
        watched_dir[(watched_folder/ - User Files)]
        state_dir[(sync_state/ - Isolated Postcard/JSON Catalogs)]
        trash_dir[(sync_state/trash/ - Version Archives)]
        log_sink[(daemon.log - Active Log File)]
    end

    subgraph cloud_sync_lib [cloud_sync_lib Core Services]
        client_facade[CloudSyncClient Facade]
        layer_pipeline[Layer Pipeline: Log -> Metrics -> Cache -> Comp -> RL -> Retry -> CB]
        composite_backends[Composite Backends: UnionBackend / ChunkerBackend]
        enc_wrapper[EncryptedBackend AES-256-GCM]
        providers[17 Cloud Storage Backends]
        snapshot_engine[SnapshotEngine Content-Addressable Backup]
        cloud_transfer[Direct Cloud-to-Cloud Transfer Engine]
        dedupe_engine[DedupeEngine Storage Cleaner]
    end

    subgraph Remote_Cloud [Cloud Storage Endpoints]
        cloud_apis[Cloud REST APIs / WebDAV / SFTP / S3 / FTP / SMB]
    end

    %% UI Connections
    egui_app -->|TCP Socket Commands & Logs| daemon_socket
    tauri_app -->|In-Process Engine or TCP Bridge| daemon_socket
    tauri_app -->|Embeds Web Dashboard| ui_server
    web_dashboard -->|HTTP REST & SSE| ui_server
    ui_handlers -->|TCP Socket Tunnelling| daemon_socket
    ui_logs -->|get_log / Direct Read| daemon_socket
    photos_cli --> client_facade

    %% Daemon Internals
    daemon_socket --> control_loop
    control_loop --> sync_engine
    main_loop --> sync_engine
    sync_engine --> rename_det
    sync_engine --> trash_mgr
    sync_engine <-->|Read / Write| watched_dir
    sync_engine <-->|Persist StateStore CSTX v1| state_dir
    trash_mgr <-->|Archive / Restore Versions| trash_dir
    main_loop -->|Emit Log Events| tracing_appender
    tracing_appender -->|Flush Records| log_sink

    %% Sync Engine to Library
    sync_engine --> client_facade
    client_facade --> layer_pipeline
    layer_pipeline --> composite_backends
    composite_backends --> enc_wrapper
    enc_wrapper --> providers
    providers --> cloud_apis
```

---

## Synchronization Engine, Rename Detection & Versioning

The 3-way synchronization engine in `cloud_sync_daemon` coordinates changes between the Base snapshot ($B$), the Local filesystem ($L$), and the Remote cloud backend ($R$):

$$\Delta = \text{Evaluate}(B, L, R)$$

### 1. Heuristic Rename & Move Detection (`RenameDetector`)
When files are renamed or moved across directories:
- Standard sync engines treat the change as a destructive deletion followed by an upload.
- `RenameDetector` inspects pending deletions and additions, matching entries using content checksums and file sizes.
- Matches are converted into efficient server-side rename/move operations where supported, saving network bandwidth.

### 2. Staggered File Versioning (`TrashManager`)
- Before any file overwrite or deletion takes place, the existing file is safely archived to `sync_state/trash/` using timestamp formatting: `YYYYMMDD_HHMMSS_<filename>`.
- Preserves point-in-time file recovery.
- Automatically enforces retention bounds (`retention_days` and `max_versions_per_file`).

---

## State Management & Catalog Isolation

To prevent watcher recursion, directory clutter, and accidental cloud uploads, synchronization catalogs are isolated from user files:

```
cloud_sync_lib/
├── sync_state/                        <-- Dedicated state directory
│   ├── .sync_state_Google_Drive.bin    <-- Binary catalog (Postcard with CSTX v1 header & CRC32)
│   ├── .sync_state_Google_Drive.json   <-- Human-readable JSON catalog
│   ├── .sync_state_Google_Drive.bak    <-- Atomic write crash backup
│   └── trash/                         <-- Staggered file versioning archive
└── watched_folder/                     <-- Clean user directory
    └── documents/
```

- **Separation of Concerns**: User documents remain clean and untainted by internal synchronization metadata.
- **Watcher Protection**: Writing state updates never triggers false-positive modification events in `notify`.
- **Atomic Two-Phase Commits**: State files are saved with `.bak` backups and temporary atomic renames (`.tmp` $\to$ final) to prevent state corruption during sudden OS reboots.
- **Pluggable Formats**: Implemented via `StateStore` with default `PostcardStateStore` (compact binary format protected by CRC32 checksum) and `JsonStateStore` (diagnostic inspection).

---

## Related Architectural References

- 📸 [Google Photos Equivalence Hub Guide](PHOTOS_HUB.md)
- 🔀 [Virtual Backends & Middleware Layers Guide](VIRTUAL_BACKENDS_AND_LAYERS.md)
- 🧰 [Operational Tools & Disaster Recovery Guide](OPERATIONAL_TOOLS.md)
- 📘 [Client Library API Reference](CLIENT_API.md)
- 📱 [Android Client Guide](ANDROID_CLIENT.md)
- 🔒 [Security & Reliability Guide](SECURITY_AND_RELIABILITY.md)
- 🗄️ [State Catalog & Storage Architecture](STATE_AND_STORAGE.md)
- 📊 [Logging & Diagnostics Architecture](LOGGING_AND_DIAGNOSTICS.md)
- 🔍 [Diagnostics & Verifiers Reference](DIAGNOSTICS_AND_VERIFIERS.md)
- 🧩 [Extending Providers & Testing](EXTENDING_AND_TESTING.md)
- 🛠️ [Troubleshooting & Operations Guide](TROUBLESHOOTING.md)
