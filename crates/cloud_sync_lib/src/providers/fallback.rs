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
    _sync_mode: super::SyncMode,
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
            _sync_mode: sync_mode,
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
            match inner.upload(local_path, remote_path).await {
                Ok(()) => Ok(()),
                Err(StorageError::Authentication(e)) | Err(StorageError::AuthenticationExpired(e)) => {
                    tracing::warn!("[{}] Authentication failed, falling back to local simulation: {}", self.name, e);
                    self.local_sim.upload(local_path, remote_path).await
                }
                Err(e) => Err(e),
            }
        } else {
            self.local_sim.upload(local_path, remote_path).await
        }
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        if let Some(ref inner) = self.inner {
            match inner.download(remote_path, local_path).await {
                Ok(()) => Ok(()),
                Err(StorageError::Authentication(e)) | Err(StorageError::AuthenticationExpired(e)) => {
                    tracing::warn!("[{}] Authentication failed, falling back to local simulation: {}", self.name, e);
                    self.local_sim.download(remote_path, local_path).await
                }
                Err(e) => Err(e),
            }
        } else {
            self.local_sim.download(remote_path, local_path).await
        }
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        if let Some(ref inner) = self.inner {
            match inner.delete(remote_path).await {
                Ok(()) => Ok(()),
                Err(StorageError::Authentication(e)) | Err(StorageError::AuthenticationExpired(e)) => {
                    tracing::warn!("[{}] Authentication failed, falling back to local simulation: {}", self.name, e);
                    self.local_sim.delete(remote_path).await
                }
                Err(e) => Err(e),
            }
        } else {
            self.local_sim.delete(remote_path).await
        }
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        if let Some(ref inner) = self.inner {
            match inner.list(remote_path).await {
                Ok(items) => Ok(items),
                Err(StorageError::Authentication(e)) | Err(StorageError::AuthenticationExpired(e)) => {
                    tracing::warn!("[{}] Authentication failed, falling back to local simulation: {}", self.name, e);
                    self.local_sim.list(remote_path).await
                }
                Err(e) => Err(e),
            }
        } else {
            self.local_sim.list(remote_path).await
        }
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        if let Some(ref inner) = self.inner {
            match inner.create_folder(remote_path).await {
                Ok(()) => Ok(()),
                Err(StorageError::Authentication(e)) | Err(StorageError::AuthenticationExpired(e)) => {
                    tracing::warn!("[{}] Authentication failed, falling back to local simulation: {}", self.name, e);
                    self.local_sim.create_folder(remote_path).await
                }
                Err(e) => Err(e),
            }
        } else {
            self.local_sim.create_folder(remote_path).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;
    use std::sync::Arc;

    struct MockBackend {
        should_fail_auth: Arc<Mutex<bool>>,
        should_fail_other: Arc<Mutex<bool>>,
    }

    #[async_trait]
    impl StorageBackend for MockBackend {
        fn name(&self) -> &str {
            "Mock"
        }

        async fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), StorageError> {
            if *self.should_fail_auth.lock().unwrap() {
                return Err(StorageError::Authentication("Mock auth error".to_string()));
            }
            if *self.should_fail_other.lock().unwrap() {
                return Err(StorageError::NotFound("Not found".to_string()));
            }
            Ok(())
        }

        async fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), StorageError> {
            if *self.should_fail_auth.lock().unwrap() {
                return Err(StorageError::AuthenticationExpired("Mock expired error".to_string()));
            }
            if *self.should_fail_other.lock().unwrap() {
                return Err(StorageError::NotFound("Not found".to_string()));
            }
            Ok(())
        }

        async fn delete(&self, _remote_path: &str) -> Result<(), StorageError> {
            if *self.should_fail_auth.lock().unwrap() {
                return Err(StorageError::Authentication("Mock auth error".to_string()));
            }
            if *self.should_fail_other.lock().unwrap() {
                return Err(StorageError::NotFound("Not found".to_string()));
            }
            Ok(())
        }

        async fn list(&self, _remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
            if *self.should_fail_auth.lock().unwrap() {
                return Err(StorageError::Authentication("Mock auth error".to_string()));
            }
            if *self.should_fail_other.lock().unwrap() {
                return Err(StorageError::NotFound("Not found".to_string()));
            }
            Ok(vec![])
        }

        async fn create_folder(&self, _remote_path: &str) -> Result<(), StorageError> {
            if *self.should_fail_auth.lock().unwrap() {
                return Err(StorageError::Authentication("Mock auth error".to_string()));
            }
            if *self.should_fail_other.lock().unwrap() {
                return Err(StorageError::NotFound("Not found".to_string()));
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_simulated_fallback_routing() {
        let temp_dir = tempdir().unwrap();
        let local_sim = LocalSimulation::new(temp_dir.path().to_path_buf(), "MockLocal".to_string());
        
        let should_fail_auth = Arc::new(Mutex::new(false));
        let should_fail_other = Arc::new(Mutex::new(false));
        let mock = MockBackend {
            should_fail_auth: should_fail_auth.clone(),
            should_fail_other: should_fail_other.clone(),
        };

        let fallback = SimulatedFallback::new(Some(mock), local_sim, "FallbackTest", crate::providers::SyncMode::TwoWay);
        assert_eq!(fallback.name(), "FallbackTest");

        let local_file = temp_dir.path().join("test.txt");
        std::fs::write(&local_file, "hello").unwrap();

        // 1. Successful routing to inner
        fallback.upload(&local_file, "remote.txt").await.unwrap();
        fallback.download("remote.txt", &local_file).await.unwrap();
        fallback.create_folder("folder").await.unwrap();
        let list = fallback.list("").await.unwrap();
        assert!(list.is_empty());
        fallback.delete("remote.txt").await.unwrap();

        // 2. Authentication failure fallback routing to local_sim
        *should_fail_auth.lock().unwrap() = true;
        fallback.upload(&local_file, "remote.txt").await.unwrap();
        fallback.download("remote.txt", &local_file).await.unwrap();
        fallback.create_folder("folder").await.unwrap();
        fallback.list("").await.unwrap();
        fallback.delete("remote.txt").await.unwrap();

        // 3. Other errors propagated without fallback
        *should_fail_auth.lock().unwrap() = false;
        *should_fail_other.lock().unwrap() = true;
        assert!(fallback.upload(&local_file, "remote.txt").await.is_err());
        assert!(fallback.download("remote.txt", &local_file).await.is_err());
        assert!(fallback.create_folder("folder").await.is_err());
        assert!(fallback.list("").await.is_err());
        assert!(fallback.delete("remote.txt").await.is_err());
    }

    #[tokio::test]
    async fn test_simulated_fallback_no_inner() {
        let temp_dir = tempdir().unwrap();
        let local_sim = LocalSimulation::new(temp_dir.path().to_path_buf(), "MockLocal".to_string());

        let fallback: SimulatedFallback<MockBackend> = SimulatedFallback::new(
            None,
            local_sim,
            "FallbackTestNoInner",
            crate::providers::SyncMode::TwoWay,
        );

        let local_file = temp_dir.path().join("test.txt");
        std::fs::write(&local_file, "hello").unwrap();

        fallback.upload(&local_file, "remote.txt").await.unwrap();
        fallback.download("remote.txt", &local_file).await.unwrap();
        fallback.create_folder("folder").await.unwrap();
        let list = fallback.list("").await.unwrap();
        assert_eq!(list.len(), 3);
        fallback.delete("remote.txt").await.unwrap();
    }
}
