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
}

impl Default for SyncOrchestrator {
    fn default() -> Self {
        Self::default_real()
    }
}
