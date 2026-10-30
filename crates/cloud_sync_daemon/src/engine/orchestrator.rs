use cloud_sync_lib::sys::{Clock, FileSystem, RealFileSystem, SystemClock};
use cloud_sync_lib::{ConflictPolicy, SyncIgnore};

use std::sync::Arc;
use std::time::SystemTime;

use crate::engine::conflict::{create_conflict_resolver, ConflictResolutionAction};
use crate::engine::executor::{ExecutionResult, PlanExecutor};
use crate::engine::filter::FileFilter;
use crate::engine::plan::SyncPlan;
use crate::engine::planner::SyncPlanner;
use crate::engine::scanner::DirectoryScanner;
use crate::engine::state_repository::SyncStateRepository;
use crate::engine::versioning::TrashManager;
use cloud_sync_lib::StorageBackend;

/// Orchestrator coordinating synchronization operations across local filesystems and remote backends (SRP & DIP).
pub struct SyncOrchestrator {
    fs: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    state_repo: SyncStateRepository,
}

impl SyncOrchestrator {
    pub fn new(fs: Arc<dyn FileSystem>, clock: Arc<dyn Clock>) -> Self {
        Self {
            fs,
            clock,
            state_repo: SyncStateRepository::new(),
        }
    }

    pub fn default_real() -> Self {
        Self::new(Arc::new(RealFileSystem), Arc::new(SystemClock))
    }

    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }

    pub fn fs(&self) -> &dyn FileSystem {
        self.fs.as_ref()
    }

    pub fn state_repo(&self) -> &SyncStateRepository {
        &self.state_repo
    }

    /// Scans local and remote trees and computes a deterministic `SyncPlan`.
    pub async fn plan_sync(
        &self,
        backend: &dyn StorageBackend,
        watch_dir: &std::path::Path,
        state_file: &std::path::Path,
        filter: &FileFilter<'_>,
        policy: cloud_sync_lib::SyncPolicy,
        conflict_policy: ConflictPolicy,
    ) -> Result<SyncPlan, Box<dyn std::error::Error + Send + Sync>> {
        let base_state = self.state_repo.load(state_file).await;
        let remote_tree = DirectoryScanner::scan_remote(backend, filter).await?;
        let local_tree = DirectoryScanner::scan_local(watch_dir, filter).await?;

        Ok(SyncPlanner::plan(
            backend.name(),
            backend.capabilities(),
            watch_dir,
            &local_tree,
            &remote_tree,
            &base_state,
            policy,
            conflict_policy,
        ))
    }

    /// Executes a computed `SyncPlan` with optional trash can versioning.
    pub async fn execute_sync(
        &self,
        backend: Arc<dyn StorageBackend>,
        plan: &SyncPlan,
        trash_mgr: Option<&TrashManager>,
        dry_run: bool,
    ) -> Result<ExecutionResult, cloud_sync_lib::StorageError> {
        PlanExecutor::execute(backend, plan, trash_mgr, dry_run).await
    }

    /// Resolves a file conflict using an injected ConflictResolver strategy.
    pub fn resolve_conflict(
        &self,
        policy: ConflictPolicy,
        rel_path: &str,
        local_mtime: SystemTime,
        remote_mtime: SystemTime,
    ) -> ConflictResolutionAction {
        let resolver = create_conflict_resolver(policy);
        resolver.resolve(rel_path, local_mtime, remote_mtime)
    }

    /// Creates a FileFilter instance for path sanitization and ignore checking.
    pub fn create_filter<'a>(
        &self,
        gitignore: &'a SyncIgnore,
        selective_sync: Option<&'a [String]>,
    ) -> FileFilter<'a> {
        FileFilter::new(gitignore, selective_sync)
    }

    /// Performs a full bidirectional sync iteration using the segregated scanner, planner, and executor pipeline (SRP).
    #[allow(clippy::too_many_arguments)]
    pub async fn sync_bidirectional(
        &self,
        watch_dir: &std::path::Path,
        backend: Arc<dyn StorageBackend>,
        policy: cloud_sync_lib::SyncPolicy,
        state_file_path: &std::path::Path,
        gitignore: &SyncIgnore,
        _max_concurrency: usize,
        conflict_policy: ConflictPolicy,
        dry_run: bool,
        selective_sync: Option<Vec<String>>,
        event_tx: Option<tokio::sync::broadcast::Sender<String>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let backend_name = backend.name().to_string();
        let broadcast_event =
            |event_type: &str, message: &str, progress: Option<f64>, active_file: Option<&str>| {
                if let Some(ref tx) = event_tx {
                    let payload = serde_json::json!({
                        "type": event_type,
                        "message": message,
                        "progress_percent": progress,
                        "active_file": active_file,
                        "backend_name": backend_name,
                    })
                    .to_string();
                    let _ = tx.send(payload);
                }
            };

        broadcast_event("status", "Starting synchronization...", Some(0.0), None);

        let sel_ref = selective_sync.as_deref();
        let filter = self.create_filter(gitignore, sel_ref);

        broadcast_event("status", "Scanning files and computing plan...", Some(15.0), None);
        let plan = self
            .plan_sync(
                backend.as_ref(),
                watch_dir,
                state_file_path,
                &filter,
                policy,
                conflict_policy,
            )
            .await?;

        broadcast_event(
            "status",
            &format!("Executing {} sync action(s)...", plan.actions.len()),
            Some(40.0),
            None,
        );
        let _result = self
            .execute_sync(backend.clone(), &plan, None, dry_run)
            .await?;

        if !dry_run {
            let mut state = self.state_repo.load(state_file_path).await;
            for action in &plan.actions {
                match action {
                    crate::engine::plan::SyncAction::ConvergeState { rel_path, state: s } => {
                        state.files.insert(rel_path.clone(), s.clone());
                    }
                    crate::engine::plan::SyncAction::DeleteLocal { local_path } => {
                        if let Ok(rel) = local_path.strip_prefix(watch_dir) {
                            let rel_str = rel.to_string_lossy().replace('\\', "/");
                            state.files.remove(&rel_str);
                        }
                    }
                    crate::engine::plan::SyncAction::DeleteRemote { remote_path } => {
                        state.files.remove(remote_path);
                    }
                    crate::engine::plan::SyncAction::Upload {
                        local_path,
                        remote_path,
                        size,
                    } => {
                        let mtime = std::fs::metadata(local_path)
                            .and_then(|m| m.modified())
                            .unwrap_or_else(|_| SystemTime::now());
                        state.files.insert(
                            remote_path.clone(),
                            cloud_sync_lib::FileState {
                                size: *size,
                                local_modified: mtime,
                                remote_modified: mtime,
                                is_dir: Some(false),
                                checksum: None,
                                permissions: None,
                            },
                        );
                    }
                    crate::engine::plan::SyncAction::Download {
                        remote_path,
                        local_path,
                        size,
                        checksum,
                        permissions,
                    } => {
                        let mtime = std::fs::metadata(local_path)
                            .and_then(|m| m.modified())
                            .unwrap_or_else(|_| SystemTime::now());
                        state.files.insert(
                            remote_path.clone(),
                            cloud_sync_lib::FileState {
                                size: *size,
                                local_modified: mtime,
                                remote_modified: mtime,
                                is_dir: Some(false),
                                checksum: checksum.clone(),
                                permissions: *permissions,
                            },
                        );
                    }
                    _ => {}
                }
            }
            let _ = self.state_repo.save(&state, state_file_path).await;
        }

        broadcast_event("status", "Synchronization complete.", Some(100.0), None);
        Ok(())
    }
}

