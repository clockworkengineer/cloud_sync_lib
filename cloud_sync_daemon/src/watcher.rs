//! Filesystem watcher event loop and full sync triggers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::sync::Mutex;
use notify::{Event, EventKind};
use tracing::{error, info, warn};
use cloud_sync_lib::StorageBackend;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::DaemonState;
use crate::{DEBOUNCE_DELAY_MS, RETRY_DELAY_MS, MAX_SYNC_ATTEMPTS};
use crate::utils::get_remote_path;

/// Builds a new Gitignore matcher based on .syncignore and app config excludes.
pub fn build_gitignore(watch_dir: &Path, exclude_patterns: &Option<Vec<String>>) -> Gitignore {
    let mut builder = GitignoreBuilder::new(watch_dir);
    let syncignore_path = watch_dir.join(".syncignore");
    if syncignore_path.exists() {
        if let Some(err) = builder.add(&syncignore_path) {
            warn!("Error loading .syncignore at {:?}: {}", syncignore_path, err);
        }
    }
    if let Some(ref excludes) = exclude_patterns {
        for pattern in excludes {
            if let Err(e) = builder.add_line(None, pattern) {
                warn!("Error parsing exclude pattern '{}': {}", pattern, e);
            }
        }
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

#[derive(Debug, Clone)]
pub struct ScannedItem {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: SystemTime,
}

/// Recursively scans a local directory, building a map of relative file paths to their metadata, applying Gitignore pattern exclusions.
pub async fn scan_local_directory(
    watch_dir: &Path,
    gitignore: &Gitignore,
) -> Result<HashMap<String, ScannedItem>, std::io::Error> {
    let mut files = HashMap::new();
    let mut queue = vec![watch_dir.to_path_buf()];

    while let Some(current_dir) = queue.pop() {
        if current_dir != watch_dir && gitignore.matched_path_or_any_parents(&current_dir, true).is_ignore() {
            continue;
        }

        let mut entries = fs::read_dir(current_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;

            if metadata.is_dir() {
                if gitignore.matched_path_or_any_parents(&path, true).is_ignore() {
                    continue;
                }
                queue.push(path.clone());
                if let Ok(rel_path) = path.strip_prefix(watch_dir) {
                    let rel_str = rel_path.to_string_lossy().to_string();
                    if !rel_str.is_empty() {
                        files.insert(rel_str, ScannedItem {
                            path: path.clone(),
                            is_dir: true,
                            size: 0,
                            modified: metadata.modified().unwrap_or(SystemTime::now()),
                        });
                    }
                }
            } else if metadata.is_file() {
                if gitignore.matched_path_or_any_parents(&path, false).is_ignore() {
                    continue;
                }
                if let Ok(rel_path) = path.strip_prefix(watch_dir) {
                    let rel_str = rel_path.to_string_lossy().to_string();
                    if rel_str == ".sync_state.json" || rel_str == ".syncignore" {
                        continue;
                    }
                    files.insert(rel_str, ScannedItem {
                        path: path.clone(),
                        is_dir: false,
                        size: metadata.len(),
                        modified: metadata.modified().unwrap_or(SystemTime::now()),
                    });
                }
            }
        }
    }

    Ok(files)
}

/// Scans the watch directory recursively and uploads all files to active backends.
pub async fn trigger_full_sync(watch_dir: &Path, backends: &[Arc<dyn StorageBackend>], gitignore: &Gitignore) -> std::io::Result<()> {
    let items = scan_local_directory(watch_dir, gitignore).await?;
    for (remote_path_str, item) in items {
        for backend in backends {
            let backend = backend.clone();
            let local_path = item.path.clone();
            let remote_path = remote_path_str.clone();
            let is_dir = item.is_dir;
            tokio::spawn(async move {
                if is_dir {
                    info!("[{}] Syncing directory '{}' via manual trigger", backend.name(), remote_path);
                    if let Err(e) = backend.create_folder(&remote_path).await {
                        error!("[{}] Failed to create directory '{}': {}", backend.name(), remote_path, e);
                    }
                } else {
                    info!("[{}] Syncing '{}' via manual trigger", backend.name(), remote_path);
                    if let Err(e) = backend.upload(&local_path, &remote_path).await {
                        error!("[{}] Failed to sync '{}': {}", backend.name(), remote_path, e);
                    } else {
                        info!("[{}] Successfully synced '{}'", backend.name(), remote_path);
                    }
                }
            });
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
    // Check if .syncignore itself changed
    let syncignore_changed = event.paths.iter().any(|p| {
        p.file_name().map_or(false, |name| name == ".syncignore")
    });

    if syncignore_changed {
        info!(".syncignore change detected. Rebuilding ignore rules...");
        let mut s = state.lock().await;
        s.gitignore = build_gitignore(&s.watch_dir, &s.exclude);
    }

    // Read current state
    let (paused, backends, watch_dir, gitignore) = {
        let s = state.lock().await;
        (s.paused, s.backends.clone(), s.watch_dir.clone(), s.gitignore.clone())
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

                // Canonicalize event path
                let abs_path = fs::canonicalize(&path).await.unwrap_or(path.clone());

                if gitignore.matched_path_or_any_parents(&abs_path, false).is_ignore() {
                    info!("Skipping excluded path: {:?}", abs_path);
                    continue;
                }

                // Make sure it is a file (we don't sync empty directories in this simple logic, but can be extended)
                let metadata = match fs::metadata(&path).await {
                    Ok(m) => m,
                    Err(e) => {
                        error!("Failed to read metadata for {:?}: {}", path, e);
                        continue;
                    }
                };

                let is_directory = metadata.is_dir();
                if !metadata.is_file() && !is_directory {
                    continue;
                }

                let remote_path_str = match get_remote_path(&abs_path, &watch_dir) {
                    Some(p) => p,
                    None => {
                        error!("Failed to strip prefix for {:?} (absolute: {:?})", path, abs_path);
                        continue;
                    }
                };
                info!("Path change detected: '{}' (dir: {}). Syncing to all cloud backends...", remote_path_str, is_directory);

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
                            let sync_res = if is_directory {
                                backend.create_folder(&remote_path).await
                            } else {
                                backend.upload(&local_path, &remote_path).await
                            };
                            match sync_res {
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
                if gitignore.matched_path_or_any_parents(&path, false).is_ignore() {
                    info!("Skipping deletion for excluded path: {:?}", path);
                    continue;
                }
                let remote_path_str = match get_remote_path(&path, &watch_dir) {
                    Some(p) => p,
                    None => {
                        error!("Failed to strip prefix for deleted path {:?}", path);
                        continue;
                    }
                };
                info!("File deletion detected: '{}'. Deleting from all cloud backends...", remote_path_str);

                for backend in &backends {
                    if !backend.sync() {
                        info!("[{}] Skipping remote deletion for '{}' because sync (deletions) is disabled.", backend.name(), remote_path_str);
                        continue;
                    }
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

    #[tokio::test]
    async fn test_watcher_deletions_sync_flag() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct TestBackend {
            name: String,
            sync: bool,
            delete_called: Arc<AtomicBool>,
        }

        #[async_trait::async_trait]
        impl StorageBackend for TestBackend {
            fn name(&self) -> &str {
                &self.name
            }
            async fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), cloud_sync_lib::StorageError> {
                Ok(())
            }
            async fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), cloud_sync_lib::StorageError> {
                Ok(())
            }
            async fn delete(&self, _remote_path: &str) -> Result<(), cloud_sync_lib::StorageError> {
                self.delete_called.store(true, Ordering::SeqCst);
                Ok(())
            }
            async fn list(&self, _remote_path: &str) -> Result<Vec<cloud_sync_lib::StorageItem>, cloud_sync_lib::StorageError> {
                Ok(vec![])
            }
            fn sync(&self) -> bool {
                self.sync
            }
        }

        let delete_called_sync_true = Arc::new(AtomicBool::new(false));
        let delete_called_sync_false = Arc::new(AtomicBool::new(false));

        let backend_true = Arc::new(TestBackend {
            name: "BackendTrue".to_string(),
            sync: true,
            delete_called: delete_called_sync_true.clone(),
        });
        let backend_false = Arc::new(TestBackend {
            name: "BackendFalse".to_string(),
            sync: false,
            delete_called: delete_called_sync_false.clone(),
        });

        let backends: Vec<Arc<dyn StorageBackend>> = vec![backend_true, backend_false];
        let state = Arc::new(Mutex::new(DaemonState {
            paused: false,
            backends,
            watch_dir: PathBuf::from("/home/user/watch"),
            config_file: "config.toml".to_string(),
            syncing: false,
            ui_addr: None,
            gitignore: Gitignore::empty(),
            exclude: None,
            upload_limiter: None,
            download_limiter: None,
        }));

        let active_locks = Arc::new(Mutex::new(HashMap::new()));
        let event = Event::new(EventKind::Remove(notify::event::RemoveKind::Any))
            .add_path(PathBuf::from("/home/user/watch/test.txt"));

        handle_event(event, state, active_locks).await;

        // Give any tokio spawns a short moment to execute
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(delete_called_sync_true.load(Ordering::SeqCst), "Backend with sync=true should have delete called");
        assert!(!delete_called_sync_false.load(Ordering::SeqCst), "Backend with sync=false should NOT have delete called");
    }

    #[test]
    fn test_build_gitignore_and_matching() {
        let watch_dir = Path::new("/home/user/watch");
        let exclude = Some(vec!["*.log".to_string(), "temp/".to_string()]);
        let gitignore = build_gitignore(watch_dir, &exclude);

        assert!(gitignore.matched_path_or_any_parents(Path::new("/home/user/watch/error.log"), false).is_ignore());
        assert!(!gitignore.matched_path_or_any_parents(Path::new("/home/user/watch/error.txt"), false).is_ignore());
        assert!(gitignore.matched_path_or_any_parents(Path::new("/home/user/watch/temp/file.txt"), false).is_ignore());
    }
}
