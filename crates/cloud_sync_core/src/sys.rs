use alloc::boxed::Box;
#[cfg(feature = "std")]
use std::path::Path;
#[cfg(feature = "std")]
use std::time::SystemTime;

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

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_system_clock() {
        let clock = SystemClock;
        let t1 = clock.now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let t2 = clock.now();
        assert!(t2 >= t1);
    }

    #[tokio::test]
    async fn test_real_filesystem_operations() {
        let dir = tempdir().unwrap();
        let fs = RealFileSystem;

        let sub_dir = dir.path().join("sub/nested");
        fs.create_dir_all(&sub_dir).await.expect("create_dir_all should succeed");
        assert!(sub_dir.is_dir());

        let file_a = sub_dir.join("test.txt");
        tokio::fs::write(&file_a, b"hello world").await.unwrap();

        let meta = fs.metadata(&file_a).await.expect("metadata should succeed");
        assert_eq!(meta.len(), 11);

        let file_b = sub_dir.join("copy.txt");
        let bytes_copied = fs.copy(&file_a, &file_b).await.expect("copy should succeed");
        assert_eq!(bytes_copied, 11);
        assert!(file_b.is_file());

        fs.remove_file(&file_b).await.expect("remove_file should succeed");
        assert!(!file_b.exists());

        fs.remove_dir_all(&sub_dir).await.expect("remove_dir_all should succeed");
        assert!(!sub_dir.exists());
    }
}