/// Standalone convenience function executing bidirectional synchronization.
#[allow(clippy::too_many_arguments)]
pub async fn sync_bidirectional(
    watch_dir: &std::path::Path,
    backend: Arc<dyn StorageBackend>,
    policy: cloud_sync_lib::SyncPolicy,
    state_file_path: &std::path::Path,
    gitignore: &SyncIgnore,
    max_concurrency: usize,
    conflict_policy: ConflictPolicy,
    dry_run: bool,
    selective_sync: Option<Vec<String>>,
    event_tx: Option<tokio::sync::broadcast::Sender<String>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    SyncOrchestrator::default_real()
        .sync_bidirectional(
            watch_dir,
            backend,
            policy,
            state_file_path,
            gitignore,
            max_concurrency,
            conflict_policy,
            dry_run,
            selective_sync,
            event_tx,
        )
        .await
}

impl Default for SyncOrchestrator {
    fn default() -> Self {
        Self::default_real()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_sync_lib::providers::local_sim::LocalSimulation;
    use cloud_sync_lib::{SyncMode, SyncPolicy};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_orchestrator_bidirectional_sync_flow() {
        let watch_dir = tempdir().unwrap();
        let sim_dir = tempdir().unwrap();
        let state_dir = tempdir().unwrap();

        // Write a test local file
        let local_file = watch_dir.path().join("document.txt");
        tokio::fs::write(&local_file, b"Hello Cloud Sync SRP Engine!").await.unwrap();

        let backend: Arc<dyn StorageBackend> = Arc::new(LocalSimulation::new(
            sim_dir.path().to_path_buf(),
            "local_test".to_string(),
        ));

        let state_file = state_dir.path().join(".sync_state.bin");
        let gitignore = SyncIgnore::new(watch_dir.path(), &[]);

        let orchestrator = SyncOrchestrator::default_real();
        let res = orchestrator
            .sync_bidirectional(
                watch_dir.path(),
                backend.clone(),
                SyncPolicy::new(SyncMode::TwoWay),
                &state_file,
                &gitignore,
                4,
                ConflictPolicy::RenameLocal,
                false,
                None,
                None,
            )
            .await;

        assert!(res.is_ok());

        // Verify remote file created
        let remote_items = backend.list("").await.unwrap();
        assert_eq!(remote_items.len(), 1);
        assert_eq!(remote_items[0].path, std::path::Path::new("document.txt"));

        // Verify state catalog updated
        let state = orchestrator.state_repo().load(&state_file).await;
        assert!(state.files.contains_key("document.txt"));
    }
}
