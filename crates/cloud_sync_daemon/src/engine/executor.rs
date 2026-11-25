//! Concurrent and Resumable Synchronization Plan Executor (SRP).
//!
//! Executes `SyncPlan` actions with optional trash retention, resumable chunked
//! transfers for large files, server-side moves, and deterministic state updates.

use cloud_sync_lib::{StorageBackend, StorageCapabilities, StorageError};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::engine::conflict::ConflictResolutionAction;
use crate::engine::orchestrator::SyncInterceptor;
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
        Self::execute_with_interceptor(backend, plan, None, trash_mgr, None, dry_run).await
    }

    /// Executes a `SyncPlan` with an optional local `watch_dir` for complete automated conflict resolution.
    pub async fn execute_with_watch_dir(
        backend: Arc<dyn StorageBackend>,
        plan: &SyncPlan,
        watch_dir: Option<&std::path::Path>,
        trash_mgr: Option<&TrashManager>,
        dry_run: bool,
    ) -> Result<ExecutionResult, StorageError> {
        Self::execute_with_interceptor(backend, plan, watch_dir, trash_mgr, None, dry_run).await
    }

    /// Executes a `SyncPlan` with optional `watch_dir`, `TrashManager`, and [`SyncInterceptor`] lifecycle hooks.
    pub async fn execute_with_interceptor(
        backend: Arc<dyn StorageBackend>,
        plan: &SyncPlan,
        watch_dir: Option<&std::path::Path>,
        trash_mgr: Option<&TrashManager>,
        interceptor: Option<&dyn SyncInterceptor>,
        dry_run: bool,
    ) -> Result<ExecutionResult, StorageError> {
        Self::execute_with_options(backend, plan, watch_dir, trash_mgr, interceptor, dry_run, 1)
            .await
    }

    /// Executes a `SyncPlan` with bounded concurrency (`max_concurrency`), optional `watch_dir`,
    /// `TrashManager`, and [`SyncInterceptor`] lifecycle hooks.
    pub async fn execute_with_options(
        backend: Arc<dyn StorageBackend>,
        plan: &SyncPlan,
        watch_dir: Option<&std::path::Path>,
        trash_mgr: Option<&TrashManager>,
        interceptor: Option<&dyn SyncInterceptor>,
        dry_run: bool,
        max_concurrency: usize,
    ) -> Result<ExecutionResult, StorageError> {
        let concurrency = max_concurrency.max(1);
        let capabilities = backend.capabilities();

        if concurrency > 1 && !dry_run && plan.actions.len() > 1 {
            // Partition actions into topological phases:
            // Phase 1: Transfers, moves, and creations (concurrently bounded)
            // Phase 2: Deletions (executed in reverse order so children are deleted before parent directories)
            let mut transfers = Vec::new();
            let mut deletions = Vec::new();

            for action in &plan.actions {
                match action {
                    SyncAction::DeleteLocal { .. } | SyncAction::DeleteRemote { .. } => {
                        deletions.push(action);
                    }
                    _ => {
                        transfers.push(action);
                    }
                }
            }

            let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
            let mut futures = Vec::with_capacity(transfers.len());

            for action in transfers {
                let sem = semaphore.clone();
                let backend_ref = backend.clone();
                futures.push(async move {
                    let _permit = sem.acquire().await.ok();
                    Self::execute_single_action(
                        backend_ref,
                        capabilities,
                        action,
                        watch_dir,
                        trash_mgr,
                        interceptor,
                        dry_run,
                    )
                    .await
                });
            }

            let results = futures_util::future::join_all(futures).await;
            let mut total = ExecutionResult {
                executed_actions: 0,
                failed_actions: 0,
                bytes_uploaded: 0,
                bytes_downloaded: 0,
            };
            for r in results {
                total.executed_actions += r.executed_actions;
                total.failed_actions += r.failed_actions;
                total.bytes_uploaded += r.bytes_uploaded;
                total.bytes_downloaded += r.bytes_downloaded;
            }

            // Phase 2: Execute deletions in reverse depth order
            deletions.reverse();
            for del in deletions {
                let r = Self::execute_single_action(
                    backend.clone(),
                    capabilities,
                    del,
                    watch_dir,
                    trash_mgr,
                    interceptor,
                    dry_run,
                )
                .await;
                total.executed_actions += r.executed_actions;
                total.failed_actions += r.failed_actions;
            }

            return Ok(total);
        }

        let mut total = ExecutionResult {
            executed_actions: 0,
            failed_actions: 0,
            bytes_uploaded: 0,
            bytes_downloaded: 0,
        };

        for action in &plan.actions {
            let r = Self::execute_single_action(
                backend.clone(),
                capabilities,
                action,
                watch_dir,
                trash_mgr,
                interceptor,
                dry_run,
            )
            .await;
            total.executed_actions += r.executed_actions;
            total.failed_actions += r.failed_actions;
            total.bytes_uploaded += r.bytes_uploaded;
            total.bytes_downloaded += r.bytes_downloaded;
        }

        Ok(total)
    }

    async fn execute_single_action(
        backend: Arc<dyn StorageBackend>,
        capabilities: StorageCapabilities,
        action: &SyncAction,
        watch_dir: Option<&std::path::Path>,
        trash_mgr: Option<&TrashManager>,
        interceptor: Option<&dyn SyncInterceptor>,
        dry_run: bool,
    ) -> ExecutionResult {
        let mut executed_actions = 0;
        let mut failed_actions = 0;
        let mut bytes_uploaded = 0;
        let mut bytes_downloaded = 0;

        if let Some(hook) = interceptor {
            if let Err(e) = hook.before_action(action).await {
                warn!(
                    "[{}] Action canceled by SyncInterceptor::before_action: {}",
                    backend.name(),
                    e
                );
                hook.on_error(action, &e).await;
                return ExecutionResult {
                    executed_actions: 0,
                    failed_actions: 1,
                    bytes_uploaded: 0,
                    bytes_downloaded: 0,
                };
            }
        }
        let action_start = std::time::Instant::now();

        if dry_run {
            info!("[{}] [DRY-RUN] Would execute: {:?}", backend.name(), action);
            if let Some(hook) = interceptor {
                hook.after_action(action, action_start.elapsed()).await;
            }
            return ExecutionResult {
                executed_actions: 1,
                failed_actions: 0,
                bytes_uploaded: 0,
                bytes_downloaded: 0,
            };
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
                let res = if local_path.is_dir() {
                    tokio::fs::remove_dir_all(local_path).await
                } else {
                    tokio::fs::remove_file(local_path).await
                };
                match res {
                    Ok(()) => executed_actions += 1,
                    Err(e) => {
                        error!(
                            "[{}] Delete local failed for '{}': {}",
                            backend.name(),
                            local_path.display(),
                            e
                        );
                        failed_actions += 1;
                    }
                }
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
                        if let Some(wdir) = watch_dir {
                            let local_path = wdir.join(rel_path);
                            if let Err(e) = backend.upload(&local_path, rel_path).await {
                                error!(
                                    "[{}] Conflict UploadLocal failed for '{}': {}",
                                    backend.name(),
                                    rel_path,
                                    e
                                );
                                failed_actions += 1;
                            } else {
                                executed_actions += 1;
                            }
                        } else {
                            executed_actions += 1;
                        }
                    }
                    ConflictResolutionAction::DownloadRemote => {
                        if let Some(wdir) = watch_dir {
                            let local_path = wdir.join(rel_path);
                            if let Some(tm) = trash_mgr {
                                let _ = tm.archive_file(&local_path).await;
                            }
                            if let Err(e) = backend.download(rel_path, &local_path).await {
                                error!(
                                    "[{}] Conflict DownloadRemote failed for '{}': {}",
                                    backend.name(),
                                    rel_path,
                                    e
                                );
                                failed_actions += 1;
                            } else {
                                executed_actions += 1;
                            }
                        } else {
                            executed_actions += 1;
                        }
                    }
                    ConflictResolutionAction::RenameLocalAndDownloadRemote(new_local) => {
                        let mut err_occurred = false;
                        if let Some(wdir) = watch_dir {
                            let orig_local = wdir.join(rel_path);
                            let renamed_local = wdir.join(new_local);
                            if orig_local.exists() {
                                let _ = tokio::fs::rename(&orig_local, &renamed_local).await;
                                let _ = backend.upload(&renamed_local, new_local).await;
                            }
                            if let Err(e) = backend.download(rel_path, &orig_local).await {
                                error!(
                                    "[{}] Conflict download after local rename failed for '{}': {}",
                                    backend.name(),
                                    rel_path,
                                    e
                                );
                                failed_actions += 1;
                                err_occurred = true;
                            }
                        }
                        if !err_occurred {
                            info!("Renamed local conflict to '{}'", new_local);
                            executed_actions += 1;
                        }
                    }
                    ConflictResolutionAction::RenameRemoteAndUploadLocal(new_remote) => {
                        let mut err_occurred = false;
                        let _ = backend.rename(rel_path, new_remote).await;
                        if let Some(wdir) = watch_dir {
                            let orig_local = wdir.join(rel_path);
                            let renamed_local = wdir.join(new_remote);
                            let _ = backend.download(new_remote, &renamed_local).await;
                            if orig_local.exists() {
                                if let Err(e) = backend.upload(&orig_local, rel_path).await {
                                    error!(
                                            "[{}] Conflict upload after remote rename failed for '{}': {}",
                                            backend.name(),
                                            rel_path,
                                            e
                                        );
                                    failed_actions += 1;
                                    err_occurred = true;
                                }
                            }
                        }
                        if !err_occurred {
                            info!("Renamed remote conflict to '{}'", new_remote);
                            executed_actions += 1;
                        }
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

        if let Some(hook) = interceptor {
            if failed_actions == 0 {
                hook.after_action(action, action_start.elapsed()).await;
            } else {
                let err = StorageError::Provider {
                    message: format!("SyncAction failed on {}", backend.name()),
                    status: None,
                };
                hook.on_error(action, &err).await;
            }
        }

        ExecutionResult {
            executed_actions,
            failed_actions,
            bytes_uploaded,
            bytes_downloaded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_sync_lib::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_plan_executor_concurrent_transfers() {
        let local_dir = tempdir().unwrap();
        let remote_dir = tempdir().unwrap();

        let backend: Arc<dyn StorageBackend> = Arc::new(LocalSimulation::new(
            remote_dir.path().to_path_buf(),
            "concurrent_test".into(),
        ));

        let mut actions = Vec::new();
        for i in 0..8 {
            let file_path = local_dir.path().join(format!("file_{}.txt", i));
            let content = format!("concurrent payload {}", i);
            tokio::fs::write(&file_path, content.as_bytes())
                .await
                .unwrap();
            actions.push(SyncAction::Upload {
                local_path: file_path,
                remote_path: format!("batch/file_{}.txt", i),
                size: content.len() as u64,
            });
        }

        let plan = SyncPlan {
            actions,
            ..Default::default()
        };

        let result = PlanExecutor::execute_with_options(
            backend.clone(),
            &plan,
            Some(local_dir.path()),
            None,
            None,
            false,
            4,
        )
        .await
        .unwrap();

        assert_eq!(result.executed_actions, 8);
        assert_eq!(result.failed_actions, 0);
        assert!(result.bytes_uploaded > 0);

        let remote_files = backend.list("batch").await.unwrap();
        assert_eq!(remote_files.len(), 8);
    }

    #[tokio::test]
    async fn test_plan_executor_all_actions_and_dry_run() {
        let local_dir = tempdir().unwrap();
        let remote_dir = tempdir().unwrap();

        let backend: Arc<dyn StorageBackend> = Arc::new(LocalSimulation::new(
            remote_dir.path().to_path_buf(),
            "actions_test".into(),
        ));

        // 1. Upload and Download
        let test_file = local_dir.path().join("upload_me.txt");
        tokio::fs::write(&test_file, b"test content").await.unwrap();

        let download_dest = local_dir.path().join("downloaded.txt");
        let plan_transfer = SyncPlan {
            actions: vec![SyncAction::Upload {
                local_path: test_file.clone(),
                remote_path: "uploaded.txt".to_string(),
                size: 12,
            }],
            ..Default::default()
        };

        let res_up = PlanExecutor::execute_with_watch_dir(
            backend.clone(),
            &plan_transfer,
            Some(local_dir.path()),
            None,
            false,
        )
        .await
        .unwrap();
        assert_eq!(res_up.executed_actions, 1);
        assert_eq!(res_up.bytes_uploaded, 12);

        let plan_down = SyncPlan {
            actions: vec![SyncAction::Download {
                remote_path: "uploaded.txt".to_string(),
                local_path: download_dest.clone(),
                size: 12,
                checksum: None,
                permissions: None,
            }],
            ..Default::default()
        };
        let res_down = PlanExecutor::execute(backend.clone(), &plan_down, None, false)
            .await
            .unwrap();
        assert_eq!(res_down.executed_actions, 1);
        assert_eq!(res_down.bytes_downloaded, 12);
        assert!(download_dest.exists());

        // 2. ServerSideMove and ServerSideCopy
        let plan_moves = SyncPlan {
            actions: vec![
                SyncAction::ServerSideCopy {
                    from: "uploaded.txt".to_string(),
                    to: "copied_remote.txt".to_string(),
                },
                SyncAction::ServerSideMove {
                    from: "uploaded.txt".to_string(),
                    to: "moved_remote.txt".to_string(),
                },
            ],
            ..Default::default()
        };
        let res_moves = PlanExecutor::execute(backend.clone(), &plan_moves, None, false)
            .await
            .unwrap();
        assert_eq!(res_moves.executed_actions, 2);

        // 3. DeleteLocal and DeleteRemote
        let plan_delete = SyncPlan {
            actions: vec![
                SyncAction::DeleteLocal {
                    local_path: download_dest.clone(),
                },
                SyncAction::DeleteRemote {
                    remote_path: "moved_remote.txt".to_string(),
                },
            ],
            ..Default::default()
        };
        let res_del = PlanExecutor::execute(backend.clone(), &plan_delete, None, false)
            .await
            .unwrap();
        assert_eq!(res_del.executed_actions, 2);
        assert!(!download_dest.exists());

        // 4. Conflicts handling
        let conflict_file = local_dir.path().join("conflict.txt");
        tokio::fs::write(&conflict_file, b"local conflict")
            .await
            .unwrap();
        backend
            .upload(&conflict_file, "conflict.txt")
            .await
            .unwrap();

        let plan_conflicts = SyncPlan {
            actions: vec![
                SyncAction::Conflict {
                    rel_path: "conflict.txt".to_string(),
                    action: ConflictResolutionAction::Skip,
                },
                SyncAction::Conflict {
                    rel_path: "conflict.txt".to_string(),
                    action: ConflictResolutionAction::RenameLocalAndDownloadRemote(
                        "conflict.txt.local_copy".to_string(),
                    ),
                },
                SyncAction::ConvergeState {
                    rel_path: "conflict.txt".to_string(),
                    state: cloud_sync_lib::FileState {
                        size: 14,
                        local_modified: std::time::SystemTime::now(),
                        remote_modified: std::time::SystemTime::now(),
                        is_dir: Some(false),
                        checksum: None,
                        permissions: None,
                    },
                },
            ],
            ..Default::default()
        };
        let res_conf = PlanExecutor::execute_with_watch_dir(
            backend.clone(),
            &plan_conflicts,
            Some(local_dir.path()),
            None,
            false,
        )
        .await
        .unwrap();
        assert_eq!(res_conf.executed_actions, 3);
        assert!(local_dir.path().join("conflict.txt.local_copy").exists());

        // 5. Dry run execution
        let dry_plan = SyncPlan {
            actions: vec![SyncAction::Upload {
                local_path: test_file.clone(),
                remote_path: "never_uploaded.txt".to_string(),
                size: 12,
            }],
            ..Default::default()
        };
        let res_dry = PlanExecutor::execute(backend.clone(), &dry_plan, None, true)
            .await
            .unwrap();
        assert_eq!(res_dry.executed_actions, 1);
        assert_eq!(res_dry.bytes_uploaded, 0); // No real bytes transferred during dry run
    }
}
