#[cfg(feature = "std")]
use std::path::Path;
#[cfg(feature = "std")]
use std::time::SystemTime;
use alloc::boxed::Box;


/// Abstraction for system clock operations enabling deterministic testing.
#[cfg(feature = "std")]
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

/// Standard system clock implementation using `SystemTime::now()`.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

#[cfg(feature = "std")]
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Abstraction for filesystem operations enabling virtual/in-memory filesystem testing.
#[cfg(feature = "std")]
#[async_trait::async_trait]
pub trait FileSystem: Send + Sync {
    async fn metadata(&self, path: &Path) -> Result<std::fs::Metadata, std::io::Error>;
    async fn create_dir_all(&self, path: &Path) -> Result<(), std::io::Error>;
    async fn remove_file(&self, path: &Path) -> Result<(), std::io::Error>;
    async fn remove_dir_all(&self, path: &Path) -> Result<(), std::io::Error>;
    async fn copy(&self, from: &Path, to: &Path) -> Result<u64, std::io::Error>;
}

/// Real filesystem implementation using `tokio::fs`.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFileSystem;

#[cfg(feature = "std")]
#[async_trait::async_trait]
impl FileSystem for RealFileSystem {
    async fn metadata(&self, path: &Path) -> Result<std::fs::Metadata, std::io::Error> {
        tokio::fs::metadata(path).await
    }
    async fn create_dir_all(&self, path: &Path) -> Result<(), std::io::Error> {
        tokio::fs::create_dir_all(path).await
    }
    async fn remove_file(&self, path: &Path) -> Result<(), std::io::Error> {
        tokio::fs::remove_file(path).await
    }
    async fn remove_dir_all(&self, path: &Path) -> Result<(), std::io::Error> {
        tokio::fs::remove_dir_all(path).await
    }
    async fn copy(&self, from: &Path, to: &Path) -> Result<u64, std::io::Error> {
        tokio::fs::copy(from, to).await
    }
}
