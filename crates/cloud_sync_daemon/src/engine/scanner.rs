//! Directory tree scanning and metadata discovery (SRP).

use cloud_sync_lib::{path::get_permissions, StorageBackend, StorageError};
use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use crate::engine::filter::FileFilter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedItem {
    pub size: u64,
    pub modified: SystemTime,
    pub is_dir: bool,
    pub checksum: Option<String>,
    pub permissions: Option<u32>,
}

pub struct DirectoryScanner;

impl DirectoryScanner {
    /// Recursively scans remote storage directory and returns normalized relative path map.
    pub async fn scan_remote(
        backend: &dyn StorageBackend,
        filter: &FileFilter<'_>,
    ) -> Result<HashMap<String, ScannedItem>, StorageError> {
        let mut files = HashMap::new();
        let mut queue = vec!["".to_string()];

        while let Some(current) = queue.pop() {
            match backend.list(&current).await {
                Ok(items) => {
                    for item in items {
                        let path_str = item.path.to_string_lossy().to_string();
                        if FileFilter::is_unsafe_path(&path_str)
                            || FileFilter::is_state_file(&path_str)
                        {
                            continue;
                        }
                        if filter.is_ignored(&item.path, item.is_dir) {
                            continue;
                        }
                        if !filter.is_path_selected(&path_str) {
                            continue;
                        }

                        if item.is_dir {
                            queue.push(path_str.clone());
                            files.insert(
                                path_str,
                                ScannedItem {
                                    size: 0,
                                    modified: item.modified,
                                    is_dir: true,
                                    checksum: None,
                                    permissions: item.permissions,
                                },
                            );
                        } else {
                            if !filter.is_size_allowed(item.size) {
                                continue;
                            }
                            files.insert(
                                path_str,
                                ScannedItem {
                                    size: item.size,
                                    modified: item.modified,
                                    is_dir: false,
                                    checksum: item.checksum,
                                    permissions: item.permissions,
                                },
                            );
                        }
                    }
                }
                Err(StorageError::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }

        Ok(files)
    }

    /// Recursively scans local watched folder and returns normalized relative path map.
    pub async fn scan_local(
        watch_dir: &Path,
        filter: &FileFilter<'_>,
    ) -> Result<HashMap<String, ScannedItem>, std::io::Error> {
        let mut files = HashMap::new();
        if !watch_dir.exists() {
            return Ok(files);
        }

        let mut queue = vec![watch_dir.to_path_buf()];
        while let Some(dir) = queue.pop() {
            let mut entries = tokio::fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let rel_path = match path.strip_prefix(watch_dir) {
                    Ok(p) => p.to_string_lossy().replace('\\', "/"),
                    Err(_) => continue,
                };

                if FileFilter::is_unsafe_path(&rel_path) || FileFilter::is_state_file(&rel_path) {
                    continue;
                }

                let meta = match entry.metadata().await {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let is_dir = meta.is_dir();

                if filter.is_ignored(Path::new(&rel_path), is_dir) {
                    continue;
                }
                if !filter.is_path_selected(&rel_path) {
                    continue;
                }

                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

                if is_dir {
                    queue.push(path);
                    files.insert(
                        rel_path,
                        ScannedItem {
                            size: 0,
                            modified,
                            is_dir: true,
                            checksum: None,
                            permissions: get_permissions(&meta.permissions()),
                        },
                    );
                } else {
                    if !filter.is_size_allowed(meta.len()) {
                        continue;
                    }
                    files.insert(
                        rel_path,
                        ScannedItem {
                            size: meta.len(),
                            modified,
                            is_dir: false,
                            checksum: None,
                            permissions: get_permissions(&meta.permissions()),
                        },
                    );
                }
            }
        }

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_sync_lib::{StorageCapabilities, StorageItem};
    use tempfile::tempdir;

    struct MockScannerBackend {
        items: Vec<StorageItem>,
        fail_not_found: bool,
    }

    #[async_trait::async_trait]
    impl StorageBackend for MockScannerBackend {
        fn name(&self) -> &str {
            "mock-scanner"
        }
        fn capabilities(&self) -> StorageCapabilities {
            StorageCapabilities::NONE
        }
        async fn upload(&self, _local: &Path, _remote: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn download(&self, _remote: &str, _local: &Path) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete(&self, _remote: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
            if self.fail_not_found {
                return Err(StorageError::NotFound("not found".to_string()));
            }
            if remote_path.is_empty() {
                Ok(self.items.clone())
            } else {
                Ok(vec![])
            }
        }
    }

    #[tokio::test]
    async fn test_scan_local_and_filters() {
        let temp_dir = tempdir().unwrap();
        let watch_dir = temp_dir.path().join("watch");

        let gitignore = cloud_sync_lib::SyncIgnore::new(&watch_dir, &[]);
        let filter = FileFilter::new(&gitignore, None);

        // 1. Non-existent watch dir returns empty map
        let empty = DirectoryScanner::scan_local(&watch_dir, &filter).await.unwrap();
        assert!(empty.is_empty());

        // 2. Create watch directory structure
        tokio::fs::create_dir_all(&watch_dir).await.unwrap();
        let sub_dir = watch_dir.join("subfolder");
        tokio::fs::create_dir_all(&sub_dir).await.unwrap();

        let file1 = watch_dir.join("file1.txt");
        let file2 = sub_dir.join("file2.txt");
        let state_file = watch_dir.join(".sync_state_test.bin");
        tokio::fs::write(&file1, b"hello").await.unwrap();
        tokio::fs::write(&file2, b"world!").await.unwrap();
        tokio::fs::write(&state_file, b"state").await.unwrap();

        let scanned = DirectoryScanner::scan_local(&watch_dir, &filter).await.unwrap();
        assert!(scanned.contains_key("subfolder"));
        assert!(scanned.contains_key("file1.txt"));
        assert!(scanned.contains_key("subfolder/file2.txt"));
        assert!(!scanned.contains_key(".sync_state_test.bin")); // filtered out

        assert!(scanned.get("subfolder").unwrap().is_dir);
        assert!(!scanned.get("file1.txt").unwrap().is_dir);
        assert_eq!(scanned.get("file1.txt").unwrap().size, 5);
    }

    #[tokio::test]
    async fn test_scan_remote_and_filters() {
        let temp_dir = tempdir().unwrap();
        let gitignore = cloud_sync_lib::SyncIgnore::new(temp_dir.path(), &[]);
        let filter = FileFilter::new(&gitignore, None);

        // 1. Remote list returning NotFound
        let not_found_backend = MockScannerBackend {
            items: vec![],
            fail_not_found: true,
        };
        let res = DirectoryScanner::scan_remote(&not_found_backend, &filter).await.unwrap();
        assert!(res.is_empty());

        // 2. Remote list returning folder and file
        let items = vec![
            StorageItem {
                path: std::path::PathBuf::from("docs"),
                size: 0,
                modified: SystemTime::now(),
                is_dir: true,
                checksum: None,
                permissions: None,
            },
            StorageItem {
                path: std::path::PathBuf::from("report.pdf"),
                size: 1024,
                modified: SystemTime::now(),
                is_dir: false,
                checksum: Some("chk123".to_string()),
                permissions: Some(0o644),
            },
            StorageItem {
                path: std::path::PathBuf::from(".sync_state_remote.bin"),
                size: 50,
                modified: SystemTime::now(),
                is_dir: false,
                checksum: None,
                permissions: None,
            },
        ];

        let backend = MockScannerBackend {
            items,
            fail_not_found: false,
        };

        let remote_scanned = DirectoryScanner::scan_remote(&backend, &filter).await.unwrap();
        assert!(remote_scanned.contains_key("docs"));
        assert!(remote_scanned.contains_key("report.pdf"));
        assert!(!remote_scanned.contains_key(".sync_state_remote.bin"));
        assert_eq!(remote_scanned.get("report.pdf").unwrap().size, 1024);
        assert_eq!(
            remote_scanned.get("report.pdf").unwrap().checksum.as_deref(),
            Some("chk123")
        );
    }
}
