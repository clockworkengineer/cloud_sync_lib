//! Exponential backoff retry middleware layer.

use super::StorageLayer;
use cloud_sync_core::{
    RemoteChange, ResumableUploadSession, StorageBackend, StorageCapabilities, StorageError,
    StorageItem,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
        }
    }
}

pub struct RetryLayer {
    config: RetryConfig,
}

impl RetryLayer {
    pub fn new(config: RetryConfig) -> Self {
        Self { config }
    }
}

impl Default for RetryLayer {
    fn default() -> Self {
        Self::new(RetryConfig::default())
    }
}

impl StorageLayer for RetryLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(RetryBackend {
            inner: backend,
            config: self.config.clone(),
        })
    }
}

pub struct RetryBackend {
    inner: Arc<dyn StorageBackend>,
    config: RetryConfig,
}

impl RetryBackend {
    fn is_retryable(err: &StorageError) -> bool {
        match err {
            StorageError::RateLimit { .. } => true,
            StorageError::ConnectionFailed(_) => true,
            StorageError::AuthenticationExpired(_) => true,
            StorageError::Provider { status, .. } => {
                if let Some(code) = status {
                    *code == 429 || *code >= 500
                } else {
                    true
                }
            }
            StorageError::Reqwest(e) => e.is_connect() || e.is_timeout() || e.is_request(),
            _ => false,
        }
    }

    async fn execute_with_retry<F, Fut, T>(&self, op_name: &str, f: F) -> Result<T, StorageError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, StorageError>>,
    {
        let mut attempts = 0;
        let mut delay = self.config.base_delay;

        loop {
            attempts += 1;
            match f().await {
                Ok(val) => return Ok(val),
                Err(err) => {
                    if attempts >= self.config.max_attempts || !Self::is_retryable(&err) {
                        return Err(err);
                    }

                    let sleep_dur = match &err {
                        StorageError::RateLimit {
                            retry_after: Some(dur),
                            ..
                        } => *dur,
                        _ => {
                            let jitter = Duration::from_millis((rand::random::<u64>() % 200) + 50);
                            delay.saturating_add(jitter)
                        }
                    };

                    warn!(
                        "[{}] Operation '{}' failed on attempt {}/{} with retryable error ({}). Retrying after {:?}...",
                        self.inner.name(),
                        op_name,
                        attempts,
                        self.config.max_attempts,
                        err,
                        sleep_dur
                    );

                    tokio::time::sleep(sleep_dur).await;
                    delay = (delay * 2).min(self.config.max_delay);
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl StorageBackend for RetryBackend {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn capabilities(&self) -> StorageCapabilities {
        self.inner.capabilities()
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        self.execute_with_retry("upload", || self.inner.upload(local_path, remote_path))
            .await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        self.execute_with_retry("download", || self.inner.download(remote_path, local_path))
            .await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        self.execute_with_retry("delete", || self.inner.delete(remote_path))
            .await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        self.execute_with_retry("list", || self.inner.list(remote_path))
            .await
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.execute_with_retry("create_folder", || self.inner.create_folder(remote_path))
            .await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.execute_with_retry("rename", || self.inner.rename(from, to))
            .await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.execute_with_retry("copy", || self.inner.copy(from, to))
            .await
    }

    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn ResumableUploadSession>, StorageError> {
        self.execute_with_retry("start_resumable_upload", || {
            self.inner
                .start_resumable_upload(local_path, remote_path, chunk_size)
        })
        .await
    }

    async fn compute_local_checksum(
        &self,
        local_path: &Path,
    ) -> Result<Option<String>, StorageError> {
        self.inner.compute_local_checksum(local_path).await
    }

    async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError> {
        self.execute_with_retry("get_latest_cursor", || self.inner.get_latest_cursor())
            .await
    }

    async fn fetch_changes(
        &self,
        cursor: &str,
    ) -> Result<Option<(Vec<RemoteChange>, String)>, StorageError> {
        self.execute_with_retry("fetch_changes", || self.inner.fetch_changes(cursor))
            .await
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
    use std::sync::atomic::{AtomicU32, Ordering};

    struct FlakyBackend {
        fails_remaining: AtomicU32,
    }

    #[async_trait::async_trait]
    impl StorageBackend for FlakyBackend {
        fn name(&self) -> &str {
            "Flaky"
        }
        async fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), StorageError> {
            let prev = self.fails_remaining.fetch_sub(1, Ordering::SeqCst);
            if prev > 1 {
                Err(StorageError::ConnectionFailed(
                    "transient dropped link".into(),
                ))
            } else {
                Ok(())
            }
        }
        async fn download(
            &self,
            _remote_path: &str,
            _local_path: &Path,
        ) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete(&self, _remote_path: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list(&self, _remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_retry_layer_recovers() {
        let flaky = Arc::new(FlakyBackend {
            fails_remaining: AtomicU32::new(3),
        });
        let layer = RetryLayer::new(RetryConfig {
            max_attempts: 4,
            base_delay: Duration::from_millis(5),
            max_delay: Duration::from_millis(20),
        });
        let wrapped = layer.layer(flaky);
        let res = wrapped.upload(Path::new("dummy"), "file.txt").await;
        assert!(res.is_ok());
    }
}
