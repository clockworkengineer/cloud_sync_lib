# Workspace Architecture

This document describes the modular architecture of the Cloud Sync ecosystem, detailing the 8-crate workspace organization, the OpenDAL-style layered middleware pipeline, the desktop interfaces (`cloud_sync_egui`, `cloud_sync_tauri`), the Axum Web UI gateway (`cloud_sync_ui`), the background daemon (`cloud_sync_daemon`), the isolated state catalog, the non-blocking observability pipeline, and the storage backend providers.

---

## Workspace Crate Topology

The repository is structured as an 8-crate Cargo workspace strictly enforcing separation of concerns, Single Responsibility, and Dependency Inversion:

```mermaid
graph TD
    subgraph UI_And_Shell [Presentation & Management Layer]
        egui_app[cloud_sync_egui - Immediate-mode GPU GUI]
        tauri_app[cloud_sync_tauri - Native Desktop Shell]
        ui_gw[cloud_sync_ui - Axum HTTP & SSE Gateway :8082]
    end

    subgraph Service_And_Lib [Service & Library Facade]
        daemon[cloud_sync_daemon - Background Core Daemon :8081]
        lib_facade[cloud_sync_lib - Client Facade & Dynamic Registry]
    end

    subgraph Middleware_And_Engine [Engine & Processing Pipelines]
        engine[cloud_sync_engine - 3-Way Sync Engine & Conflict Resolver]
        middleware[cloud_sync_middleware - OpenDAL-Style Middleware Layers]
    end

    subgraph Domain_And_Hardware [Core Abstractions & Hardware Support]
        providers[cloud_sync_providers - Concrete Cloud Storage Providers]
        fs_crate[cloud_sync_fs - Filesystem Abstraction & State Store]
        core[cloud_sync_core - Domain Types, Interfaces & Error Taxonomy]
    end

    %% presentation dependencies
    egui_app -->|TCP Socket Commands| daemon
    tauri_app -->|Spawns & Supervises Sidecar| daemon
    tauri_app -->|Embeds Web Dashboard| ui_gw
    ui_gw -->|TCP Socket Bridge| daemon

    %% daemon dependencies
    daemon --> engine
    daemon --> fs_crate
    daemon --> lib_facade

    %% engine & lib facade dependencies
    lib_facade --> middleware
    lib_facade --> providers
    lib_facade --> engine
    engine --> core
    engine --> fs_crate
    middleware --> core
    providers --> core
    fs_crate --> core
```

### Crate Roles and Responsibilities

