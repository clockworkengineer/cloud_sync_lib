# Phase 3 DRY Refactor Plan: Generic Simulation Fallback Wrapper

This document proposes a design to eliminate the repetitive simulation check (`if self.credentials.is_none() { ... }`) present in every operation of all three cloud providers.

---

## 1. Current Duplication

Currently, `GoogleDriveProvider`, `DropboxProvider`, and `OneDriveProvider` each contain:
- An internal `LocalSimulation` instance.
- An optional `OAuthCredentials` field.
- Redundant guards in `upload`, `download`, `delete`, and `list` methods:
  ```rust
  if self.credentials.is_none() {
      return self.local_sim.upload(local_path, remote_path).await;
  }
  ```

This duplicates check patterns across 12 different methods and merges two separate responsibilities: mock filesystem simulation and real cloud REST API client operations.

---

## 2. Proposed Design: `SimulatedFallback` Wrapper

We can implement a generic wrapper struct in `cloud_sync_lib/src/providers/fallback.rs` (or registered in `mod.rs`) that wraps any inner backend:

```rust
use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::local_sim::LocalSimulation;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct SimulatedFallback<B: StorageBackend> {
    inner: Option<B>,
    local_sim: LocalSimulation,
    name: String,
}

impl<B: StorageBackend> SimulatedFallback<B> {
    pub fn new(inner: Option<B>, local_sim: LocalSimulation, name: &str) -> Self {
        Self {
            inner,
            local_sim,
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl<B: StorageBackend> StorageBackend for SimulatedFallback<B> {
    fn name(&self) -> &str {
        &self.name
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        if let Some(ref inner) = self.inner {
            inner.upload(local_path, remote_path).await
        } else {
            self.local_sim.upload(local_path, remote_path).await
        }
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        if let Some(ref inner) = self.inner {
            inner.download(remote_path, local_path).await
        } else {
            self.local_sim.download(remote_path, local_path).await
        }
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        if let Some(ref inner) = self.inner {
            inner.delete(remote_path).await
        } else {
            self.local_sim.delete(remote_path).await
        }
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        if let Some(ref inner) = self.inner {
            inner.list(remote_path).await
        } else {
            self.local_sim.list(remote_path).await
        }
    }
}
```

### Benefits:
1. **100% DRY Fallbacks**: Centralizes all simulated storage operations.
2. **Simplified Providers**: Individual providers no longer need `Option<OAuthCredentials>` or `LocalSimulation` internally. They can assume credentials are always present (non-optional) and focus purely on real API HTTP calls.
3. **Better Testing**: Makes it easier to test the real backend components independently of the simulation framework.
