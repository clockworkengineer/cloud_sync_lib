# Embedded Systems Refactoring Plan for `cloud_sync_lib`

This document outlines a concrete, actionable refactoring plan to adapt and optimize `cloud_sync_lib` and its sync/backup daemons for resource-constrained, embedded systems (e.g., ESP32, STM32, embedded Linux devices, or RTOS-based microcontrollers).

---

## 1. Current Bottlenecks & Embedded Constraints

1. **Heavy Networking & Runtime**: The library relies on `reqwest` (pulling in `hyper`, `tokio-rustls`, `mio`, etc.) and the full `tokio` runtime. On microcontrollers or constrained devices, this runtime footprint consumes too much RAM and flash space.
2. **Dynamic Allocations (Heap Overhead)**: Extensive use of traits objects (`&dyn StorageBackend`), heap-allocated strings (`String`), `Box<dyn std::error::Error>`, and frequent clones cause heap fragmentation and allocation overhead.
3. **Flash Memory Wear**: The daemons periodically serialize sync state to `.sync_state.json` on local storage. Constant small writes to flash storage degrade hardware lifespan quickly on systems without filesystem wear leveling.
4. **Active Polling (Power Consumption)**: The continuous 60-second polling loops keep the CPU active, preventing the device from entering low-power/deep-sleep modes.

---

## 2. Refactoring Pillars

### Pillar 1: Dependency Pruning & Lightweight Networking
* **Action**: Swap `reqwest` for a lightweight, embedded-friendly HTTP/networking stack.
  * *Option A (Embedded Linux)*: Swap `reqwest` with `ureq` (minimal, synchronous, lightweight client) or compile with tiny TLS features.
  * *Option B (Bare-Metal/RTOS)*: Integrate with `embedded-nal` (Network Abstraction Layer) and ESP/STM hardware TCP sockets.
* **Action**: Make all providers optional via cargo feature flags so the compiler only builds the specific backend needed by the device (e.g. only compiling `webdav` for a local NAS sync, saving hundreds of KB of flash).

### Pillar 2: Static Dispatch & Zero-Allocation Design
* **Action**: Eliminate dynamic dispatch (`dyn StorageBackend`, `Box<dyn Error>`). Replace them with compile-time generics (`impl StorageBackend`) or statically dispatched enums (`enum ActiveProvider`).
* **Action**: Refactor key paths and filenames to use `&str` or `Cow<'a, str>` instead of allocating `String` and `PathBuf` on every directory traversal.

### Pillar 3: Flash Wear Leveling & Compact State
* **Action**: Replace JSON serialization (`serde_json` / `.sync_state.json`) with a lightweight binary format like **`postcard`** (designed for `no_std` and embedded microcontrollers) or **MessagePack**.
* **Action**: Buffer state writes in memory and flush to persistent storage (Flash/EEPROM) only when changes occur, or implement a basic wear-leveling flash driver wrapper.

### Pillar 4: Sleep-Wake Cycles (Power Management)
* **Action**: Replace the active loop (`tokio::time::sleep`) with a single-shot execution model. 
* **Action**: Expose control hooks so the device can boot, execute a single sync iteration, write the updated state, signal completion to the power management unit (PMU), and go into deep sleep (RTC-wakeup).

### Pillar 5: `no_std` Core Architecture
* **Action**: Split `cloud_sync_lib` into:
  * `cloud_sync_core`: A `no_std` crate defining traits, state structures, and safe path normalization.
  * `cloud_sync_std`: An optional `std` companion crate providing filesystem helper utilities.

---

## 3. Action Plan Phases

### Phase 1: Compile-Time Optimization (Immediate Win)
Configure `Cargo.toml` profiles for size optimization:
```toml
[profile.release]
opt-level = "z"      # Optimize for size
lto = true           # Link-Time Optimization
codegen-units = 1    # Reduce parallel compilation to maximize LTO
panic = "abort"      # Remove landing pads for panic handling
```

### Phase 2: Static Dispatch & Allocator Reductions
1. Rewrite `StorageBackend` trait methods to return custom concrete error types instead of `Box<dyn Error>`.
2. Convert sync engine structures to use generic type parameters:
   ```rust
   pub struct SyncEngine<S: StorageBackend, D: StorageBackend> {
       source: S,
       destination: D,
   }
   ```

### Phase 3: Lightweight Networking Migration
1. Introduce a feature flag `embedded-net` that replaces `reqwest` with a synchronous socket/minimal HTTP library.
2. Replace `quick-xml` (used in WebDAV) with a stack-allocated parser like `xmlparser` or `minidom` configured for minimal footprint.

### Phase 4: Wear-Leveling State Wrapper
Implement a state manager that uses the `postcard` binary format:
```rust
// Compact binary representation of sync state
#[derive(serde::Serialize, serde::Deserialize)]
struct CompactState {
    version: u8,
    last_sync: u64,
    items: Vec<(CompactPathHash, CompactMetadata)>,
}
```
