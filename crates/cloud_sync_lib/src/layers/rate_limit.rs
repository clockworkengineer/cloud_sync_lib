//! TokenBucket rate limiter middleware layer.

use super::StorageLayer;
use crate::rate_limit::TokenBucket;
use cloud_sync_core::{
    ChangeFeed, RemoteChange, ResumableUploadSession, ResumableUploader, ServerSideOperations,
    StorageBackend, StorageCapabilities, StorageError, StorageItem,
};
use std::path::Path;
use std::sync::Arc;

pub struct RateLimitLayer {
    upload_limiter: Option<TokenBucket>,
    download_limiter: Option<TokenBucket>,
}

impl RateLimitLayer {
    pub fn new(upload_limiter: Option<TokenBucket>, download_limiter: Option<TokenBucket>) -> Self {
        Self {
            upload_limiter,
            download_limiter,
        }
    }
}

impl StorageLayer for RateLimitLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(RateLimitBackend {
            inner: backend,
            upload_limiter: self.upload_limiter.clone(),
            download_limiter: self.download_limiter.clone(),
        })
    }
}

pub struct RateLimitBackend {
    inner: Arc<dyn StorageBackend>,
    upload_limiter: Option<TokenBucket>,
    download_limiter: Option<TokenBucket>,
}

#[async_trait::async_trait]
impl StorageBackend for RateLimitBackend {
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
        if let Some(limiter) = &self.upload_limiter {
            if let Ok(meta) = tokio::fs::metadata(local_path).await {
                if let Some(delay) = limiter.consume(meta.len()) {
                    tokio::time::sleep(delay).await;
                }
            }
        }
        self.inner.upload(local_path, remote_path).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        self.inner.download(remote_path, local_path).await?;
        if let Some(limiter) = &self.download_limiter {
            if let Ok(meta) = tokio::fs::metadata(local_path).await {
                if let Some(delay) = limiter.consume(meta.len()) {
                    tokio::time::sleep(delay).await;
                }
            }
        }
        Ok(())
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.delete(remote_path).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        self.inner.list(remote_path).await
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.inner.create_folder(remote_path).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to).await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.copy(from, to).await
    }

    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn ResumableUploadSession>, StorageError> {
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
    use crate::rate_limit::TokenBucket;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_rate_limit_layer_roundtrip() {
        let dir = tempdir().unwrap();
        let sim_root = dir.path().join("sim_storage");
        tokio::fs::create_dir_all(&sim_root).await.unwrap();

        let local_sim: Arc<dyn StorageBackend> = Arc::new(LocalSimulation::new(sim_root, "local_sim".to_string()));
        let upload_limiter = TokenBucket::new(100_000);
        let download_limiter = TokenBucket::new(100_000);

        let layer = RateLimitLayer::new(Some(upload_limiter), Some(download_limiter));
        let wrapped = layer.layer(local_sim);

        assert_eq!(wrapped.name(), "local_sim");

        let file_path = dir.path().join("rate_test.txt");
        tokio::fs::write(&file_path, b"rate limited data").await.unwrap();

        wrapped.upload(&file_path, "ratelimit/file.txt").await.expect("upload should succeed");
        let items = wrapped.list("ratelimit").await.expect("list should succeed");
        assert_eq!(items.len(), 1);

        let dl_path = dir.path().join("downloaded.txt");
        wrapped.download("ratelimit/file.txt", &dl_path).await.expect("download should succeed");
        assert_eq!(tokio::fs::read(&dl_path).await.unwrap(), b"rate limited data");

        wrapped.delete("ratelimit/file.txt").await.expect("delete should succeed");
        assert!(wrapped.list("ratelimit").await.unwrap().is_empty());
    }
}
