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
