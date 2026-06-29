//! Filesystem watcher event loop and full sync triggers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::sync::Mutex;
use notify::{Event, EventKind};
use tracing::{error, info, warn};
use cloud_sync_lib::StorageBackend;

use crate::DaemonState;
use crate::{DEBOUNCE_DELAY_MS, RETRY_DELAY_MS, MAX_SYNC_ATTEMPTS};

/// Helper function to strip prefix from the watch directory path to get the relative remote path.
///
/// # Arguments
/// * `path` - Path of the file being synced.
/// * `watch_dir` - The watched root directory path.
///
/// # Returns
/// The normalized remote path string, or None if prefix stripping fails.
pub fn get_remote_path(path: &Path, watch_dir: &Path) -> Option<String> {
    let relative_path = match path.strip_prefix(watch_dir) {
        Ok(p) => p.to_path_buf(),
        Err(_) => {
            let path_str = path.to_string_lossy();
            let watch_dir_str = watch_dir.to_string_lossy();
            if path_str.starts_with(&*watch_dir_str) {
                Path::new(&path_str[watch_dir_str.len()..]).to_path_buf()
            } else {
                return None;
            }
        }
    };
    Some(relative_path.to_string_lossy().replace('\\', "/"))
}

/// Scans the watch directory recursively and uploads all files to active backends.
///
/// # Arguments
/// * `watch_dir` - The local directory root to scan.
/// * `backends` - Slice of active storage backends.
///
/// # Returns
/// `std::io::Result` indicating scanning success/failure.
pub async fn trigger_full_sync(watch_dir: &Path, backends: &[Arc<dyn StorageBackend>]) -> std::io::Result<()> {
    let mut dir_entries = vec![watch_dir.to_path_buf()];
    while let Some(current_dir) = dir_entries.pop() {
        let mut entries = fs::read_dir(current_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = fs::metadata(&path).await?;
            if metadata.is_dir() {
                dir_entries.push(path);
            } else if metadata.is_file() {
                if let Some(remote_path_str) = get_remote_path(&path, watch_dir) {
                    for backend in backends {
                        let backend = backend.clone();
                        let local_path = path.clone();
                        let remote_path = remote_path_str.clone();
                        tokio::spawn(async move {
                            info!("[{}] Syncing '{}' via manual trigger", backend.name(), remote_path);
                            if let Err(e) = backend.upload(&local_path, &remote_path).await {
                                error!("[{}] Failed to sync '{}': {}", backend.name(), remote_path, e);
                            } else {
                                info!("[{}] Successfully synced '{}'", backend.name(), remote_path);
                            }
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// Processes a filesystem notification event from `notify`.
///
/// Automatically creates, updates, or deletes files on remote backends based on local events.
///
/// # Arguments
/// * `event` - The filesystem event detail.
/// * `state` - The daemon's internal state.
/// * `active_locks` - Concurrent sync locking map for active files/backends.
pub async fn handle_event(
    event: Event,
    state: Arc<Mutex<DaemonState>>,
    active_locks: Arc<Mutex<HashMap<(String, PathBuf), Arc<tokio::sync::Mutex<()>>>>>,
) {
    // Read current state
    let (paused, backends, watch_dir) = {
        let s = state.lock().await;
        (s.paused, s.backends.clone(), s.watch_dir.clone())
    };

    if paused {
        info!("Daemon is paused. Skipping file change event.");
        return;
    }

    // Only respond to creation, modification (writes), and deletions
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(notify::event::ModifyKind::Data(_)) | EventKind::Modify(notify::event::ModifyKind::Any) => {
            for path in event.paths {
                if !path.exists() {
                    continue; // Skip if file was deleted before we could process it
                }

                // Make sure it is a file (we don't sync empty directories in this simple logic, but can be extended)
                let metadata = match fs::metadata(&path).await {
                    Ok(m) => m,
                    Err(e) => {
                        error!("Failed to read metadata for {:?}: {}", path, e);
                        continue;
                    }
                };

                if !metadata.is_file() {
                    continue;
                }

                // Canonicalize event path
                let abs_path = fs::canonicalize(&path).await.unwrap_or(path.clone());

                let remote_path_str = match get_remote_path(&abs_path, &watch_dir) {
                    Some(p) => p,
                    None => {
                        error!("Failed to strip prefix for {:?} (absolute: {:?})", path, abs_path);
                        continue;
                    }
                };
                info!("File change detected: '{}'. Syncing to all cloud backends...", remote_path_str);

                for backend in &backends {
                    let backend = backend.clone();
                    let local_path = path.clone();
                    let remote_path = remote_path_str.clone();

                    let key = (backend.name().to_string(), local_path.clone());
                    let file_mutex = {
                        let mut locks = active_locks.lock().await;
                        locks.entry(key).or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))).clone()
                    };

                    tokio::spawn(async move {
                        // Sequential lock to prevent concurrent uploads for the same file/backend
                        let _guard = file_mutex.lock().await;

                        // Debounce: wait briefly for concurrent writes/events to settle
                        tokio::time::sleep(Duration::from_millis(DEBOUNCE_DELAY_MS)).await;

                        // Add minor delay/retry logic in case the file is still being written to by the OS/editor
                        let mut attempts = MAX_SYNC_ATTEMPTS;
                        while attempts > 0 {
                            match backend.upload(&local_path, &remote_path).await {
                                Ok(_) => {
                                    info!("[{}] Successfully synced '{}'", backend.name(), remote_path);
                                    break;
                                }
                                Err(e) => {
                                    warn!(
                                        "[{}] Attempt failed to sync '{}': {}. Retrying in {}ms...",
                                        backend.name(),
                                        remote_path,
                                        e,
                                        RETRY_DELAY_MS
                                    );
                                    tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
                                    attempts -= 1;
                                }
                            }
                        }
                        if attempts == 0 {
                            error!(
                                "[{}] Failed to sync '{}' after multiple attempts.",
                                backend.name(),
                                remote_path
                            );
                        }
                    });
                }
            }
        }
        EventKind::Remove(_) => {
            for path in event.paths {
                let remote_path_str = match get_remote_path(&path, &watch_dir) {
                    Some(p) => p,
                    None => {
                        error!("Failed to strip prefix for deleted path {:?}", path);
                        continue;
                    }
                };
                info!("File deletion detected: '{}'. Deleting from all cloud backends...", remote_path_str);

                for backend in &backends {
                    let backend = backend.clone();
                    let remote_path = remote_path_str.clone();

                    tokio::spawn(async move {
                        match backend.delete(&remote_path).await {
                            Ok(_) => info!("[{}] Successfully deleted remote file '{}'", backend.name(), remote_path),
                            Err(cloud_sync_lib::StorageError::NotFound(_)) => {
                                // Already deleted or doesn't exist
                            }
                            Err(e) => error!("[{}] Failed to delete '{}': {}", backend.name(), remote_path, e),
                        }
                    });
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that `get_remote_path` maps watched local file paths correctly and bounds-checks unrelated files.
    #[test]
    fn test_get_remote_path() {
        let watch_dir = Path::new("/home/user/watch");
        let file_path = Path::new("/home/user/watch/docs/report.pdf");
        assert_eq!(
            get_remote_path(file_path, watch_dir),
            Some("docs/report.pdf".to_string())
        );

        let unrelated_path = Path::new("/home/user/other/report.pdf");
        assert_eq!(get_remote_path(unrelated_path, watch_dir), None);
    }
}
