//! Circuit breaker middleware layer for fault isolation and cascading failure prevention.
//!
//! When a remote storage provider experiences continuous outages or rate limiting, the
//! circuit breaker opens to fail fast, preserving local bandwidth and threadpool resources.

use super::StorageLayer;
use cloud_sync_core::{
    ChangeFeed, RemoteChange, ResumableUploadSession, ResumableUploader, ServerSideOperations,
    StorageBackend, StorageCapabilities, StorageError, StorageItem,
};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Configuration parameters for the circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive transient failures before opening the circuit.
    pub failure_threshold: u32,
    /// Duration the circuit remains open before transitioning to HalfOpen.
    pub recovery_timeout: Duration,
    /// Number of successful probe operations required to close the circuit again.
    pub probe_success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            probe_success_threshold: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Closed,
    Open(Instant),
    HalfOpen,
}

struct BreakerInternal {
    state: State,
    consecutive_failures: u32,
    consecutive_successes: u32,
}

/// Composable storage layer wrapping a backend with circuit breaker protection.
#[derive(Clone)]
pub struct CircuitBreakerLayer {
    config: CircuitBreakerConfig,
}

impl CircuitBreakerLayer {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self { config }
    }
}

impl Default for CircuitBreakerLayer {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

impl StorageLayer for CircuitBreakerLayer {
    fn layer(&self, backend: Arc<dyn StorageBackend>) -> Arc<dyn StorageBackend> {
        Arc::new(CircuitBreakerBackend {
            inner: backend,
            config: self.config.clone(),
            internal: Arc::new(Mutex::new(BreakerInternal {
                state: State::Closed,
                consecutive_failures: 0,
                consecutive_successes: 0,
            })),
        })
    }
}

pub struct CircuitBreakerBackend {
    inner: Arc<dyn StorageBackend>,
    config: CircuitBreakerConfig,
    internal: Arc<Mutex<BreakerInternal>>,
}

impl CircuitBreakerBackend {
    /// Checks whether the circuit permits an operation to execute.
    fn before_call(&self) -> Result<(), StorageError> {
        let mut guard = self.internal.lock().unwrap();
        match guard.state {
            State::Closed => Ok(()),
            State::Open(opened_at) => {
                if opened_at.elapsed() >= self.config.recovery_timeout {
                    info!(
                        "[{}] Circuit breaker transitioning from Open to HalfOpen",
                        self.inner.name()
                    );
                    guard.state = State::HalfOpen;
                    guard.consecutive_successes = 0;
                    Ok(())
                } else {
                    let remaining = self.config.recovery_timeout.saturating_sub(opened_at.elapsed());
                    Err(StorageError::ConnectionFailed(format!(
                        "Circuit breaker open for provider '{}'; fast-failing for another {:.1?}",
                        self.inner.name(),
                        remaining
                    )))
                }
            }
            State::HalfOpen => Ok(()),
        }
    }

    /// Records outcome of an operation to update circuit breaker state.
    fn after_call<T>(&self, result: &Result<T, StorageError>) {
        let mut guard = self.internal.lock().unwrap();
        match result {
            Ok(_) => {
                match guard.state {
                    State::HalfOpen => {
                        guard.consecutive_successes += 1;
                        if guard.consecutive_successes >= self.config.probe_success_threshold {
                            info!(
                                "[{}] Circuit breaker recovered: HalfOpen -> Closed",
                                self.inner.name()
                            );
                            guard.state = State::Closed;
                            guard.consecutive_failures = 0;
                        }
                    }
                    State::Closed => {
                        guard.consecutive_failures = 0;
                    }
                    State::Open(_) => {}
                }
            }
            Err(err) => {
                if err.is_transient() {
                    guard.consecutive_failures += 1;
                    if guard.consecutive_failures >= self.config.failure_threshold {
                        warn!(
                            "[{}] Circuit breaker tripped after {} consecutive failures: Closed -> Open",
                            self.inner.name(),
                            guard.consecutive_failures
                        );
                        guard.state = State::Open(Instant::now());
                    }
                }
            }
        }
    }

    async fn execute<F, Fut, T>(&self, f: F) -> Result<T, StorageError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, StorageError>>,
    {
        self.before_call()?;
        let res = f().await;
        self.after_call(&res);
        res
    }
}

#[async_trait::async_trait]
impl StorageBackend for CircuitBreakerBackend {
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
        self.execute(|| self.inner.upload(local_path, remote_path)).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        self.execute(|| self.inner.download(remote_path, local_path)).await
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        self.execute(|| self.inner.delete(remote_path)).await
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        self.execute(|| self.inner.list(remote_path)).await
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        self.execute(|| self.inner.create_folder(remote_path)).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.execute(|| self.inner.rename(from, to)).await
    }

    async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.execute(|| self.inner.copy(from, to)).await
    }

    async fn start_resumable_upload(
        &self,
        local_path: &Path,
        remote_path: &str,
        chunk_size: usize,
    ) -> Result<Box<dyn ResumableUploadSession>, StorageError> {
        self.execute(|| self.inner.start_resumable_upload(local_path, remote_path, chunk_size)).await
    }

    async fn get_latest_cursor(&self) -> Result<Option<String>, StorageError> {
        self.execute(|| self.inner.get_latest_cursor()).await
    }

    async fn fetch_changes(
        &self,
        cursor: &str,
    ) -> Result<Option<(Vec<RemoteChange>, String)>, StorageError> {
        self.execute(|| self.inner.fetch_changes(cursor)).await
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
    use cloud_sync_core::StorageCapabilities;

    struct FlakyBackend {
        fail_count: Arc<Mutex<u32>>,
    }

    #[async_trait::async_trait]
    impl StorageBackend for FlakyBackend {
        fn name(&self) -> &str {
            "flaky"
        }
        fn capabilities(&self) -> StorageCapabilities {
            StorageCapabilities::NONE
        }
        async fn upload(&self, _local: &Path, _remote: &str) -> Result<(), StorageError> {
            let mut count = self.fail_count.lock().unwrap();
            *count += 1;
            Err(StorageError::ConnectionFailed("Backend unreachable".into()))
        }
        async fn download(&self, _remote: &str, _local: &Path) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete(&self, _remote: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list(&self, _remote: &str) -> Result<Vec<StorageItem>, StorageError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_trips_and_fast_fails() {
        let fail_count = Arc::new(Mutex::new(0));
        let backend = Arc::new(FlakyBackend {
            fail_count: fail_count.clone(),
        });

        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_millis(100),
            probe_success_threshold: 1,
        };

        let breaker = CircuitBreakerLayer::new(config).layer(backend);

        let dummy_path = Path::new("dummy");

        // First 3 calls fail against the actual backend
        for _ in 0..3 {
            assert!(breaker.upload(dummy_path, "test").await.is_err());
        }
        assert_eq!(*fail_count.lock().unwrap(), 3);

        // 4th call must fast-fail through circuit breaker without touching the backend
        let fast_fail_err = breaker.upload(dummy_path, "test").await.unwrap_err();
        assert_eq!(*fail_count.lock().unwrap(), 3); // Untouched!
        assert!(fast_fail_err.to_string().contains("Circuit breaker open"));

        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Probing call touches the backend again
        let _ = breaker.upload(dummy_path, "test").await;
        assert_eq!(*fail_count.lock().unwrap(), 4);
    }
}
