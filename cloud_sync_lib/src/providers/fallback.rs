//! Fallback wrapper for simulated storage provider logic.
//!
//! Route requests to either a real storage provider or local directory simulation
//! depending on the presence of configured credentials.

use crate::traits::{StorageBackend, StorageError, StorageItem};
use crate::providers::local_sim::LocalSimulation;
use async_trait::async_trait;
use std::path::Path;

/// Storage backend wrapper that transparently routes operations to either a real cloud provider
/// or the shared `LocalSimulation` mock directory fallback.
pub struct SimulatedFallback<B: StorageBackend> {
    /// The real cloud provider client, if configured.
    inner: Option<B>,
    /// The local directory simulation mock fallback.
    local_sim: LocalSimulation,
    /// The user-friendly name of the storage backend.
    name: String,
    /// The sync mode for this connection.
    sync_mode: super::SyncMode,
}

impl<B: StorageBackend> SimulatedFallback<B> {
    /// Creates a new `SimulatedFallback` wrapper around an optional inner provider backend.
    ///
    /// # Arguments
    /// * `inner` - The optional real storage backend to route operations to.
    /// * `local_sim` - The simulation backend to use as a fallback.
    /// * `name` - The user-friendly name of the storage backend.
    /// * `sync_mode` - The sync mode.
    ///
    /// # Returns
    /// A new instance of `SimulatedFallback`.
    pub fn new(inner: Option<B>, local_sim: LocalSimulation, name: &str, sync_mode: super::SyncMode) -> Self {
        Self {
            inner,
            local_sim,
            name: name.to_string(),
            sync_mode,
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

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        if let Some(ref inner) = self.inner {
            inner.create_folder(remote_path).await
        } else {
            self.local_sim.create_folder(remote_path).await
        }
    }

    fn sync_mode(&self) -> super::SyncMode {
        self.sync_mode
    }
}
