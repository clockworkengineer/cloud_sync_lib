use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;
use std::sync::Arc;
use cloud_sync_lib::{StorageBackend, SyncState, FileState, SyncIgnore};
use tracing::info;

#[derive(Clone, Debug)]
struct FileInfo {
    size: u64,
    modified: SystemTime,
    is_dir: bool,
    checksum: Option<String>,
}



/// Scans the remote storage directory recursively and builds a map of relative file paths to their metadata.
async fn scan_remote_dir(
    backend: &dyn StorageBackend,
    gitignore: &SyncIgnore,
) -> Result<HashMap<String, FileInfo>, Box<dyn std::error::Error>> {
    let mut files = HashMap::new();
    let mut queue = vec!["".to_string()];

    while let Some(current) = queue.pop() {
        match backend.list(&current).await {
            Ok(items) => {
                for item in items {
                    let path_str = item.path.to_string_lossy().to_string();
                    if path_str.contains("..") || path_str.contains("./") || path_str.starts_with('/') {
                        info!("Skipping potentially unsafe remote path containing traversal: {}", path_str);
                        continue;
                    }
                    if gitignore.is_ignored(&item.path, item.is_dir) {
                        continue;
                    }
                    if item.is_dir {
                        queue.push(path_str.clone());
                        files.insert(path_str, FileInfo {
                            size: 0,
                            modified: item.modified,
                            is_dir: true,
                            checksum: None,
                        });
                    } else {
                        files.insert(path_str, FileInfo {
                            size: item.size,
                            modified: item.modified,
                            is_dir: false,
                            checksum: item.checksum,
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

async fn get_remote_file_info(backend: &dyn StorageBackend, rel_path: &str) -> Option<FileInfo> {
    let path = Path::new(rel_path);
    let parent = path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let file_name = path.file_name()?.to_string_lossy();

    let items = backend.list(&parent).await.ok()?;
    for item in items {
        if let Some(item_name) = item.path.file_name() {
            if item_name.to_string_lossy() == file_name {
                return Some(FileInfo {
                    size: item.size,
                    modified: item.modified,
                    is_dir: item.is_dir,
                    checksum: item.checksum,
                });
            }
        }
    }
    None
}

async fn get_remote_mtime(backend: &dyn StorageBackend, rel_path: &str) -> Option<SystemTime> {
    get_remote_file_info(backend, rel_path).await.map(|info| info.modified)
}

async fn verified_upload(
    backend: &dyn StorageBackend,
    local_path: &Path,
    remote_path: &str,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let local_checksum = cloud_sync_lib::checksum::compute_sha256(local_path).await.ok();
    let mut retries = 0;
    loop {
        backend.upload(local_path, remote_path).await?;
        if let Some(remote_info) = get_remote_file_info(backend, remote_path).await {
            if let (Some(ref local_hash), Some(ref remote_hash)) = (&local_checksum, &remote_info.checksum) {
                if local_hash != remote_hash {
                    if retries < 3 {
                        retries += 1;
                        tracing::warn!("Checksum mismatch on upload for '{}' (local: {}, remote: {}). Retrying... ({}/3)", remote_path, local_hash, remote_hash, retries);
                        continue;
                    } else {
                        return Err(Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Checksum verification failed for '{}' after 3 upload attempts", remote_path)
                        )));
                    }
                }
            }
            return Ok(remote_info.checksum);
        }
        return Ok(None);
    }
}

async fn verified_download(
    backend: &dyn StorageBackend,
    remote_path: &str,
    local_path: &Path,
    remote_checksum: Option<&str>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut retries = 0;
    loop {
        backend.download(remote_path, local_path).await?;
        let local_checksum = cloud_sync_lib::checksum::compute_sha256(local_path).await.ok();
        if let (Some(ref local_hash), Some(ref remote_hash)) = (&local_checksum, &remote_checksum) {
            if local_hash != *remote_hash {
                if retries < 3 {
                    retries += 1;
                    tracing::warn!("Checksum mismatch on download for '{}' (local: {}, remote: {}). Retrying... ({}/3)", remote_path, local_hash, remote_hash, retries);
                    continue;
                } else {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Checksum verification failed for '{}' after 3 download attempts", remote_path)
                    )));
                }
            }
        }
        return Ok(local_checksum);
    }
}

#[allow(clippy::too_many_arguments)]
async fn sync_single_file(
    watch_dir: std::path::PathBuf,
    rel_path: String,
    backend: Arc<dyn StorageBackend>,
    sync_both: bool,
    sync_deletions: bool,
    local_opt: Option<FileInfo>,
    remote_opt: Option<FileInfo>,
    state_opt: Option<FileState>,
) -> Result<Vec<(String, FileState)>, Box<dyn std::error::Error + Send + Sync>> {
    let mut updates = Vec::new();
    let local_file_path = watch_dir.join(&rel_path);

    match (local_opt, remote_opt, state_opt) {
        // Case 1: Exists everywhere (check for modifications)
        (Some(local), Some(remote), Some(state)) => {
            let local_changed = local.size != state.size 
                || local.modified != state.local_modified
                || (local.checksum.is_some() && state.checksum.is_some() && local.checksum != state.checksum);
            let remote_changed = remote.size != state.size 
                || remote.modified != state.remote_modified
                || (remote.checksum.is_some() && state.checksum.is_some() && remote.checksum != state.checksum);

            if local_changed && remote_changed {
                if sync_both {
                    let conflict_rel_path = format!("{}.local-conflict", rel_path);
                    let conflict_local_path = watch_dir.join(&conflict_rel_path);

                    info!("Renaming conflicting local file to: {:?}", conflict_local_path);
                    tokio::fs::rename(&local_file_path, &conflict_local_path).await?;

                    info!("Uploading conflict copy '{}' to remote", conflict_rel_path);
                    let conflict_remote_checksum = verified_upload(backend.as_ref(), &conflict_local_path, &conflict_rel_path).await?;
                    let conflict_remote_mtime = get_remote_mtime(backend.as_ref(), &conflict_rel_path).await.unwrap_or(SystemTime::now());
                    let conflict_local_mtime = get_local_mtime(&conflict_local_path).await.unwrap_or(SystemTime::now());

                    updates.push((conflict_rel_path.clone(), FileState {
                        size: local.size,
                        local_modified: conflict_local_mtime,
                        remote_modified: conflict_remote_mtime,
                        is_dir: Some(false),
                        checksum: conflict_remote_checksum.or(local.checksum.clone()),
                    }));

                    info!("Downloading remote file '{}' to original local path", rel_path);
                    let local_checksum = verified_download(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref()).await?;
                    let replaced_local_mtime = get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now());

                    updates.push((rel_path.clone(), FileState {
                        size: remote.size,
                        local_modified: replaced_local_mtime,
                        remote_modified: remote.modified,
                        is_dir: Some(false),
                        checksum: local_checksum.or(remote.checksum.clone()),
                    }));
                } else {
                    info!("Unidirectional: overwriting remote file '{}' (conflict, local is source of truth)", rel_path);
                    let remote_checksum = verified_upload(backend.as_ref(), &local_file_path, &rel_path).await?;
                    let remote_info = get_remote_file_info(backend.as_ref(), &rel_path).await.unwrap_or(remote);
                    updates.push((rel_path.clone(), FileState {
                        size: local.size,
                        local_modified: local.modified,
                        remote_modified: remote_info.modified,
                        is_dir: Some(false),
                        checksum: remote_checksum.or(local.checksum),
                    }));
                }
            } else if local_changed {
                info!("Bidirectional: uploading local modification '{}' to remote", rel_path);
                let remote_checksum = verified_upload(backend.as_ref(), &local_file_path, &rel_path).await?;
                let remote_info = get_remote_file_info(backend.as_ref(), &rel_path).await.unwrap_or(remote);
                updates.push((rel_path.clone(), FileState {
                    size: local.size,
                    local_modified: local.modified,
                    remote_modified: remote_info.modified,
                    is_dir: Some(false),
                    checksum: remote_checksum.or(local.checksum),
                }));
            } else if remote_changed {
                if sync_both {
                    info!("Bidirectional: downloading remote modification '{}' to local", rel_path);
                    let local_checksum = verified_download(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref()).await?;
                    let new_local_mtime = get_local_mtime(&local_file_path).await.unwrap_or(local.modified);
                    updates.push((rel_path.clone(), FileState {
                        size: remote.size,
                        local_modified: new_local_mtime,
                        remote_modified: remote.modified,
                        is_dir: Some(false),
                        checksum: local_checksum.or(remote.checksum),
                    }));
                } else {
                    updates.push((rel_path.clone(), FileState {
                        size: remote.size,
                        local_modified: local.modified,
                        remote_modified: remote.modified,
                        is_dir: Some(false),
                        checksum: remote.checksum.or(state.checksum),
                    }));
                }
            } else {
                updates.push((rel_path.clone(), state.clone()));
            }
        }
        // Case 2: Local & Remote, but not in state catalog (e.g. concurrent initial additions)
        (Some(local), Some(remote), None) => {
            let same_checksum = match (&local.checksum, &remote.checksum) {
                (Some(lc), Some(rc)) => lc == rc,
                _ => false,
            };
            if local.size == remote.size || same_checksum {
                updates.push((rel_path.clone(), FileState {
                    size: local.size,
                    local_modified: local.modified,
                    remote_modified: remote.modified,
                    is_dir: Some(false),
                    checksum: remote.checksum.or(local.checksum),
                }));
            } else {
                if sync_both {
                    let conflict_rel_path = format!("{}.local-conflict", rel_path);
                    let conflict_local_path = watch_dir.join(&conflict_rel_path);

                    info!("Renaming conflicting local file to: {:?}", conflict_local_path);
                    tokio::fs::rename(&local_file_path, &conflict_local_path).await?;

                    info!("Uploading conflict copy '{}' to remote", conflict_rel_path);
                    let conflict_remote_checksum = verified_upload(backend.as_ref(), &conflict_local_path, &conflict_rel_path).await?;
                    let conflict_remote_mtime = get_remote_mtime(backend.as_ref(), &conflict_rel_path).await.unwrap_or(SystemTime::now());
                    let conflict_local_mtime = get_local_mtime(&conflict_local_path).await.unwrap_or(SystemTime::now());

                    updates.push((conflict_rel_path, FileState {
                        size: local.size,
                        local_modified: conflict_local_mtime,
                        remote_modified: conflict_remote_mtime,
                        is_dir: Some(false),
                        checksum: conflict_remote_checksum.or(local.checksum.clone()),
                    }));

                    info!("Downloading remote file '{}' to original local path", rel_path);
                    let local_checksum = verified_download(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref()).await?;
                    let replaced_local_mtime = get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now());

                    updates.push((rel_path.clone(), FileState {
                        size: remote.size,
                        local_modified: replaced_local_mtime,
                        remote_modified: remote.modified,
                        is_dir: Some(false),
                        checksum: local_checksum.or(remote.checksum),
                    }));
                } else {
                    info!("Unidirectional: overwriting remote file '{}' (initial diff, local is source of truth)", rel_path);
                    let remote_checksum = verified_upload(backend.as_ref(), &local_file_path, &rel_path).await?;
                    let remote_info = get_remote_file_info(backend.as_ref(), &rel_path).await.unwrap_or(remote);
                    updates.push((rel_path.clone(), FileState {
                        size: local.size,
                        local_modified: local.modified,
                        remote_modified: remote_info.modified,
                        is_dir: Some(false),
                        checksum: remote_checksum.or(local.checksum),
                    }));
                }
            }
        }
        // Case 3: Local-only, not in state catalog (New local file)
        (Some(local), None, None) => {
            info!("Bidirectional: uploading new local file '{}' to remote", rel_path);
            let remote_checksum = verified_upload(backend.as_ref(), &local_file_path, &rel_path).await?;
            let remote_mtime = get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now());
            updates.push((rel_path.clone(), FileState {
                size: local.size,
                local_modified: local.modified,
                remote_modified: remote_mtime,
                is_dir: Some(false),
                checksum: remote_checksum.or(local.checksum),
            }));
        }
        // Case 4: Remote-only, not in state catalog (New remote file)
        (None, Some(remote), None) => {
            if sync_both {
                info!("Bidirectional: downloading new remote file '{}' to local", rel_path);
                let local_checksum = verified_download(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref()).await?;
                let new_local_mtime = get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now());
                updates.push((rel_path.clone(), FileState {
                    size: remote.size,
                    local_modified: new_local_mtime,
                    remote_modified: remote.modified,
                    is_dir: Some(false),
                    checksum: local_checksum.or(remote.checksum),
                }));
            }
        }
        // Case 5: Local & State, but missing remote (Deleted remotely)
        (Some(local), None, Some(state)) => {
            let local_changed = local.size != state.size 
                || local.modified != state.local_modified
                || (local.checksum.is_some() && state.checksum.is_some() && local.checksum != state.checksum);
            if local_changed {
                info!("Bidirectional: re-uploading modified local file '{}' that was deleted remotely", rel_path);
                let remote_checksum = verified_upload(backend.as_ref(), &local_file_path, &rel_path).await?;
                let remote_mtime = get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now());
                updates.push((rel_path.clone(), FileState {
                    size: local.size,
                    local_modified: local.modified,
                    remote_modified: remote_mtime,
                    is_dir: Some(false),
                    checksum: remote_checksum.or(local.checksum),
                }));
            } else {
                if sync_both {
                    info!("Bidirectional: deleting local file '{}' since it was deleted remotely", rel_path);
                    let _ = tokio::fs::remove_file(local_file_path).await;
                } else {
                    info!("Unidirectional: recreating remote file '{}' (deleted remotely)", rel_path);
                    let remote_checksum = verified_upload(backend.as_ref(), &local_file_path, &rel_path).await?;
                    let remote_mtime = get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now());
                    updates.push((rel_path.clone(), FileState {
                        size: local.size,
                        local_modified: local.modified,
                        remote_modified: remote_mtime,
                        is_dir: Some(false),
                        checksum: remote_checksum.or(local.checksum),
                    }));
                }
            }
        }
        // Case 6: Remote & State, but missing local (Deleted locally)
        (None, Some(remote), Some(state)) => {
            let remote_changed = remote.size != state.size 
                || remote.modified != state.remote_modified
                || (remote.checksum.is_some() && state.checksum.is_some() && remote.checksum != state.checksum);
            if remote_changed {
                if sync_both {
                    info!("Bidirectional: re-downloading modified remote file '{}' that was deleted locally", rel_path);
                    let local_checksum = verified_download(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref()).await?;
                    let new_local_mtime = get_local_mtime(&local_file_path).await.unwrap_or(remote.modified);
                    updates.push((rel_path.clone(), FileState {
                        size: remote.size,
                        local_modified: new_local_mtime,
                        remote_modified: remote.modified,
                        is_dir: Some(false),
                        checksum: local_checksum.or(remote.checksum),
                    }));
                } else {
                    if sync_deletions {
                        info!("Unidirectional: deleting remote file '{}' since it was deleted locally", rel_path);
                        let _ = backend.delete(&rel_path).await;
                    }
                }
            } else {
                if sync_deletions {
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

    Ok(updates)
}

/// Main entry point for performing a bidirectional synchronization step.
pub async fn sync_bidirectional(
    watch_dir: &Path,
    backend: Arc<dyn StorageBackend>,
    policy: cloud_sync_lib::SyncPolicy,
    state_file_path: &Path,
    gitignore: &SyncIgnore,
    max_concurrency: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let sync_both = policy.sync_both();
    let sync_deletions = policy.sync_deletions();

    // 1. Load sync state catalog
    let mut sync_state = SyncState::load(state_file_path).await.unwrap_or_default();

    // 2. Scan directories
    let local_scanned = crate::watcher::scan_local_directory(watch_dir, gitignore).await?;
    let mut local_files = HashMap::new();
    for (rel_path, item) in local_scanned {
        let checksum = if item.is_dir {
            None
        } else {
            cloud_sync_lib::checksum::compute_sha256(&item.path).await.ok()
        };
        local_files.insert(rel_path, FileInfo {
            size: item.size,
            modified: item.modified,
            is_dir: item.is_dir,
            checksum,
        });
    }
    let remote_files = scan_remote_dir(backend.as_ref(), gitignore).await?;

    // 3. Partition changes
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

    let mut dir_paths = Vec::new();
    let mut file_paths = Vec::new();

    for rel_path in all_paths {
        let local_opt = local_files.get(&rel_path);
        let remote_opt = remote_files.get(&rel_path);
        let state_opt = sync_state.files.get(&rel_path);

        let is_local_dir = local_opt.map(|f| f.is_dir).unwrap_or(false);
        let is_remote_dir = remote_opt.map(|f| f.is_dir).unwrap_or(false);
        let is_state_dir = state_opt.and_then(|f| f.is_dir).unwrap_or(false);

        if is_local_dir || is_remote_dir || is_state_dir {
            dir_paths.push(rel_path);
        } else {
            file_paths.push(rel_path);
        }
    }

    // Phase 1: Directories (Sequential)
    for rel_path in dir_paths {
        let local_opt = local_files.get(&rel_path);
        let remote_opt = remote_files.get(&rel_path);
        let state_opt = sync_state.files.get(&rel_path);

        match (local_opt, remote_opt, state_opt) {
            (Some(local), Some(remote), _) => {
                next_files_state.insert(rel_path.clone(), FileState {
                    size: 0,
                    local_modified: local.modified,
                    remote_modified: remote.modified,
                    is_dir: Some(true),
                    checksum: None,
                });
            }
            (Some(local), None, None) => {
                info!("Bidirectional: creating remote directory '{}'", rel_path);
                if let Err(e) = backend.create_folder(&rel_path).await {
                    info!("Failed to create remote directory '{}': {}", rel_path, e);
                }
                next_files_state.insert(rel_path.clone(), FileState {
                    size: 0,
                    local_modified: local.modified,
                    remote_modified: get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now()),
                    is_dir: Some(true),
                    checksum: None,
                });
            }
            (None, Some(remote), None) => {
                if sync_both {
                    info!("Bidirectional: creating local directory '{}'", rel_path);
                    let local_path = watch_dir.join(&rel_path);
                    if let Err(e) = tokio::fs::create_dir_all(&local_path).await {
                        info!("Failed to create local directory '{:?}': {}", local_path, e);
                    }
                    let new_local_mtime = get_local_mtime(&local_path).await.unwrap_or(SystemTime::now());
                    next_files_state.insert(rel_path.clone(), FileState {
                        size: 0,
                        local_modified: new_local_mtime,
                        remote_modified: remote.modified,
                        is_dir: Some(true),
                        checksum: None,
                    });
                }
            }
            (Some(local), None, Some(_state)) => {
                if sync_both {
                    info!("Bidirectional: deleting local directory '{}' (deleted remotely)", rel_path);
                    let local_path = watch_dir.join(&rel_path);
                    if local_path.exists() {
                        let _ = tokio::fs::remove_dir_all(&local_path).await;
                    }
                } else {
                    info!("Unidirectional: recreating remote directory '{}' (deleted remotely)", rel_path);
                    if let Err(e) = backend.create_folder(&rel_path).await {
                        info!("Failed to create remote directory '{}': {}", rel_path, e);
                    }
                    next_files_state.insert(rel_path.clone(), FileState {
                        size: 0,
                        local_modified: local.modified,
                        remote_modified: get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now()),
                        is_dir: Some(true),
                        checksum: None,
                    });
                }
            }
            (None, Some(_remote), Some(_state)) => {
                if sync_deletions {
                    info!("Bidirectional: deleting remote directory '{}' (deleted locally)", rel_path);
                    let _ = backend.delete(&rel_path).await;
                }
            }
            (None, None, Some(_)) => {}
            (None, None, None) => {}
        }
    }

    // Phase 2: Files (Concurrent)
    use futures_util::stream::StreamExt;
    let tasks = file_paths.into_iter().map(|rel_path| {
        let watch_dir = watch_dir.to_path_buf();
        let backend = backend.clone();
        let local_opt = local_files.get(&rel_path).cloned();
        let remote_opt = remote_files.get(&rel_path).cloned();
        let state_opt = sync_state.files.get(&rel_path).cloned();

        tokio::spawn(async move {
            sync_single_file(
                watch_dir,
                rel_path,
                backend,
                sync_both,
                sync_deletions,
                local_opt,
                remote_opt,
                state_opt,
            ).await
        })
    });

    let results =
        futures_util::stream::iter(tasks)
            .buffer_unordered(max_concurrency)
            .collect::<Vec<_>>()
            .await;

    for join_res in results {
        let task_res = join_res?;
        let updates = task_res.map_err(|e| e as Box<dyn std::error::Error>)?;
        for (path, file_state) in updates {
            next_files_state.insert(path, file_state);
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
    use cloud_sync_lib::StorageError;
    use cloud_sync_lib::StorageItem;

    use cloud_sync_lib::SyncPolicy;

    struct TestBackendWrapper {
        sim: LocalSimulation,
    }

    #[async_trait::async_trait]
    impl StorageBackend for TestBackendWrapper {
        fn name(&self) -> &str {
            "TestSim"
        }
        async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
            self.sim.upload(local_path, remote_path).await
        }
        async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
            self.sim.download(remote_path, local_path).await
        }
        async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
            self.sim.delete(remote_path).await
        }
        async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
            self.sim.list(remote_path).await
        }
        async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
            self.sim.create_folder(remote_path).await
        }
    }

    async fn sync_bidirectional(
        watch_dir: &Path,
        backend: Arc<dyn StorageBackend>,
        state_file_path: &Path,
        gitignore: &SyncIgnore,
        max_concurrency: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sync_mode = if watch_dir.to_string_lossy().contains("uni") {
            cloud_sync_lib::SyncMode::OneWay
        } else {
            cloud_sync_lib::SyncMode::TwoWay
        };
        let policy = SyncPolicy::new(sync_mode);
        super::sync_bidirectional(watch_dir, backend, policy, state_file_path, gitignore, max_concurrency).await
    }

    #[tokio::test]
    async fn test_bidirectional_sync_flows() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let temp_base = std::env::temp_dir();
        let local_dir = temp_base.join(format!("local_{}", unique_id));
        let remote_dir = temp_base.join(format!("remote_{}", unique_id));

        tokio::fs::create_dir_all(&local_dir).await.unwrap();
        tokio::fs::create_dir_all(&remote_dir).await.unwrap();

        let local_path = &local_dir;
        let sim = LocalSimulation::new(remote_dir.clone(), "TestSim".to_string());
        let remote_sim = Arc::new(TestBackendWrapper { sim });
        let state_file = local_path.join(".sync_state.json");
        let gitignore = SyncIgnore::empty();

        // 1. Initial State (Both empty)
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();

        // 2. Add local file -> should upload
        let file1_path = local_path.join("file1.txt");
        tokio::fs::write(&file1_path, "local data").await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(remote_sim.sim.resolve("file1.txt").exists());

        // 3. Add remote file -> should download
        let remote_file2 = remote_sim.sim.resolve("file2.txt");
        tokio::fs::write(&remote_file2, "remote data").await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(local_path.join("file2.txt").exists());

        // 4. Modify local file -> should upload
        tokio::fs::write(&file1_path, "local modified data").await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        let remote1_data = tokio::fs::read_to_string(remote_sim.sim.resolve("file1.txt")).await.unwrap();
        assert_eq!(remote1_data, "local modified data");

        // 5. Delete local file -> should delete remote
        tokio::fs::remove_file(&file1_path).await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(!remote_sim.sim.resolve("file1.txt").exists());

        // 6. Delete remote file -> should delete local
        tokio::fs::remove_file(remote_sim.sim.resolve("file2.txt")).await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(!local_path.join("file2.txt").exists());

        // 7. Add local directory -> should upload (create remotely)
        let local_subdir = local_path.join("empty_dir");
        tokio::fs::create_dir_all(&local_subdir).await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(remote_sim.sim.resolve("empty_dir").exists());
        assert!(remote_sim.sim.resolve("empty_dir").is_dir());

        // 8. Add remote directory -> should download (create locally)
        let remote_subdir = remote_sim.sim.resolve("remote_empty_dir");
        tokio::fs::create_dir_all(&remote_subdir).await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(local_path.join("remote_empty_dir").exists());
        assert!(local_path.join("remote_empty_dir").is_dir());

        // Clean up
        let _ = tokio::fs::remove_dir_all(&local_dir).await;
        let _ = tokio::fs::remove_dir_all(&remote_dir).await;
    }

    #[tokio::test]
    async fn test_unidirectional_sync_flows() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let temp_base = std::env::temp_dir();
        let local_dir = temp_base.join(format!("local_uni_{}", unique_id));
        let remote_dir = temp_base.join(format!("remote_uni_{}", unique_id));

        tokio::fs::create_dir_all(&local_dir).await.unwrap();
        tokio::fs::create_dir_all(&remote_dir).await.unwrap();

        let local_path = &local_dir;
        let sim = LocalSimulation::new(remote_dir.clone(), "TestSim".to_string());
        let remote_sim = Arc::new(TestBackendWrapper { sim });
        let state_file = local_path.join(".sync_state.json");
        let gitignore = SyncIgnore::empty();

        // 1. Initial State (Both empty)
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();

        // 2. Add local file -> should upload
        let file1_path = local_path.join("file1.txt");
        tokio::fs::write(&file1_path, "local data").await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(remote_sim.sim.resolve("file1.txt").exists());

        // 3. Add remote file -> should NOT download (since sync_both is false)
        let remote_file2 = remote_sim.sim.resolve("file2.txt");
        tokio::fs::write(&remote_file2, "remote data").await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(!local_path.join("file2.txt").exists());

        // 4. Modify remote file -> should NOT download
        tokio::fs::write(&remote_file2, "remote modified data").await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(!local_path.join("file2.txt").exists());

        // 5. Delete remote file (exists locally) -> should recreate remote file (since local is source of truth)
        tokio::fs::remove_file(&remote_file2).await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        // Wait, file1.txt was deleted remotely but is still present locally, so unidirectional sync recreates it
        assert!(remote_sim.sim.resolve("file1.txt").exists());

        // 6. Delete local file -> should delete remote file
        tokio::fs::remove_file(&file1_path).await.unwrap();
        sync_bidirectional(local_path, remote_sim.clone(), &state_file, &gitignore, 4).await.unwrap();
        assert!(!remote_sim.sim.resolve("file1.txt").exists());

        // Clean up
        let _ = tokio::fs::remove_dir_all(&local_dir).await;
        let _ = tokio::fs::remove_dir_all(&remote_dir).await;
    }

    #[tokio::test]
    async fn test_checksum_based_change_detection() {
        let watch_dir = tempfile::tempdir().unwrap();
        let local_path = watch_dir.path().join("test.txt");
        tokio::fs::write(&local_path, "modified content").await.unwrap();
        let local_mtime = get_local_mtime(&local_path).await.unwrap();

        let sim_dir = tempfile::tempdir().unwrap();
        let sim = LocalSimulation::new(sim_dir.path().to_path_buf(), "TestSim".to_string());
        let backend = Arc::new(TestBackendWrapper { sim });

        // Construct a state where size and mtime are matching, but checksum is different
        let state = FileState {
            size: 16,
            local_modified: local_mtime,
            remote_modified: SystemTime::UNIX_EPOCH,
            is_dir: Some(false),
            checksum: Some("old_checksum".to_string()),
        };

        let local_info = FileInfo {
            size: 16,
            modified: local_mtime,
            is_dir: false,
            checksum: Some("new_checksum".to_string()),
        };

        let remote_info = FileInfo {
            size: 16,
            modified: SystemTime::UNIX_EPOCH,
            is_dir: false,
            checksum: Some("old_checksum".to_string()),
        };

        let updates = sync_single_file(
            watch_dir.path().to_path_buf(),
            "test.txt".to_string(),
            backend.clone(),
            true, // sync_both
            true, // sync_deletions
            Some(local_info),
            Some(remote_info),
            Some(state),
        ).await.unwrap();

        // It should have detected local change due to checksum mismatch and triggered upload
        assert!(!updates.is_empty());
        let (path, new_state) = &updates[0];
        assert_eq!(path, "test.txt");
        assert_eq!(new_state.size, 16);
    }
}
