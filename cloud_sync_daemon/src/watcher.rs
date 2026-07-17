//! Filesystem watcher event loop and full sync triggers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::sync::Mutex;
use notify::{Event, EventKind};
use tracing::{error, info, warn};
use cloud_sync_lib::{StorageBackend, SyncIgnore};

use crate::{DaemonState, ActiveBackend};
use crate::{DEBOUNCE_DELAY_MS, RETRY_DELAY_MS, MAX_SYNC_ATTEMPTS};
use crate::utils::get_remote_path;

pub type ActiveLocks = Arc<Mutex<HashMap<(String, PathBuf), Arc<tokio::sync::Mutex<()>>>>>;

/// Builds a new SyncIgnore matcher based on .syncignore and app config excludes.
pub fn build_gitignore(watch_dir: &Path, exclude_patterns: &Option<Vec<String>>) -> SyncIgnore {
    let excludes = exclude_patterns.as_ref().cloned().unwrap_or_default();
    SyncIgnore::new(watch_dir, &excludes)
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
    gitignore: &SyncIgnore,
) -> Result<HashMap<String, ScannedItem>, std::io::Error> {
    let mut files = HashMap::new();
    let mut queue = vec![watch_dir.to_path_buf()];

    while let Some(current_dir) = queue.pop() {
        if current_dir != watch_dir && gitignore.is_ignored(&current_dir, true) {
            continue;
        }

        let mut entries = fs::read_dir(current_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;

            if metadata.is_dir() {
                if gitignore.is_ignored(&path, true) {
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
                if gitignore.is_ignored(&path, false) {
                    continue;
                }
                if let Ok(rel_path) = path.strip_prefix(watch_dir) {
                    let rel_str = rel_path.to_string_lossy().to_string();
                    if rel_str == ".sync_state.json" || rel_str == ".sync_state.bin" || rel_str == ".syncignore" || (rel_str.starts_with(".sync_state_") && (rel_str.ends_with(".json") || rel_str.ends_with(".bin"))) {
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
pub async fn trigger_full_sync(watch_dir: &Path, backends: &[ActiveBackend], gitignore: &SyncIgnore) -> std::io::Result<()> {
    let items = scan_local_directory(watch_dir, gitignore).await?;
    for (remote_path_str, item) in items {
        for active_backend in backends {
            let backend = active_backend.backend.clone();
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
    active_locks: ActiveLocks,
) {
    // Check if .syncignore itself changed
    let syncignore_changed = event.paths.iter().any(|p| {
        p.file_name().is_some_and(|name| name == ".syncignore")
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

                // Make sure it is a file or directory
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

                // Canonicalize event path
                let abs_path = fs::canonicalize(&path).await.unwrap_or(path.clone());

                if gitignore.is_ignored(&abs_path, is_directory) {
                    info!("Skipping excluded path: {:?}", abs_path);
                    continue;
                }

                let remote_path_str = match get_remote_path(&abs_path, &watch_dir) {
                    Some(p) => p,
                    None => {
                        error!("Failed to strip prefix for {:?} (absolute: {:?})", path, abs_path);
                        continue;
                    }
                };
                if remote_path_str == ".sync_state.json" || remote_path_str == ".sync_state.bin" || remote_path_str == ".syncignore" || (remote_path_str.starts_with(".sync_state_") && (remote_path_str.ends_with(".json") || remote_path_str.ends_with(".bin"))) {
                    continue;
                }
                info!("Path change detected: '{}' (dir: {}). Syncing to all cloud backends...", remote_path_str, is_directory);

                for active_backend in &backends {
                    let backend = active_backend.backend.clone();
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
                if gitignore.is_ignored(&path, false) || gitignore.is_ignored(&path, true) {
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
                if remote_path_str == ".sync_state.json" || remote_path_str == ".sync_state.bin" || remote_path_str == ".syncignore" || (remote_path_str.starts_with(".sync_state_") && (remote_path_str.ends_with(".json") || remote_path_str.ends_with(".bin"))) {
                    continue;
                }
                info!("File deletion detected: '{}'. Deleting from all cloud backends...", remote_path_str);

                for active_backend in &backends {
                    let backend = active_backend.backend.clone();
                    if !active_backend.policy.sync_deletions() {
                        info!("[{}] Skipping remote deletion for '{}' because sync (deletions) is disabled.", backend.name(), remote_path_str);
                        continue;
                    }
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
        }

        let delete_called_sync_true = Arc::new(AtomicBool::new(false));
        let delete_called_sync_false = Arc::new(AtomicBool::new(false));

        let backend_true = Arc::new(TestBackend {
            name: "BackendTrue".to_string(),
            delete_called: delete_called_sync_true.clone(),
        });
        let backend_false = Arc::new(TestBackend {
            name: "BackendFalse".to_string(),
            delete_called: delete_called_sync_false.clone(),
        });

        let backends = vec![
            ActiveBackend {
                backend: backend_true,
                policy: cloud_sync_lib::SyncPolicy::new(cloud_sync_lib::SyncMode::OneWay),
            },
            ActiveBackend {
                backend: backend_false,
                policy: cloud_sync_lib::SyncPolicy::new(cloud_sync_lib::SyncMode::OneWayNoDeletions),
            },
        ];
        let state = Arc::new(Mutex::new(DaemonState {
            paused: false,
            backends,
            watch_dir: PathBuf::from("/home/user/watch"),
            config_file: "config.toml".to_string(),
            syncing: false,
            ui_addr: None,
            gitignore: SyncIgnore::empty(),
            exclude: None,
            upload_limiter: None,
            download_limiter: None,
            max_concurrency: 4,
            connection_errors: HashMap::new(),
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
        let temp_dir = tempfile::tempdir().unwrap();
        let watch_dir = temp_dir.path();
        let exclude = Some(vec!["*.log".to_string(), "temp/".to_string()]);
        let gitignore = build_gitignore(watch_dir, &exclude);

        assert!(gitignore.is_ignored(watch_dir.join("error.log"), false));
        assert!(!gitignore.is_ignored(watch_dir.join("error.txt"), false));
        assert!(gitignore.is_ignored(watch_dir.join("temp/file.txt"), false));
        // Verify that deleting a directory matched by trailing slash is ignored by checking both false and true
        assert!(gitignore.is_ignored(watch_dir.join("temp"), false) || gitignore.is_ignored(watch_dir.join("temp"), true));
    }

    #[tokio::test]
    async fn test_scan_local_directory_ignores_sync_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let watch_dir = temp_dir.path();
        
        tokio::fs::write(watch_dir.join("test.txt"), "hello").await.unwrap();
        tokio::fs::write(watch_dir.join(".sync_state.bin"), "{}").await.unwrap();
        tokio::fs::write(watch_dir.join(".syncignore"), "*.log").await.unwrap();

        let gitignore = build_gitignore(watch_dir, &None);
        let items = scan_local_directory(watch_dir, &gitignore).await.unwrap();

        assert!(items.contains_key("test.txt"));
        assert!(!items.contains_key(".sync_state.bin"));
        assert!(!items.contains_key(".syncignore"));
    }

    #[tokio::test]
    async fn test_handle_event_ignores_sync_files() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct TestBackend {
            called: Arc<AtomicBool>,
        }

        #[async_trait::async_trait]
        impl StorageBackend for TestBackend {
            fn name(&self) -> &str {
                "TestBackend"
            }
            async fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), cloud_sync_lib::StorageError> {
                self.called.store(true, Ordering::SeqCst);
                Ok(())
            }
            async fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), cloud_sync_lib::StorageError> {
                Ok(())
            }
            async fn delete(&self, _remote_path: &str) -> Result<(), cloud_sync_lib::StorageError> {
                self.called.store(true, Ordering::SeqCst);
                Ok(())
            }
            async fn list(&self, _remote_path: &str) -> Result<Vec<cloud_sync_lib::StorageItem>, cloud_sync_lib::StorageError> {
                Ok(vec![])
            }
        }

        let called = Arc::new(AtomicBool::new(false));
        let backend = Arc::new(TestBackend { called: called.clone() });

        let backends = vec![ActiveBackend {
            backend,
            policy: cloud_sync_lib::SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
        }];

        let temp_dir = tempfile::tempdir().unwrap();
        let watch_dir = temp_dir.path().to_path_buf();
        
        let state = Arc::new(Mutex::new(DaemonState {
            paused: false,
            backends,
            watch_dir: watch_dir.clone(),
            config_file: "config.toml".to_string(),
            syncing: false,
            ui_addr: None,
            gitignore: SyncIgnore::empty(),
            exclude: None,
            upload_limiter: None,
            download_limiter: None,
            max_concurrency: 4,
            connection_errors: HashMap::new(),
        }));

        let active_locks = Arc::new(Mutex::new(HashMap::new()));
        
        // 1. Check Create event for .sync_state.bin
        let sync_state_file = watch_dir.join(".sync_state.bin");
        tokio::fs::write(&sync_state_file, "{}").await.unwrap();
        let event = Event::new(EventKind::Create(notify::event::CreateKind::File))
            .add_path(sync_state_file.clone());
        handle_event(event, state.clone(), active_locks.clone()).await;

        // 2. Check Remove event for .sync_state.bin
        let event_remove = Event::new(EventKind::Remove(notify::event::RemoveKind::File))
            .add_path(sync_state_file);
        handle_event(event_remove, state.clone(), active_locks.clone()).await;

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!called.load(Ordering::SeqCst), "Should not trigger upload/delete operations for internal sync files");
    }
}
