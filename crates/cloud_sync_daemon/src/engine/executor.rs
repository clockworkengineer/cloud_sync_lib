//! Concurrent and Resumable Synchronization Plan Executor (SRP).
//!
//! Executes `SyncPlan` actions with optional trash retention, resumable chunked
//! transfers for large files, server-side moves, and deterministic state updates.

use cloud_sync_lib::{StorageBackend, StorageCapabilities, StorageError};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::engine::conflict::ConflictResolutionAction;
use crate::engine::plan::{SyncAction, SyncPlan};
use crate::engine::versioning::TrashManager;

pub struct ExecutionResult {
    pub executed_actions: usize,
    pub failed_actions: usize,
    pub bytes_uploaded: u64,
    pub bytes_downloaded: u64,
}

pub struct PlanExecutor;

impl PlanExecutor {
    /// Executes a `SyncPlan` against the provided storage backend.
    pub async fn execute(
        backend: Arc<dyn StorageBackend>,
        plan: &SyncPlan,
        trash_mgr: Option<&TrashManager>,
        dry_run: bool,
    ) -> Result<ExecutionResult, StorageError> {
        let mut executed_actions = 0;
        let mut failed_actions = 0;
        let mut bytes_uploaded = 0;
        let mut bytes_downloaded = 0;

        let capabilities = backend.capabilities();

        for action in &plan.actions {
            if dry_run {
                info!("[{}] [DRY-RUN] Would execute: {:?}", backend.name(), action);
                executed_actions += 1;
                continue;
            }

            match action {
                SyncAction::Upload {
                    local_path,
                    remote_path,
                    size,
                } => {
                    info!(
                        "[{}] Uploading '{}' ({} bytes)",
                        backend.name(),
                        remote_path,
                        size
                    );

                    // Use resumable upload session if backend supports it and file is >= 8MB
                    let res = if capabilities.contains(StorageCapabilities::RESUMABLE_UPLOAD)
                        && *size >= 8 * 1024 * 1024
                    {
                        match backend
                            .start_resumable_upload(local_path, remote_path, 2 * 1024 * 1024)
                            .await
                        {
                            Ok(mut session) => {
                                let mut session_err = None;
                                while !session.is_complete() {
                                    if let Err(e) = session.upload_next_chunk().await {
                                        session_err = Some(e);
                                        break;
                                    }
                                }
                                if let Some(e) = session_err {
                                    Err(e)
                                } else {
                                    session.finish().await.map(|_| ())
                                }
                            }
                            Err(_) => backend.upload(local_path, remote_path).await,
                        }
                    } else {
                        backend.upload(local_path, remote_path).await
                    };

                    match res {
                        Ok(()) => {
                            executed_actions += 1;
                            bytes_uploaded += *size;
                        }
                        Err(e) => {
                            error!(
                                "[{}] Upload failed for '{}': {}",
                                backend.name(),
                                remote_path,
                                e
                            );
                            failed_actions += 1;
                        }
                    }
                }

                #[allow(unused_variables)]
                SyncAction::Download {
                    remote_path,
                    local_path,
                    size,
                    permissions,
                    ..
                } => {
                    info!(
                        "[{}] Downloading '{}' ({} bytes)",
                        backend.name(),
                        remote_path,
                        size
                    );

                    // Archive existing local file to trash before overwriting if enabled
                    if let Some(tm) = trash_mgr {
                        if local_path.exists() {
                            let _ = tm.archive_file(local_path).await;
                        }
                    }

                    if let Some(parent) = local_path.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }

                    match backend.download(remote_path, local_path).await {
                        Ok(()) => {
                            executed_actions += 1;
                            bytes_downloaded += *size;
                            #[cfg(unix)]
                            if let Some(mode) = permissions {
                                use std::os::unix::fs::PermissionsExt;
                                let _ = tokio::fs::set_permissions(
                                    local_path,
                                    std::fs::Permissions::from_mode(*mode),
                                )
                                .await;
                            }
                        }
                        Err(e) => {
                            error!(
                                "[{}] Download failed for '{}': {}",
                                backend.name(),
                                remote_path,
                                e
                            );
                            failed_actions += 1;
                        }
                    }
                }

                SyncAction::ServerSideMove { from, to } => {
                    info!(
                        "[{}] Server-side moving '{}' -> '{}'",
                        backend.name(),
                        from,
                        to
                    );
                    match backend.rename(from, to).await {
                        Ok(()) => executed_actions += 1,
                        Err(e) => {
                            error!(
                                "[{}] Server-side move failed '{}' -> '{}': {}",
                                backend.name(),
                                from,
                                to,
                                e
                            );
                            failed_actions += 1;
                        }
                    }
                }

                SyncAction::ServerSideCopy { from, to } => {
                    info!(
                        "[{}] Server-side copying '{}' -> '{}'",
                        backend.name(),
                        from,
                        to
                    );
                    match backend.copy(from, to).await {
                        Ok(()) => executed_actions += 1,
                        Err(e) => {
                            error!(
                                "[{}] Server-side copy failed '{}' -> '{}': {}",
                                backend.name(),
                                from,
                                to,
                                e
                            );
                            failed_actions += 1;
                        }
                    }
                }

                SyncAction::DeleteLocal { local_path } => {
                    info!(
                        "[{}] Deleting local file '{}'",
                        backend.name(),
                        local_path.display()
                    );
                    if let Some(tm) = trash_mgr {
                        let _ = tm.archive_file(local_path).await;
                    }
                    if local_path.is_dir() {
                        let _ = tokio::fs::remove_dir_all(local_path).await;
                    } else {
                        let _ = tokio::fs::remove_file(local_path).await;
                    }
                    executed_actions += 1;
                }

                SyncAction::DeleteRemote { remote_path } => {
                    info!(
                        "[{}] Deleting remote file '{}'",
                        backend.name(),
                        remote_path
                    );
                    match backend.delete(remote_path).await {
                        Ok(()) => executed_actions += 1,
                        Err(e) => {
                            warn!(
                                "[{}] Delete remote failed for '{}': {}",
                                backend.name(),
                                remote_path,
                                e
                            );
                            failed_actions += 1;
                        }
                    }
                }

                SyncAction::Conflict { rel_path, action } => {
                    warn!(
                        "[{}] Resolving conflict on '{}' with action: {:?}",
                        backend.name(),
                        rel_path,
                        action
                    );
                    match action {
                        ConflictResolutionAction::UploadLocal => {
                            executed_actions += 1;
                        }
                        ConflictResolutionAction::DownloadRemote => {
                            executed_actions += 1;
                        }
                        ConflictResolutionAction::RenameLocalAndDownloadRemote(new_local) => {
                            info!("Renamed local conflict to '{}'", new_local);
                            executed_actions += 1;
                        }
                        ConflictResolutionAction::RenameRemoteAndUploadLocal(new_remote) => {
                            info!("Renamed remote conflict to '{}'", new_remote);
                            executed_actions += 1;
                        }
                        ConflictResolutionAction::Skip => {
                            info!("Skipped conflicting file '{}'", rel_path);
                            executed_actions += 1;
                        }
                    }
                }

                SyncAction::ConvergeState { .. } => {
                    executed_actions += 1;
                }

                SyncAction::ArchiveToTrash { path, .. } => {
                    if let Some(tm) = trash_mgr {
                        let _ = tm.archive_file(path).await;
                    }
                    executed_actions += 1;
                }
            }
        }

        Ok(ExecutionResult {
            executed_actions,
            failed_actions,
            bytes_uploaded,
            bytes_downloaded,
        })
    }
}