| Crate | Responsibility | Key Exports & Traits |
| :--- | :--- | :--- |
| [`cloud_sync_core`](../crates/cloud_sync_core/README.md) | Domain types, foundational error taxonomy, security wrappers, and core storage traits. Has minimal external dependencies. | `StorageBackend`, `StorageReader`, `StorageWriter`, `DirectoryLister`, `ServerSideOperations`, `ResumableUploader`, `NormalizedPath`, `StorageError`, `StorageItem`, `StorageCapabilities` |
| [`cloud_sync_fs`](../crates/cloud_sync_fs/README.md) | Filesystem abstraction, zero-copy buffer pooling, and pluggable sync catalog storage. | `FileSystem`, `RealFileSystem`, `MockFileSystem`, `BufferPool`, `PooledBuffer`, `StateStore`, `PostcardStateStore`, `JsonStateStore` |
| [`cloud_sync_middleware`](../crates/cloud_sync_middleware/README.md) | Composable, OpenDAL-style middleware decorators wrapping `StorageBackend` dynamically. | `StorageLayer`, `LayeredBackendExt`, `RetryLayer`, `CircuitBreakerLayer`, `RateLimitLayer`, `LoggingLayer`, `MetricsLayer` |
| [`cloud_sync_engine`](../crates/cloud_sync_engine/README.md) | 3-way synchronization differential algorithms, conflict resolution policies, and event interception hooks. | `SyncEngine`, `SyncPolicy`, `ConflictPolicy`, `SyncInterceptor`, `LoggingInterceptor`, `MetricsInterceptor`, `CompositeInterceptor` |
| [`cloud_sync_providers`](../crates/cloud_sync_providers/README.md) | 13 concrete cloud storage implementations, OAuth2 token refreshing, and mock harnesses. | `GoogleDriveProvider`, `DropboxProvider`, `OneDriveProvider`, `BoxProvider`, `S3Provider`, `AzureBlobProvider`, `GCSProvider`, `B2Provider`, `PCloudProvider`, `MegaProvider`, `NextcloudProvider`, `WebDAVProvider`, `SFTPProvider` |
| [`cloud_sync_lib`](../crates/cloud_sync_lib/README.md) | Consumer-facing client library facade, dynamic registry, credential parsing, and backward-compatible re-exports. | `CloudSyncClient`, `CloudSyncBuilder`, `DynamicBackendRegistry`, `BackendRegistry`, `SimulatedFallback` |
| [`cloud_sync_daemon`](../crates/cloud_sync_daemon/README.md) | Standalone background OS daemon listening on TCP port 8081, managing filesystem watchers (`notify`) and sync schedulers. | `daemon::run`, `control::handle_control_command`, `check_backends` CLI |
| [`cloud_sync_ui`](../crates/cloud_sync_ui/README.md) / `cloud_sync_egui` / `cloud_sync_tauri` | Desktop immediate-mode GUI, browser-based web dashboard (Axum on :8082 with SSE), and OS-native desktop shell. | Native desktop dashboards, status badges, log viewer modals, tray management |

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
    participant RL as RateLimitLayer
    participant Retry as RetryLayer
    participant CB as CircuitBreakerLayer
    participant Backend as StorageBackend (e.g. S3 / Mega / GoogleDrive)

    Caller->>Client: upload(local_path, remote_path)
    Client->>Log: upload()
    Note over Log: Emits tracing span: storage.upload { path, provider }
    Log->>Met: upload()
    Note over Met: Starts latency timer & increments op counters
    Met->>RL: upload()
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
    RL-->>Met: Ok(())
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
4. **MetricsLayer**: Aggregates latency distributions, transfer volumes, and error rates via thread-safe atomic counters.
5. **LoggingLayer (Outermost)**: Emits structured OpenTelemetry-compatible tracing spans around the entire request lifecycle.

---

## High-Level System Architecture Diagram

```mermaid
graph TD
    subgraph UI_Clients [Frontend User Interfaces]
        egui_app[cloud_sync_egui - Native eframe GUI]
        tauri_app[cloud_sync_tauri - Native Desktop Shell]
        web_dashboard[Web Dashboard - Browser SPA]
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
        sync_engine[3-Way Differential sync_engine]
        tracing_appender[tracing-appender Non-blocking Worker]
    end

    subgraph File_System [Local Filesystem & State Isolation]
        watched_dir[(watched_folder/ - User Files)]
        state_dir[(sync_state/ - Isolated Postcard/JSON Catalogs)]
        log_sink[(daemon.log - Active Log File)]
    end

    subgraph cloud_sync_lib [cloud_sync_lib & cloud_sync_middleware]
        client_facade[CloudSyncClient Facade]
        layer_pipeline[Layer Pipeline: Log -> Metrics -> RateLimit -> Retry -> Breaker]
        registry[DynamicBackendRegistry]
        enc_wrapper[EncryptedBackend AES-256-GCM]
        providers[Storage Backends: Google Drive, Dropbox, OneDrive, Box, S3, Mega, etc.]
        fallback[SimulatedFallback / LocalSimulation]
    end

    subgraph Remote_Cloud [Cloud Storage Endpoints]
        cloud_apis[Cloud REST APIs / WebDAV / SFTP]
    end

    %% UI Connections
    egui_app -->|TCP Socket Commands & Logs| daemon_socket
    tauri_app -->|Spawns Sidecar & Embeds| ui_server
    web_dashboard -->|HTTP REST & SSE| ui_server
    ui_handlers -->|TCP Socket Tunnelling| daemon_socket
    ui_logs -->|get_log / Direct Read| daemon_socket

    %% Daemon Internals
    daemon_socket --> control_loop
    control_loop --> sync_engine
    main_loop --> sync_engine
    sync_engine <-->|Read / Write via RealFileSystem| watched_dir
    sync_engine <-->|Persist StateStore CSTX v1| state_dir
    main_loop -->|Emit Log Events| tracing_appender
    tracing_appender -->|Flush Records| log_sink

    %% Sync Engine to Library
    sync_engine --> client_facade
    client_facade --> layer_pipeline
    layer_pipeline --> registry
    registry --> enc_wrapper
    enc_wrapper --> providers
    providers -->|Valid OAuth Credentials| cloud_apis
    providers -->|Offline or Auth Block| fallback
```

