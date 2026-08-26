use cloud_sync_lib::sys::{Clock, FileSystem, RealFileSystem, SystemClock};
use cloud_sync_lib::{SyncIgnore, ConflictPolicy};

use std::sync::Arc;
use std::time::SystemTime;

use crate::engine::conflict::{create_conflict_resolver, ConflictResolutionAction};
use crate::engine::filter::FileFilter;
use crate::engine::state_repository::SyncStateRepository;

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
