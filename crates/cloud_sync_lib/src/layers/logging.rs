//! Structured tracing and logging layer.

use super::StorageLayer;
use cloud_sync_core::{
    ChangeFeed, RemoteChange, ResumableUploadSession, ResumableUploader, ServerSideOperations,
    StorageBackend, StorageCapabilities, StorageError, StorageItem,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info};

pub struct LoggingLayer;

impl StorageLayer for LoggingLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(LoggingBackend { inner: backend })
    }
}

pub struct LoggingBackend {
    inner: Arc<dyn StorageBackend>,
}

#[async_trait::async_trait]
impl StorageBackend for LoggingBackend {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn capabilities(&self) -> StorageCapabilities {
        self.inner.capabilities()
    }

    fn as_server_side_ops(&self) -> Option<&dyn ServerSideOperations> {
        self.inner.as_server_side_ops()
    }

    fn as_change_feed(&self) -> Option<&dyn ChangeFeed> {
        self.inner.as_change_feed()
    }

    fn as_resumable_uploader(&self) -> Option<&dyn ResumableUploader> {
        self.inner.as_resumable_uploader()
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        debug!(
            "[{}] Starting upload '{}' -> '{}'",
            self.name(),
            local_path.display(),
            remote_path
        );
        let start = Instant::now();
        let res = self.inner.upload(local_path, remote_path).await;
        let elapsed = start.elapsed();
        match &res {
            Ok(()) => info!(
                "[{}] Upload succeeded for '{}' in {:?}",
                self.name(),
                remote_path,
                elapsed
            ),
            Err(e) => error!(
                "[{}] Upload failed for '{}' after {:?}: {}",
                self.name(),
                remote_path,
                elapsed,
                e
            ),
        }
        res
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        debug!(
            "[{}] Starting download '{}' -> '{}'",
            self.name(),
            remote_path,
            local_path.display()
        );
        let start = Instant::now();
        let res = self.inner.download(remote_path, local_path).await;
        let elapsed = start.elapsed();
        match &res {
            Ok(()) => info!(
                "[{}] Download succeeded for '{}' in {:?}",
                self.name(),
                remote_path,
                elapsed
            ),
            Err(e) => error!(
                "[{}] Download failed for '{}' after {:?}: {}",
                self.name(),
                remote_path,
                elapsed,
                e
            ),
        }
        res
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        debug!("[{}] Starting delete '{}'", self.name(), remote_path);
        let start = Instant::now();
        let res = self.inner.delete(remote_path).await;
        let elapsed = start.elapsed();
        match &res {
            Ok(()) => info!(
                "[{}] Delete succeeded for '{}' in {:?}",
                self.name(),
                remote_path,
                elapsed
            ),
            Err(e) => error!(
                "[{}] Delete failed for '{}' after {:?}: {}",
                self.name(),
                remote_path,
                elapsed,
                e
            ),
        }
        res
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        debug!("[{}] Starting list '{}'", self.name(), remote_path);
        let start = Instant::now();
        let res = self.inner.list(remote_path).await;
        let elapsed = start.elapsed();
        match &res {
            Ok(items) => debug!(
                "[{}] Listed {} items in '{}' in {:?}",
                self.name(),
                items.len(),
                remote_path,
                elapsed
            ),
            Err(e) => error!(
                "[{}] List failed for '{}' after {:?}: {}",
                self.name(),
                remote_path,
                elapsed,
                e
            ),
        }
        res
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        debug!("[{}] Creating folder '{}'", self.name(), remote_path);
        self.inner.create_folder(remote_path).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        info!("[{}] Renaming '{}' -> '{}'", self.name(), from, to);
        self.inner.rename(from, to).await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        info!("[{}] Copying '{}' -> '{}'", self.name(), from, to);
        self.inner.copy(from, to).await
    }

    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn ResumableUploadSession>, StorageError> {
        info!(
            "[{}] Starting resumable upload session for '{}' (chunk_size: {} bytes)",
            self.name(),
            remote_path,
            chunk_size
        );
        self.inner
            .start_resumable_upload(local_path, remote_path, chunk_size)
            .await
    }

    async fn compute_local_checksum(
        &self,
        local_path: &Path,
    ) -> Result<Option<String>, StorageError> {
        self.inner.compute_local_checksum(local_path).await
    }

    async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError> {
        self.inner.get_latest_cursor().await
    }

    async fn fetch_changes(
        &self,
        cursor: &str,
    ) -> Result<Option<(Vec<RemoteChange>, String)>, StorageError> {
        self.inner.fetch_changes(cursor).await
    }

    fn is_simulated(&self) -> bool {
        self.inner.is_simulated()
    }

    fn simulation_reason(&self) -> Option<String> {
        self.inner.simulation_reason()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_logging_layer_delegation() {
        let dir = tempdir().unwrap();
        let sim_root = dir.path().join("sim_storage");
        tokio::fs::create_dir_all(&sim_root).await.unwrap();

        let local_sim: Arc<dyn StorageBackend> =
            Arc::new(LocalSimulation::new(sim_root, "local_sim".to_string()));
        let layer = LoggingLayer;
        let wrapped = layer.layer(local_sim);

        assert_eq!(wrapped.name(), "local_sim");
        assert!(wrapped.is_simulated());

        let file_path = dir.path().join("log_test.txt");
        tokio::fs::write(&file_path, b"test content").await.unwrap();

        // Test upload, list, download, delete through LoggingLayer
        wrapped
            .upload(&file_path, "logs/test.txt")
            .await
            .expect("upload should succeed");
        let items = wrapped.list("logs").await.expect("list should succeed");
        assert_eq!(items.len(), 1);

        let dl_path = dir.path().join("downloaded.txt");
        wrapped
            .download("logs/test.txt", &dl_path)
            .await
            .expect("download should succeed");
        assert_eq!(tokio::fs::read(&dl_path).await.unwrap(), b"test content");

        wrapped
            .delete("logs/test.txt")
            .await
            .expect("delete should succeed");
        assert!(wrapped.list("logs").await.unwrap().is_empty());
    }
}
