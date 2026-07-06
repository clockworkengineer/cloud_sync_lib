use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;
use cloud_sync_lib::{StorageBackend, SyncState, FileState};
use tracing::info;

#[derive(Debug)]
struct FileInfo {
    size: u64,
    modified: SystemTime,
}

/// Scans the local watched directory recursively and builds a map of relative file paths to their metadata.
async fn scan_local_dir(
    watch_dir: &Path,
    gitignore: &ignore::gitignore::Gitignore,
) -> Result<HashMap<String, FileInfo>, std::io::Error> {
    let mut files = HashMap::new();
    let mut queue = vec![watch_dir.to_path_buf()];

    while let Some(current_dir) = queue.pop() {
        if current_dir != watch_dir && gitignore.matched_path_or_any_parents(&current_dir, true).is_ignore() {
            continue;
        }

        let mut entries = tokio::fs::read_dir(current_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;

            if metadata.is_dir() {
                if gitignore.matched_path_or_any_parents(&path, true).is_ignore() {
                    continue;
                }
                queue.push(path);
            } else if metadata.is_file() {
                if gitignore.matched_path_or_any_parents(&path, false).is_ignore() {
                    continue;
                }
                if let Ok(rel_path) = path.strip_prefix(watch_dir) {
                    let rel_str = rel_path.to_string_lossy().to_string();
                    if rel_str == ".sync_state.json" || rel_str == ".syncignore" {
                        continue;
                    }
                    files.insert(rel_str, FileInfo {
                        size: metadata.len(),
                        modified: metadata.modified().unwrap_or(SystemTime::now()),
                    });
                }
            }
        }
    }

    Ok(files)
}

/// Scans the remote storage directory recursively and builds a map of relative file paths to their metadata.
async fn scan_remote_dir(
    backend: &dyn StorageBackend,
) -> Result<HashMap<String, FileInfo>, Box<dyn std::error::Error>> {
    let mut files = HashMap::new();
    let mut queue = vec!["".to_string()];

    while let Some(current) = queue.pop() {
        match backend.list(&current).await {
            Ok(items) => {
                for item in items {
                    let path_str = item.path.to_string_lossy().to_string();
                    if item.is_dir {
                        queue.push(path_str);
                    } else {
                        files.insert(path_str, FileInfo {
                            size: item.size,
                            modified: item.modified,
                        });
                    }
                }
            }
            Err(cloud_sync_lib::StorageError::NotFound(_)) => {}
            Err(e) => return Err(Box::new(e)),
        }
    }

    Ok(files)
}

async fn get_local_mtime(path: &Path) -> Option<SystemTime> {
    tokio::fs::metadata(path).await.ok().and_then(|m| m.modified().ok())
}

async fn get_remote_mtime(backend: &dyn StorageBackend, rel_path: &str) -> Option<SystemTime> {
    let path = Path::new(rel_path);
    let parent = path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let file_name = path.file_name()?.to_string_lossy();

    let items = backend.list(&parent).await.ok()?;
    for item in items {
        if let Some(item_name) = item.path.file_name() {
            if item_name.to_string_lossy() == file_name {
                return Some(item.modified);
            }
        }
    }
    None
}

async fn resolve_conflict(
    watch_dir: &Path,
    rel_path: &str,
    backend: &dyn StorageBackend,
    local_size: u64,
    _local_modified: SystemTime,
    remote_size: u64,
    remote_modified: SystemTime,
    next_files_state: &mut HashMap<String, FileState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let local_file_path = watch_dir.join(rel_path);
    let conflict_rel_path = format!("{}.local-conflict", rel_path);
    let conflict_local_path = watch_dir.join(&conflict_rel_path);

    info!("Renaming conflicting local file to: {:?}", conflict_local_path);
    tokio::fs::rename(&local_file_path, &conflict_local_path).await?;

    info!("Uploading conflict copy '{}' to remote", conflict_rel_path);
    backend.upload(&conflict_local_path, &conflict_rel_path).await?;
    let conflict_remote_mtime = get_remote_mtime(backend, &conflict_rel_path).await.unwrap_or(SystemTime::now());
    let conflict_local_mtime = get_local_mtime(&conflict_local_path).await.unwrap_or(SystemTime::now());

    next_files_state.insert(conflict_rel_path, FileState {
        size: local_size,
        local_modified: conflict_local_mtime,
        remote_modified: conflict_remote_mtime,
    });

    info!("Downloading remote file '{}' to original local path", rel_path);
    backend.download(rel_path, &local_file_path).await?;
    let replaced_local_mtime = get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now());

    next_files_state.insert(rel_path.to_string(), FileState {
        size: remote_size,
        local_modified: replaced_local_mtime,
        remote_modified: remote_modified,
    });

    Ok(())
}

