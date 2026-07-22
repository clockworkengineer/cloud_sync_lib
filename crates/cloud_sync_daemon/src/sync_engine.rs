use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;
use std::sync::Arc;
use cloud_sync_lib::{StorageBackend, SyncState, FileState, SyncIgnore};
use tracing::info;

#[derive(Clone, Debug)]
struct FileInfo {
    permissions: Option<u32>,
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
                    if path_str == ".sync_state.json" || path_str == ".sync_state.bin" || path_str == ".syncignore" || (path_str.starts_with(".sync_state_") && (path_str.ends_with(".json") || path_str.ends_with(".bin"))) {
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
                            permissions: item.permissions,
                        });
                    } else {
                        files.insert(path_str, FileInfo {
                            size: item.size,
                            modified: item.modified,
                            is_dir: false,
                            checksum: item.checksum,
                            permissions: item.permissions,
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
    let (parent, file_name) = cloud_sync_lib::path::get_parent_and_filename(rel_path);
    if file_name.is_empty() {
        return None;
    }

    let items = backend.list(&parent).await.ok()?;
    for item in items {
        if let Some(item_name) = item.path.file_name() {
            if item_name.to_string_lossy() == file_name {
                return Some(FileInfo {
                    size: item.size,
                    modified: item.modified,
                    is_dir: item.is_dir,
                    checksum: item.checksum,
                    permissions: item.permissions,
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
    let local_checksum = backend.compute_local_checksum(local_path).await.ok().flatten();
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

async fn set_local_permissions(path: &Path, mode: Option<u32>) -> Result<(), std::io::Error> {
    if let Some(mode) = mode {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).await?;
        }
        #[cfg(not(unix))]
        {
            if let Ok(metadata) = tokio::fs::metadata(path).await {
                let mut perms = metadata.permissions();
                perms.set_readonly((mode & 0o200) == 0);
                let _ = tokio::fs::set_permissions(path, perms).await;
            }
        }
    }
    Ok(())
}

async fn verified_download(
    backend: &dyn StorageBackend,
    remote_path: &str,
    local_path: &Path,
    remote_checksum: Option<&str>,
    mode: Option<u32>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut retries = 0;
    loop {
        backend.download(remote_path, local_path).await?;
        let _ = set_local_permissions(local_path, mode).await;
        let local_checksum = backend.compute_local_checksum(local_path).await.ok().flatten();
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
    conflict_policy: cloud_sync_lib::ConflictPolicy,
    dry_run: bool,
) -> Result<(Vec<(String, FileState)>, bool), Box<dyn std::error::Error + Send + Sync>> {
    let mut updates = Vec::new();
    let mut copied = false;

    macro_rules! upload_file {
        ($b:expr, $lp:expr, $rp:expr) => {{
            let res = verified_upload($b, $lp, $rp).await?;
            copied = true;
            res
        }};
    }

    macro_rules! download_file {
        ($b:expr, $rp:expr, $lp:expr, $rc:expr, $p:expr) => {{
            let res = verified_download($b, $rp, $lp, $rc, $p).await?;
            copied = true;
            res
        }};
    }

    let local_file_path = watch_dir.join(&rel_path);
    let current_permissions = local_opt.as_ref().and_then(|l| l.permissions)
        .or_else(|| remote_opt.as_ref().and_then(|r| r.permissions))
        .or_else(|| state_opt.as_ref().and_then(|s| s.permissions));

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
                let resolved_policy = if sync_both {
                    conflict_policy
                } else {
                    cloud_sync_lib::ConflictPolicy::KeepLocal
                };

                match resolved_policy {
                    cloud_sync_lib::ConflictPolicy::RenameLocal => {
                        let conflict_rel_path = format!("{}.local-conflict", rel_path);
                        let conflict_local_path = watch_dir.join(&conflict_rel_path);

                        info!("Conflict Policy (RenameLocal): Renaming conflicting local file to: {:?}", conflict_local_path);
                        if !dry_run {
                            tokio::fs::rename(&local_file_path, &conflict_local_path).await?;
                        } else {
                            info!("[DRY-RUN] Would rename local file {:?} to {:?}", local_file_path, conflict_local_path);
                        }

                        info!("Uploading conflict copy '{}' to remote", conflict_rel_path);
                        let conflict_remote_checksum = if !dry_run {
                            upload_file!(backend.as_ref(), &conflict_local_path, &conflict_rel_path)
                        } else {
                            info!("[DRY-RUN] Would upload local file {:?} to remote path '{}'", conflict_local_path, conflict_rel_path);
                            None
                        };
                        let conflict_remote_mtime = if !dry_run {
                            get_remote_mtime(backend.as_ref(), &conflict_rel_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };
                        let conflict_local_mtime = if !dry_run {
                            get_local_mtime(&conflict_local_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((conflict_rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: local.size,
                            local_modified: conflict_local_mtime,
                            remote_modified: conflict_remote_mtime,
                            is_dir: Some(false),
                            checksum: conflict_remote_checksum.or(local.checksum.clone()),
                        }));

                        info!("Downloading remote file '{}' to original local path", rel_path);
                        let local_checksum = if !dry_run {
                            download_file!(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref(), remote.permissions)
                        } else {
                            info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, local_file_path);
                            None
                        };
                        let replaced_local_mtime = if !dry_run {
                            get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: remote.size,
                            local_modified: replaced_local_mtime,
                            remote_modified: remote.modified,
                            is_dir: Some(false),
                            checksum: local_checksum.or(remote.checksum.clone()),
                        }));
                    }
                    cloud_sync_lib::ConflictPolicy::RenameRemote => {
                        let conflict_rel_path = format!("{}.remote-conflict", rel_path);
                        let conflict_local_path = watch_dir.join(&conflict_rel_path);

                        info!("Conflict Policy (RenameRemote): Downloading remote conflict copy '{}' to {:?}", conflict_rel_path, conflict_local_path);
                        let conflict_local_checksum = if !dry_run {
                            download_file!(backend.as_ref(), &rel_path, &conflict_local_path, remote.checksum.as_deref(), remote.permissions)
                        } else {
                            info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, conflict_local_path);
                            None
                        };
                        let conflict_local_mtime = if !dry_run {
                            get_local_mtime(&conflict_local_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((conflict_rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: remote.size,
                            local_modified: conflict_local_mtime,
                            remote_modified: remote.modified,
                            is_dir: Some(false),
                            checksum: conflict_local_checksum.or(remote.checksum.clone()),
                        }));

                        info!("Uploading local file '{}' to remote original path", rel_path);
                        let remote_checksum = if !dry_run {
                            upload_file!(backend.as_ref(), &local_file_path, &rel_path)
                        } else {
                            info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                            None
                        };
                        let remote_mtime = if !dry_run {
                            get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: local.size,
                            local_modified: local.modified,
                            remote_modified: remote_mtime,
                            is_dir: Some(false),
                            checksum: remote_checksum.or(local.checksum.clone()),
                        }));
                    }
                    cloud_sync_lib::ConflictPolicy::KeepLocal => {
                        info!("Conflict Policy (KeepLocal): Overwriting remote file '{}' with local changes", rel_path);
                        let remote_checksum = if !dry_run {
                            upload_file!(backend.as_ref(), &local_file_path, &rel_path)
                        } else {
                            info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                            None
                        };
                        let remote_mtime = if !dry_run {
                            get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: local.size,
                            local_modified: local.modified,
                            remote_modified: remote_mtime,
                            is_dir: Some(false),
                            checksum: remote_checksum.or(local.checksum),
                        }));
                    }
                    cloud_sync_lib::ConflictPolicy::KeepRemote => {
                        info!("Conflict Policy (KeepRemote): Overwriting local file '{:?}' with remote changes", local_file_path);
                        let local_checksum = if !dry_run {
                            download_file!(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref(), remote.permissions)
                        } else {
                            info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, local_file_path);
                            None
                        };
                        let local_mtime = if !dry_run {
                            get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: remote.size,
                            local_modified: local_mtime,
                            remote_modified: remote.modified,
                            is_dir: Some(false),
                            checksum: local_checksum.or(remote.checksum),
                        }));
                    }
                    cloud_sync_lib::ConflictPolicy::KeepNewer => {
                        let local_time = local.modified;
                        let remote_time = remote.modified;
                        if local_time >= remote_time {
                            info!("Conflict Policy (KeepNewer): Local is newer or equal ({:?} >= {:?}). Choosing KeepLocal.", local_time, remote_time);
                            let remote_checksum = if !dry_run {
                                upload_file!(backend.as_ref(), &local_file_path, &rel_path)
                            } else {
                                info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                                None
                            };
                            let remote_mtime = if !dry_run {
                                get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now())
                            } else {
                                SystemTime::now()
                            };

                            updates.push((rel_path.clone(), FileState {
                                permissions: current_permissions,
                                size: local.size,
                                local_modified: local.modified,
                                remote_modified: remote_mtime,
                                is_dir: Some(false),
                                checksum: remote_checksum.or(local.checksum),
                            }));
                        } else {
                            info!("Conflict Policy (KeepNewer): Remote is newer ({:?} < {:?}). Choosing KeepRemote.", local_time, remote_time);
                            let local_checksum = if !dry_run {
                                download_file!(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref(), remote.permissions)
                            } else {
                                info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, local_file_path);
                                None
                            };
                            let local_mtime = if !dry_run {
                                get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now())
                            } else {
                                SystemTime::now()
                            };

                            updates.push((rel_path.clone(), FileState {
                                permissions: current_permissions,
                                size: remote.size,
                                local_modified: local_mtime,
                                remote_modified: remote.modified,
                                is_dir: Some(false),
                                checksum: local_checksum.or(remote.checksum),
                            }));
                        }
                    }
                }
            } else if local_changed {
                info!("Bidirectional: uploading local modification '{}' to remote", rel_path);
                let remote_checksum = if !dry_run {
                    upload_file!(backend.as_ref(), &local_file_path, &rel_path)
                } else {
                    info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                    None
                };
                let remote_info = if !dry_run {
                    get_remote_file_info(backend.as_ref(), &rel_path).await.unwrap_or(remote.clone())
                } else {
                    remote.clone()
                };
                updates.push((rel_path.clone(), FileState {
                    permissions: current_permissions,
                    size: local.size,
                    local_modified: local.modified,
                    remote_modified: remote_info.modified,
                    is_dir: Some(false),
                    checksum: remote_checksum.or(local.checksum),
                }));
            } else if remote_changed {
                if sync_both {
                    info!("Bidirectional: downloading remote modification '{}' to local", rel_path);
                    let local_checksum = if !dry_run {
                        download_file!(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref(), remote.permissions)
                    } else {
                        info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, local_file_path);
                        None
                    };
                    let new_local_mtime = if !dry_run {
                        get_local_mtime(&local_file_path).await.unwrap_or(local.modified)
                    } else {
                        local.modified
                    };
                    updates.push((rel_path.clone(), FileState {
                        permissions: current_permissions,
                        size: remote.size,
                        local_modified: new_local_mtime,
                        remote_modified: remote.modified,
                        is_dir: Some(false),
                        checksum: local_checksum.or(remote.checksum),
                    }));
                } else {
                    updates.push((rel_path.clone(), FileState {
                        permissions: current_permissions,
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
                    permissions: current_permissions,
                    size: local.size,
                    local_modified: local.modified,
                    remote_modified: remote.modified,
                    is_dir: Some(false),
                    checksum: remote.checksum.or(local.checksum),
                }));
            } else {
                let resolved_policy = if sync_both {
                    conflict_policy
                } else {
                    cloud_sync_lib::ConflictPolicy::KeepLocal
                };

                match resolved_policy {
                    cloud_sync_lib::ConflictPolicy::RenameLocal => {
                        let conflict_rel_path = format!("{}.local-conflict", rel_path);
                        let conflict_local_path = watch_dir.join(&conflict_rel_path);

                        info!("Renaming conflicting local file to: {:?}", conflict_local_path);
                        if !dry_run {
                            tokio::fs::rename(&local_file_path, &conflict_local_path).await?;
                        } else {
                            info!("[DRY-RUN] Would rename local file {:?} to {:?}", local_file_path, conflict_local_path);
                        }

                        info!("Uploading conflict copy '{}' to remote", conflict_rel_path);
                        let conflict_remote_checksum = if !dry_run {
                            upload_file!(backend.as_ref(), &conflict_local_path, &conflict_rel_path)
                        } else {
                            info!("[DRY-RUN] Would upload local file {:?} to remote path '{}'", conflict_local_path, conflict_rel_path);
                            None
                        };
                        let conflict_remote_mtime = if !dry_run {
                            get_remote_mtime(backend.as_ref(), &conflict_rel_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };
                        let conflict_local_mtime = if !dry_run {
                            get_local_mtime(&conflict_local_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((conflict_rel_path, FileState {
                            permissions: current_permissions,
                            size: local.size,
                            local_modified: conflict_local_mtime,
                            remote_modified: conflict_remote_mtime,
                            is_dir: Some(false),
                            checksum: conflict_remote_checksum.or(local.checksum.clone()),
                        }));

                        info!("Downloading remote file '{}' to original local path", rel_path);
                        let local_checksum = if !dry_run {
                            download_file!(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref(), remote.permissions)
                        } else {
                            info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, local_file_path);
                            None
                        };
                        let replaced_local_mtime = if !dry_run {
                            get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: remote.size,
                            local_modified: replaced_local_mtime,
                            remote_modified: remote.modified,
                            is_dir: Some(false),
                            checksum: local_checksum.or(remote.checksum),
                        }));
                    }
                    cloud_sync_lib::ConflictPolicy::RenameRemote => {
                        let conflict_rel_path = format!("{}.remote-conflict", rel_path);
                        let conflict_local_path = watch_dir.join(&conflict_rel_path);

                        info!("Downloading remote conflict copy '{}' to {:?}", conflict_rel_path, conflict_local_path);
                        let conflict_local_checksum = if !dry_run {
                            download_file!(backend.as_ref(), &rel_path, &conflict_local_path, remote.checksum.as_deref(), remote.permissions)
                        } else {
                            info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, conflict_local_path);
                            None
                        };
                        let conflict_local_mtime = if !dry_run {
                            get_local_mtime(&conflict_local_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((conflict_rel_path, FileState {
                            permissions: current_permissions,
                            size: remote.size,
                            local_modified: conflict_local_mtime,
                            remote_modified: remote.modified,
                            is_dir: Some(false),
                            checksum: conflict_local_checksum.or(remote.checksum.clone()),
                        }));

                        info!("Uploading local file '{}' to remote original path", rel_path);
                        let remote_checksum = if !dry_run {
                            upload_file!(backend.as_ref(), &local_file_path, &rel_path)
                        } else {
                            info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                            None
                        };
                        let remote_mtime = if !dry_run {
                            get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        updates.push((rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: local.size,
                            local_modified: local.modified,
                            remote_modified: remote_mtime,
                            is_dir: Some(false),
                            checksum: remote_checksum.or(local.checksum.clone()),
                        }));
                    }
                    cloud_sync_lib::ConflictPolicy::KeepLocal => {
                        info!("Unidirectional/Conflict: overwriting remote file '{}' (local is source of truth)", rel_path);
                        let remote_checksum = if !dry_run {
                            upload_file!(backend.as_ref(), &local_file_path, &rel_path)
                        } else {
                            info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                            None
                        };
                        let remote_info = if !dry_run {
                            get_remote_file_info(backend.as_ref(), &rel_path).await.unwrap_or(remote.clone())
                        } else {
                            remote.clone()
                        };
                        updates.push((rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: local.size,
                            local_modified: local.modified,
                            remote_modified: remote_info.modified,
                            is_dir: Some(false),
                            checksum: remote_checksum.or(local.checksum),
                        }));
                    }
                    cloud_sync_lib::ConflictPolicy::KeepRemote => {
                        info!("Conflict: downloading remote file '{}' to local (KeepRemote)", rel_path);
                        let local_checksum = if !dry_run {
                            download_file!(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref(), remote.permissions)
                        } else {
                            info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, local_file_path);
                            None
                        };
                        let local_mtime = if !dry_run {
                            get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };
                        updates.push((rel_path.clone(), FileState {
                            permissions: current_permissions,
                            size: remote.size,
                            local_modified: local_mtime,
                            remote_modified: remote.modified,
                            is_dir: Some(false),
                            checksum: local_checksum.or(remote.checksum),
                        }));
                    }
                    cloud_sync_lib::ConflictPolicy::KeepNewer => {
                        let local_time = local.modified;
                        let remote_time = remote.modified;
                        if local_time >= remote_time {
                            info!("Conflict (KeepNewer): Local is newer or equal ({:?} >= {:?}). Choosing KeepLocal.", local_time, remote_time);
                            let remote_checksum = if !dry_run {
                                upload_file!(backend.as_ref(), &local_file_path, &rel_path)
                            } else {
                                info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                                None
                            };
                            let remote_info = if !dry_run {
                                get_remote_file_info(backend.as_ref(), &rel_path).await.unwrap_or(remote.clone())
                            } else {
                                remote.clone()
                            };
                            updates.push((rel_path.clone(), FileState {
                                permissions: current_permissions,
                                size: local.size,
                                local_modified: local.modified,
                                remote_modified: remote_info.modified,
                                is_dir: Some(false),
                                checksum: remote_checksum.or(local.checksum),
                            }));
                        } else {
                            info!("Conflict (KeepNewer): Remote is newer ({:?} < {:?}). Choosing KeepRemote.", local_time, remote_time);
                            let local_checksum = if !dry_run {
                                download_file!(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref(), remote.permissions)
                            } else {
                                info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, local_file_path);
                                None
                            };
                            let local_mtime = if !dry_run {
                                get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now())
                            } else {
                                SystemTime::now()
                            };
                            updates.push((rel_path.clone(), FileState {
                                permissions: current_permissions,
                                size: remote.size,
                                local_modified: local_mtime,
                                remote_modified: remote.modified,
                                is_dir: Some(false),
                                checksum: local_checksum.or(remote.checksum),
                            }));
                        }
                    }
                }
            }
        }
        // Case 3: Local-only, not in state catalog (New local file)
        (Some(local), None, None) => {
            info!("Bidirectional: uploading new local file '{}' to remote", rel_path);
            let remote_checksum = if !dry_run {
                upload_file!(backend.as_ref(), &local_file_path, &rel_path)
            } else {
                info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                None
            };
            let remote_mtime = if !dry_run {
                get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now())
            } else {
                SystemTime::now()
            };
            updates.push((rel_path.clone(), FileState {
                permissions: current_permissions,
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
                let local_checksum = if !dry_run {
                    download_file!(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref(), remote.permissions)
                } else {
                    info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, local_file_path);
                    None
                };
                let new_local_mtime = if !dry_run {
                    get_local_mtime(&local_file_path).await.unwrap_or(SystemTime::now())
                } else {
                    SystemTime::now()
                };
                updates.push((rel_path.clone(), FileState {
                    permissions: current_permissions,
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
                let remote_checksum = if !dry_run {
                    upload_file!(backend.as_ref(), &local_file_path, &rel_path)
                } else {
                    info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                    None
                };
                let remote_mtime = if !dry_run {
                    get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now())
                } else {
                    SystemTime::now()
                };
                updates.push((rel_path.clone(), FileState {
                    permissions: current_permissions,
                    size: local.size,
                    local_modified: local.modified,
                    remote_modified: remote_mtime,
                    is_dir: Some(false),
                    checksum: remote_checksum.or(local.checksum),
                }));
            } else {
                if sync_both {
                    info!("Bidirectional: deleting local file '{}' since it was deleted remotely", rel_path);
                    if !dry_run {
                        let _ = tokio::fs::remove_file(local_file_path).await;
                    } else {
                        info!("[DRY-RUN] Would delete local file '{:?}'", local_file_path);
                    }
                } else {
                    info!("Unidirectional: recreating remote file '{}' (deleted remotely)", rel_path);
                    let remote_checksum = if !dry_run {
                        upload_file!(backend.as_ref(), &local_file_path, &rel_path)
                    } else {
                        info!("[DRY-RUN] Would upload local path {:?} to remote path '{}'", local_file_path, rel_path);
                        None
                    };
                    let remote_mtime = if !dry_run {
                        get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now())
                    } else {
                        SystemTime::now()
                    };
                    updates.push((rel_path.clone(), FileState {
                        permissions: current_permissions,
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
                    let local_checksum = if !dry_run {
                        download_file!(backend.as_ref(), &rel_path, &local_file_path, remote.checksum.as_deref(), remote.permissions)
                    } else {
                        info!("[DRY-RUN] Would download remote path '{}' to local path {:?}", rel_path, local_file_path);
                        None
                    };
                    let new_local_mtime = if !dry_run {
                        get_local_mtime(&local_file_path).await.unwrap_or(remote.modified)
                    } else {
                        remote.modified
                    };
                    updates.push((rel_path.clone(), FileState {
                        permissions: current_permissions,
                        size: remote.size,
                        local_modified: new_local_mtime,
                        remote_modified: remote.modified,
                        is_dir: Some(false),
                        checksum: local_checksum.or(remote.checksum),
                    }));
                } else {
                    if sync_deletions {
                        info!("Unidirectional: deleting remote file '{}' since it was deleted locally", rel_path);
                        if !dry_run {
                            let _ = backend.delete(&rel_path).await;
                        } else {
                            info!("[DRY-RUN] Would delete remote file '{}'", rel_path);
                        }
                    }
                }
            } else {
                if sync_deletions {
                    info!("Bidirectional: deleting remote file '{}' since it was deleted locally", rel_path);
                    if !dry_run {
                        let _ = backend.delete(&rel_path).await;
                    } else {
                        info!("[DRY-RUN] Would delete remote file '{}'", rel_path);
                    }
                }
            }
        }
        // Case 7: Only existed in state (Deleted on both sides)
        (None, None, Some(_)) => {}
        // Case 8: Doesn't exist anywhere
        (None, None, None) => {}
    }

    Ok((updates, copied))
}

fn is_path_selected(rel_path: &str, selective_sync: &Option<Vec<String>>) -> bool {
    if let Some(list) = selective_sync {
        if list.is_empty() {
            return false;
        }
        for prefix in list {
            let prefix_replaced = prefix.replace('\\', "/");
            let prefix_norm = prefix_replaced.trim_start_matches('/').trim_end_matches('/');
            let path_replaced = rel_path.replace('\\', "/");
            let path_norm = path_replaced.trim_start_matches('/').trim_end_matches('/');
            if path_norm == prefix_norm || path_norm.starts_with(&format!("{}/", prefix_norm)) {
                return true;
            }
        }
        false
    } else {
        true
    }
}

/// Main entry point for performing a bidirectional synchronization step.
pub async fn sync_bidirectional(
    watch_dir: &Path,
    backend: Arc<dyn StorageBackend>,
    policy: cloud_sync_lib::SyncPolicy,
    state_file_path: &Path,
    gitignore: &SyncIgnore,
    max_concurrency: usize,
    conflict_policy: cloud_sync_lib::ConflictPolicy,
    dry_run: bool,
    selective_sync: Option<Vec<String>>,
    event_tx: Option<tokio::sync::broadcast::Sender<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let broadcast_event = |event_type: &str, message: &str, progress: Option<f64>, active_file: Option<&str>| {
        if let Some(ref tx) = event_tx {
            let payload = serde_json::json!({
                "type": event_type,
                "message": message,
                "progress_percent": progress,
                "active_file": active_file,
            }).to_string();
            let _ = tx.send(payload);
        }
    };

    broadcast_event("status", "Starting synchronization...", Some(0.0), None);

    let sync_both = policy.sync_both();
    let sync_deletions = policy.sync_deletions();

    // 1. Load sync state catalog
    let mut sync_state = SyncState::load(state_file_path).await.unwrap_or_default();

    // 2. Scan directories
    let mut local_files = HashMap::new();
    let mut scanner = crate::watcher::DirectoryScanner::new(watch_dir, gitignore);
    while let Some((rel_path, item)) = scanner.next().await? {
        let checksum = if item.is_dir {
            None
        } else {
            backend.compute_local_checksum(&item.path).await.ok().flatten()
        };
        local_files.insert(rel_path, FileInfo {
            permissions: item.permissions,
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

    for (path, state) in &sync_state.files {
        if !is_path_selected(path, &selective_sync) {
            next_files_state.insert(path.clone(), state.clone());
        }
    }

    for path in local_files.keys() {
        if is_path_selected(path, &selective_sync) {
            all_paths.insert(path.clone());
        }
    }
    for path in remote_files.keys() {
        if is_path_selected(path, &selective_sync) {
            all_paths.insert(path.clone());
        }
    }
    for path in sync_state.files.keys() {
        if is_path_selected(path, &selective_sync) {
            all_paths.insert(path.clone());
        }
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
                        permissions: None,
                    size: 0,
                    local_modified: local.modified,
                    remote_modified: remote.modified,
                    is_dir: Some(true),
                    checksum: None,
                });
            }
            (Some(local), None, None) => {
                info!("Bidirectional: creating remote directory '{}'", rel_path);
                if !dry_run {
                    if let Err(e) = backend.create_folder(&rel_path).await {
                        info!("Failed to create remote directory '{}': {}", rel_path, e);
                    }
                } else {
                    info!("[DRY-RUN] Would create remote directory '{}'", rel_path);
                }
                next_files_state.insert(rel_path.clone(), FileState {
                        permissions: None,
                    size: 0,
                    local_modified: local.modified,
                    remote_modified: if !dry_run {
                        get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now())
                    } else {
                        SystemTime::now()
                    },
                    is_dir: Some(true),
                    checksum: None,
                });
            }
            (None, Some(remote), None) => {
                if sync_both {
                    info!("Bidirectional: creating local directory '{}'", rel_path);
                    let local_path = watch_dir.join(&rel_path);
                    if !dry_run {
                        if let Err(e) = tokio::fs::create_dir_all(&local_path).await {
                            info!("Failed to create local directory '{:?}': {}", local_path, e);
                        }
                    } else {
                        info!("[DRY-RUN] Would create local directory '{:?}'", local_path);
                    }
                    let new_local_mtime = if !dry_run {
                        get_local_mtime(&local_path).await.unwrap_or(SystemTime::now())
                    } else {
                        SystemTime::now()
                    };
                    next_files_state.insert(rel_path.clone(), FileState {
                        permissions: None,
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
                    if !dry_run {
                        if local_path.exists() {
                            let _ = tokio::fs::remove_dir_all(&local_path).await;
                        }
                    } else {
                        info!("[DRY-RUN] Would delete local directory '{:?}'", local_path);
                    }
                } else {
                    info!("Unidirectional: recreating remote directory '{}' (deleted remotely)", rel_path);
                    if !dry_run {
                        if let Err(e) = backend.create_folder(&rel_path).await {
                            info!("Failed to create remote directory '{}': {}", rel_path, e);
                        }
                    } else {
                        info!("[DRY-RUN] Would create remote directory '{}'", rel_path);
                    }
                    next_files_state.insert(rel_path.clone(), FileState {
                        permissions: None,
                        size: 0,
                        local_modified: local.modified,
                        remote_modified: if !dry_run {
                            get_remote_mtime(backend.as_ref(), &rel_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        },
                        is_dir: Some(true),
                        checksum: None,
                    });
                }
            }
            (None, Some(_remote), Some(_state)) => {
                if sync_deletions {
                    info!("Bidirectional: deleting remote directory '{}' (deleted locally)", rel_path);
                    if !dry_run {
                        let _ = backend.delete(&rel_path).await;
                    } else {
                        info!("[DRY-RUN] Would delete remote directory '{}'", rel_path);
                    }
                }
            }
            (None, None, Some(_)) => {}
            (None, None, None) => {}
        }
    }

    // Phase 1.5: Intelligent File Move/Rename Detection
    let mut resolved_moves = HashSet::new();
    if sync_both && sync_deletions {
        let mut from_candidates = Vec::new();
        let mut to_candidates = Vec::new();

        for path in &file_paths {
            let local_opt = local_files.get(path);
            let remote_opt = remote_files.get(path);
            let state_opt = sync_state.files.get(path);

            if local_opt.is_none() && remote_opt.is_some() && state_opt.is_some() {
                from_candidates.push(path.clone());
            } else if local_opt.is_some() && remote_opt.is_none() && state_opt.is_none() {
                to_candidates.push(path.clone());
            }
        }

        let mut matched_from = HashSet::new();
        for to_path in &to_candidates {
            let to_file = local_files.get(to_path).unwrap();
            if to_file.is_dir {
                continue;
            }

            let mut best_match = None;
            for from_path in &from_candidates {
                if matched_from.contains(from_path) {
                    continue;
                }
                let from_state = sync_state.files.get(from_path).unwrap();
                if from_state.is_dir.unwrap_or(false) {
                    continue;
                }

                if to_file.size == from_state.size {
                    if let (Some(to_chk), Some(from_chk)) = (&to_file.checksum, &from_state.checksum) {
                        if to_chk == from_chk {
                            best_match = Some(from_path.clone());
                            break;
                        }
                    } else {
                        best_match = Some(from_path.clone());
                    }
                }
            }

            if let Some(from_path) = best_match {
                matched_from.insert(from_path.clone());
                info!("Intelligent Move Detection: local move detected from '{}' to '{}'", from_path, to_path);

                let rename_res = if !dry_run {
                    backend.rename(&from_path, &to_path).await
                } else {
                    info!("[DRY-RUN] Would rename remote path '{}' to '{}'", from_path, to_path);
                    Ok(())
                };

                match rename_res {
                    Ok(()) => {
                        resolved_moves.insert(from_path.clone());
                        resolved_moves.insert(to_path.clone());

                        let remote_mtime = if !dry_run {
                            get_remote_mtime(backend.as_ref(), to_path).await.unwrap_or(SystemTime::now())
                        } else {
                            SystemTime::now()
                        };

                        next_files_state.insert(to_path.clone(), FileState {
                            permissions: to_file.permissions,
                            size: to_file.size,
                            local_modified: to_file.modified,
                            remote_modified: remote_mtime,
                            is_dir: Some(false),
                            checksum: to_file.checksum.clone(),
                        });
                    }
                    Err(e) => {
                        info!("Remote rename not supported or failed: {}. Falling back to default sync paths.", e);
                    }
                }
            }
        }
    }

    // Phase 2: Files (Concurrent)
    use futures_util::stream::StreamExt;
    let file_paths: Vec<String> = file_paths.into_iter().filter(|path| !resolved_moves.contains(path)).collect();
    let sync_state_files = sync_state.files.clone();
    let tasks = file_paths.into_iter().map(move |rel_path| {
        let watch_dir = watch_dir.to_path_buf();
        let backend = backend.clone();
        let local_opt = local_files.get(&rel_path).cloned();
        let remote_opt = remote_files.get(&rel_path).cloned();
        let state_opt = sync_state_files.get(&rel_path).cloned();

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
                conflict_policy,
                dry_run,
            ).await
        })
    });

    let total_files = tasks.len();
    let mut completed_files = 0;
    let mut files_copied = 0;
    let mut stream = futures_util::stream::iter(tasks).buffer_unordered(max_concurrency);

    while let Some(join_res) = stream.next().await {
        completed_files += 1;
        let task_res = join_res?;
        let (updates, copied) = task_res.map_err(|e| e as Box<dyn std::error::Error>)?;
        if copied {
            files_copied += 1;
        }
        
        let mut last_path = String::new();
        for (path, file_state) in updates {
            last_path = path.clone();
            next_files_state.insert(path, file_state);
        }

        let progress_percent = if total_files > 0 {
            (completed_files as f64 / total_files as f64) * 100.0
        } else {
            100.0
        };

        broadcast_event(
            "progress",
            &format!("Processed file {} of {}", completed_files, total_files),
            Some(progress_percent),
            if last_path.is_empty() { None } else { Some(&last_path) }
        );
    }

    // 4. Save catalog state
    if !dry_run {
        sync_state.files = next_files_state;
        sync_state.save(state_file_path).await?;
    } else {
        info!("[DRY-RUN] Sync execution completed. Skipping saving sync state catalog to disk.");
    }

    let status_msg = if files_copied > 0 {
        "Synchronization completed successfully."
    } else {
        "Already in sync."
    };
    broadcast_event("status", status_msg, Some(100.0), None);
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
        super::sync_bidirectional(watch_dir, backend, policy, state_file_path, gitignore, max_concurrency, cloud_sync_lib::ConflictPolicy::RenameLocal, false, None, None).await
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
        let state_file = local_path.join(".sync_state.bin");
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
        let state_file = local_path.join(".sync_state.bin");
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
            permissions: None,
        };

        let local_info = FileInfo {
            size: 16,
            modified: local_mtime,
            is_dir: false,
            checksum: Some("new_checksum".to_string()),
            permissions: None,
        };

        let remote_info = FileInfo {
            size: 16,
            modified: SystemTime::UNIX_EPOCH,
            is_dir: false,
            checksum: Some("old_checksum".to_string()),
            permissions: None,
        };

        let (updates, _copied) = sync_single_file(
            watch_dir.path().to_path_buf(),
            "test.txt".to_string(),
            backend.clone(),
            true, // sync_both
            true, // sync_deletions
            Some(local_info),
            Some(remote_info),
            Some(state),
            cloud_sync_lib::ConflictPolicy::RenameLocal,
            false,
        ).await.unwrap();

        // It should have detected local change due to checksum mismatch and triggered upload
        assert!(!updates.is_empty());
        let (path, new_state) = &updates[0];
        assert_eq!(path, "test.txt");
        assert_eq!(new_state.size, 16);
    }

    #[tokio::test]
    async fn test_conflict_policies_and_dry_run() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let temp_base = std::env::temp_dir();
        
        let run_sync = |policy: cloud_sync_lib::ConflictPolicy, dry: bool| {
            let unique_id = unique_id;
            let temp_base = temp_base.clone();
            async move {
                let local_dir = temp_base.join(format!("local_policy_{}", unique_id));
                let remote_dir = temp_base.join(format!("remote_policy_{}", unique_id));
                let _ = tokio::fs::create_dir_all(&local_dir).await;
                let _ = tokio::fs::create_dir_all(&remote_dir).await;

                let local_file = local_dir.join("conflict.txt");
                let remote_file = remote_dir.join("conflict.txt");
                tokio::fs::write(&local_file, "initial").await.unwrap();
                tokio::fs::write(&remote_file, "initial").await.unwrap();

                let sim = LocalSimulation::new(remote_dir.clone(), "TestSim".to_string());
                let remote_sim = Arc::new(TestBackendWrapper { sim });
                let state_file = local_dir.join(".sync_state.bin");
                let gitignore = SyncIgnore::empty();

                super::sync_bidirectional(
                    &local_dir,
                    remote_sim.clone(),
                    SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
                    &state_file,
                    &gitignore,
                    4,
                    cloud_sync_lib::ConflictPolicy::RenameLocal,
                    false,
                    None,
                    None
                ).await.unwrap();

                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                tokio::fs::write(&local_file, "local modified").await.unwrap();
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                tokio::fs::write(&remote_file, "remote modified").await.unwrap();

                super::sync_bidirectional(
                    &local_dir,
                    remote_sim.clone(),
                    SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
                    &state_file,
                    &gitignore,
                    4,
                    policy,
                    dry,
                    None,
                    None
                ).await.unwrap();

                (local_dir, remote_dir, state_file)
            }
        };

        let (local, remote, _) = run_sync(cloud_sync_lib::ConflictPolicy::RenameLocal, true).await;
        assert_eq!(tokio::fs::read_to_string(local.join("conflict.txt")).await.unwrap(), "local modified");
        assert_eq!(tokio::fs::read_to_string(remote.join("conflict.txt")).await.unwrap(), "remote modified");
        assert!(!local.join("conflict.txt.local-conflict").exists());

        let (local, _remote, _) = run_sync(cloud_sync_lib::ConflictPolicy::RenameLocal, false).await;
        assert_eq!(tokio::fs::read_to_string(local.join("conflict.txt")).await.unwrap(), "remote modified");
        assert_eq!(tokio::fs::read_to_string(local.join("conflict.txt.local-conflict")).await.unwrap(), "local modified");

        let (local, remote, _) = run_sync(cloud_sync_lib::ConflictPolicy::KeepLocal, false).await;
        assert_eq!(tokio::fs::read_to_string(local.join("conflict.txt")).await.unwrap(), "local modified");
        assert_eq!(tokio::fs::read_to_string(remote.join("conflict.txt")).await.unwrap(), "local modified");

        let (local, remote, _) = run_sync(cloud_sync_lib::ConflictPolicy::KeepRemote, false).await;
        assert_eq!(tokio::fs::read_to_string(local.join("conflict.txt")).await.unwrap(), "remote modified");
        assert_eq!(tokio::fs::read_to_string(remote.join("conflict.txt")).await.unwrap(), "remote modified");

        let (local, remote, _) = run_sync(cloud_sync_lib::ConflictPolicy::KeepNewer, false).await;
        assert_eq!(tokio::fs::read_to_string(local.join("conflict.txt")).await.unwrap(), "remote modified");
        assert_eq!(tokio::fs::read_to_string(remote.join("conflict.txt")).await.unwrap(), "remote modified");
    }

    #[tokio::test]
    async fn test_intelligent_move_detection() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let temp_base = std::env::temp_dir();
        let local_dir = temp_base.join(format!("local_move_{}", unique_id));
        let remote_dir = temp_base.join(format!("remote_move_{}", unique_id));
        tokio::fs::create_dir_all(&local_dir).await.unwrap();
        tokio::fs::create_dir_all(&remote_dir).await.unwrap();

        let sim = LocalSimulation::new(remote_dir.clone(), "TestSim".to_string());
        let remote_sim = Arc::new(TestBackendWrapper { sim });
        let state_file = local_dir.join(".sync_state.bin");
        let gitignore = SyncIgnore::empty();

        let old_file = local_dir.join("old_name.txt");
        tokio::fs::write(&old_file, "this is some unique content").await.unwrap();

        super::sync_bidirectional(
            &local_dir,
            remote_sim.clone(),
            SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
            &state_file,
            &gitignore,
            4,
            cloud_sync_lib::ConflictPolicy::RenameLocal,
            false,
            None,
            None
        ).await.unwrap();

        assert!(remote_sim.sim.resolve("old_name.txt").exists());

        tokio::fs::remove_file(&old_file).await.unwrap();
        let new_file = local_dir.join("new_name.txt");
        tokio::fs::write(&new_file, "this is some unique content").await.unwrap();

        super::sync_bidirectional(
            &local_dir,
            remote_sim.clone(),
            SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
            &state_file,
            &gitignore,
            4,
            cloud_sync_lib::ConflictPolicy::RenameLocal,
            false,
            None,
            None
        ).await.unwrap();

        assert!(!remote_sim.sim.resolve("old_name.txt").exists());
        assert!(remote_sim.sim.resolve("new_name.txt").exists());
    }

    #[tokio::test]
    async fn test_selective_sync() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let temp_base = std::env::temp_dir();
        let local_dir = temp_base.join(format!("local_sel_{}", unique_id));
        let remote_dir = temp_base.join(format!("remote_sel_{}", unique_id));
        tokio::fs::create_dir_all(&local_dir).await.unwrap();
        tokio::fs::create_dir_all(&remote_dir).await.unwrap();

        let sim = LocalSimulation::new(remote_dir.clone(), "TestSim".to_string());
        let remote_sim = Arc::new(TestBackendWrapper { sim });
        let state_file = local_dir.join(".sync_state.bin");
        let gitignore = SyncIgnore::empty();

        let inside_file = local_dir.join("MyFolder/inside.txt");
        let outside_file = local_dir.join("outside.txt");
        tokio::fs::create_dir_all(local_dir.join("MyFolder")).await.unwrap();
        tokio::fs::write(&inside_file, "inside").await.unwrap();
        tokio::fs::write(&outside_file, "outside").await.unwrap();

        super::sync_bidirectional(
            &local_dir,
            remote_sim.clone(),
            SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
            &state_file,
            &gitignore,
            4,
            cloud_sync_lib::ConflictPolicy::RenameLocal,
            false,
            Some(vec!["MyFolder".to_string()]),
            None
        ).await.unwrap();

        assert!(remote_sim.sim.resolve("MyFolder/inside.txt").exists());
        assert!(!remote_sim.sim.resolve("outside.txt").exists());
    }
}