---

## Synchronization Engine & Interceptors

The 3-way synchronization engine in `cloud_sync_engine` coordinates changes between the Base snapshot ($B$), the Local filesystem ($L$), and the Remote cloud backend ($R$):

$$\Delta = \text{Evaluate}(B, L, R)$$

### Sync Interceptor Pipeline

Lifecycle hooks are exposed through the `SyncInterceptor` trait, allowing telemetry, custom auditing, or external webhooks to fire without coupling to the sync loop:

- `on_sync_start(&self)`
- `before_operation(&self, item: &StorageItem, action: SyncAction) -> InterceptorDecision`
- `after_operation(&self, item: &StorageItem, action: SyncAction, result: &Result<(), StorageError>)`
- `on_conflict(&self, item: &StorageItem, resolution: ConflictResolution)`
- `on_sync_complete(&self, summary: &SyncSummary)`

Interceptors can be chained using `CompositeInterceptor`, combining `LoggingInterceptor` and `MetricsInterceptor`.

---

## State Management & Catalog Isolation

To prevent watcher recursion, directory clutter, and accidental cloud uploads, synchronization catalogs are isolated from user files:

```
cloud_sync_lib/
├── sync_state/                        <-- Dedicated state directory
│   ├── .sync_state_Google_Drive.bin    <-- Binary catalog (Postcard with CSTX v1 header)
│   ├── .sync_state_Google_Drive.json   <-- Human-readable JSON catalog
│   └── .sync_state_Google_Drive.bak    <-- Atomic write crash backup
└── watched_folder/                     <-- Clean user directory
    └── documents/
```

- **Separation of Concerns**: User documents remain clean and untainted by internal synchronization metadata.
- **Watcher Protection**: Writing state updates never triggers false-positive modification events in `notify`.
- **Atomic Two-Phase Commits**: State files are saved with `.bak` backups and temporary atomic renames (`.tmp` $\to$ final) to prevent state corruption during sudden OS reboots.
- **Pluggable Formats**: Implemented via `StateStore` with default `PostcardStateStore` (compact binary format protected by CRC32 checksum) and `JsonStateStore` (diagnostic inspection).

---

## Security & Path Normalization

All storage operations sanitize incoming user and provider paths using the `NormalizedPath` newtype:
- Verifies UTF-8 compliance.
- Replaces backslashes (`\`) with standard forward slashes (`/`).
- Strips redundant `./` components.
- Completely forbids parent directory traversals (`../`) to guarantee sandbox isolation and prevent directory traversal vulnerabilities.

---

## Related Architectural References

- 📘 [Client Library API Reference](CLIENT_API.md)
- 🔒 [Security & Reliability Guide](SECURITY_AND_RELIABILITY.md)
- 🗄️ [State Catalog & Storage Architecture](STATE_AND_STORAGE.md)
- 📊 [Logging & Diagnostics Architecture](LOGGING_AND_DIAGNOSTICS.md)
- 🔍 [Diagnostics & Verifiers Reference](DIAGNOSTICS_AND_VERIFIERS.md)
- 🧩 [Extending Providers & Testing](EXTENDING_AND_TESTING.md)
- 🛠️ [Troubleshooting & Operations Guide](TROUBLESHOOTING.md)