/// Main entry point for performing a bidirectional synchronization step.
pub async fn sync_bidirectional(
    watch_dir: &Path,
    backend: &dyn StorageBackend,
    state_file_path: &Path,
    gitignore: &ignore::gitignore::Gitignore,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load sync state catalog
    let mut sync_state = SyncState::load(state_file_path).await.unwrap_or_default();

    // 2. Scan directories
    let local_files = scan_local_dir(watch_dir, gitignore).await?;
    let remote_files = scan_remote_dir(backend).await?;

    // 3. Process changes
    let mut next_files_state = HashMap::new();
    let mut all_paths = HashSet::new();

    for path in local_files.keys() {
        all_paths.insert(path.clone());
    }
    for path in remote_files.keys() {
        all_paths.insert(path.clone());
    }
    for path in sync_state.files.keys() {
        all_paths.insert(path.clone());
    }

    for rel_path in all_paths {
        let local_opt = local_files.get(&rel_path);
        let remote_opt = remote_files.get(&rel_path);
        let state_opt = sync_state.files.get(&rel_path);

        match (local_opt, remote_opt, state_opt) {
            // Case 1: Exists everywhere (check for modifications)
            (Some(local), Some(remote), Some(state)) => {
                let local_changed = local.size != state.size || local.modified != state.local_modified;
                let remote_changed = remote.size != state.size || remote.modified != state.remote_modified;

                if local_changed && remote_changed {
                    resolve_conflict(watch_dir, &rel_path, backend, local.size, local.modified, remote.size, remote.modified, &mut next_files_state).await?;
                } else if local_changed {
                    info!("Bidirectional: uploading local modification '{}' to remote", rel_path);
                    backend.upload(&watch_dir.join(&rel_path), &rel_path).await?;
                    next_files_state.insert(rel_path.clone(), FileState {
                        size: local.size,
                        local_modified: local.modified,
                        remote_modified: get_remote_mtime(backend, &rel_path).await.unwrap_or(remote.modified),
                    });
                } else if remote_changed {
                    info!("Bidirectional: downloading remote modification '{}' to local", rel_path);
                    backend.download(&rel_path, &watch_dir.join(&rel_path)).await?;
                    let new_local_mtime = get_local_mtime(&watch_dir.join(&rel_path)).await.unwrap_or(local.modified);
                    next_files_state.insert(rel_path.clone(), FileState {
                        size: remote.size,
                        local_modified: new_local_mtime,
                        remote_modified: remote.modified,
                    });
                } else {
                    next_files_state.insert(rel_path.clone(), state.clone());
                }
            }
            // Case 2: Local & Remote, but not in state catalog (e.g. concurrent initial additions)
            (Some(local), Some(remote), None) => {
                if local.size == remote.size {
                    next_files_state.insert(rel_path.clone(), FileState {
                        size: local.size,
                        local_modified: local.modified,
                        remote_modified: remote.modified,
                    });
                } else {
                    resolve_conflict(watch_dir, &rel_path, backend, local.size, local.modified, remote.size, remote.modified, &mut next_files_state).await?;
                }
            }
            // Case 3: Local-only, not in state catalog (New local file)
            (Some(local), None, None) => {
                info!("Bidirectional: uploading new local file '{}' to remote", rel_path);
                backend.upload(&watch_dir.join(&rel_path), &rel_path).await?;
                next_files_state.insert(rel_path.clone(), FileState {
                    size: local.size,
                    local_modified: local.modified,
                    remote_modified: get_remote_mtime(backend, &rel_path).await.unwrap_or(SystemTime::now()),
                });
            }
            // Case 4: Remote-only, not in state catalog (New remote file)
            (None, Some(remote), None) => {
                info!("Bidirectional: downloading new remote file '{}' to local", rel_path);
                backend.download(&rel_path, &watch_dir.join(&rel_path)).await?;
                let new_local_mtime = get_local_mtime(&watch_dir.join(&rel_path)).await.unwrap_or(SystemTime::now());
                next_files_state.insert(rel_path.clone(), FileState {
                    size: remote.size,
                    local_modified: new_local_mtime,
                    remote_modified: remote.modified,
                });
            }
            // Case 5: Local & State, but missing remote (Deleted remotely)
            (Some(local), None, Some(state)) => {
                let local_changed = local.size != state.size || local.modified != state.local_modified;
                if local_changed {
                    info!("Bidirectional: re-uploading modified local file '{}' that was deleted remotely", rel_path);
                    backend.upload(&watch_dir.join(&rel_path), &rel_path).await?;
                    next_files_state.insert(rel_path.clone(), FileState {
                        size: local.size,
                        local_modified: local.modified,
                        remote_modified: get_remote_mtime(backend, &rel_path).await.unwrap_or(SystemTime::now()),
                    });
                } else {
                    info!("Bidirectional: deleting local file '{}' since it was deleted remotely", rel_path);
                    let local_path = watch_dir.join(&rel_path);
                    let _ = tokio::fs::remove_file(local_path).await;
                }
            }
            // Case 6: Remote & State, but missing local (Deleted locally)
            (None, Some(remote), Some(state)) => {
                let remote_changed = remote.size != state.size || remote.modified != state.remote_modified;
                if remote_changed {
                    info!("Bidirectional: re-downloading modified remote file '{}' that was deleted locally", rel_path);
                    backend.download(&rel_path, &watch_dir.join(&rel_path)).await?;
                    let new_local_mtime = get_local_mtime(&watch_dir.join(&rel_path)).await.unwrap_or(remote.modified);
                    next_files_state.insert(rel_path.clone(), FileState {
                        size: remote.size,
                        local_modified: new_local_mtime,
                        remote_modified: remote.modified,
                    });
                } else {
                    if backend.sync() {
                        info!("Bidirectional: deleting remote file '{}' since it was deleted locally", rel_path);
                        let _ = backend.delete(&rel_path).await;
                    }
                }
            }
            // Case 7: Only existed in state (Deleted on both sides)
            (None, None, Some(_)) => {}
            // Case 8: Doesn't exist anywhere
            (None, None, None) => {}
        }
    }

    // 4. Save catalog state
    sync_state.files = next_files_state;
    sync_state.save(state_file_path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_sync_lib::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_bidirectional_sync_flows() {
        let local_dir = tempdir().unwrap();
        let remote_dir = tempdir().unwrap();

        let local_path = local_dir.path();
        let remote_sim = LocalSimulation::new(remote_dir.path().to_path_buf(), "TestSim".to_string());
        let state_file = local_path.join(".sync_state.json");
        let gitignore = ignore::gitignore::Gitignore::empty();

        // 1. Initial State (Both empty)
        sync_bidirectional(local_path, &remote_sim, &state_file, &gitignore).await.unwrap();

        // 2. Add local file -> should upload
        let file1_path = local_path.join("file1.txt");
        tokio::fs::write(&file1_path, "local data").await.unwrap();
        sync_bidirectional(local_path, &remote_sim, &state_file, &gitignore).await.unwrap();
        assert!(remote_sim.resolve("file1.txt").exists());

        // 3. Add remote file -> should download
        let remote_file2 = remote_sim.resolve("file2.txt");
        tokio::fs::write(&remote_file2, "remote data").await.unwrap();
        sync_bidirectional(local_path, &remote_sim, &state_file, &gitignore).await.unwrap();
        assert!(local_path.join("file2.txt").exists());

        // 4. Modify local file -> should upload
        tokio::fs::write(&file1_path, "local modified data").await.unwrap();
        sync_bidirectional(local_path, &remote_sim, &state_file, &gitignore).await.unwrap();
        let remote1_data = tokio::fs::read_to_string(remote_sim.resolve("file1.txt")).await.unwrap();
        assert_eq!(remote1_data, "local modified data");

        // 5. Delete local file -> should delete remote
        tokio::fs::remove_file(&file1_path).await.unwrap();
        sync_bidirectional(local_path, &remote_sim, &state_file, &gitignore).await.unwrap();
        assert!(!remote_sim.resolve("file1.txt").exists());

        // 6. Delete remote file -> should delete local
        tokio::fs::remove_file(remote_sim.resolve("file2.txt")).await.unwrap();
        sync_bidirectional(local_path, &remote_sim, &state_file, &gitignore).await.unwrap();
        assert!(!local_path.join("file2.txt").exists());
    }
}
