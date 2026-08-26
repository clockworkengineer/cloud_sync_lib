use cloud_sync_lib::ConflictPolicy;
use std::time::SystemTime;


/// Action to be taken when a conflict between local and remote file is detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolutionAction {
    UploadLocal,
    DownloadRemote,
    RenameLocalAndDownloadRemote(String),
    RenameRemoteAndUploadLocal(String),
    Skip,
}

/// Strategy trait for resolving sync conflicts (Open/Closed Principle & Strategy Pattern).
pub trait ConflictResolver: Send + Sync {
    fn policy(&self) -> ConflictPolicy;
    fn resolve(
        &self,
        rel_path: &str,
        local_mtime: SystemTime,
        remote_mtime: SystemTime,
    ) -> ConflictResolutionAction;
}

/// Resolves conflicts by renaming the local file with `.local-conflict` suffix.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenameLocalStrategy;

impl ConflictResolver for RenameLocalStrategy {
    fn policy(&self) -> ConflictPolicy {
        ConflictPolicy::RenameLocal
    }
    fn resolve(
        &self,
        rel_path: &str,
        _local_mtime: SystemTime,
        _remote_mtime: SystemTime,
    ) -> ConflictResolutionAction {
        let conflict_path = format!("{}.local-conflict", rel_path);
        ConflictResolutionAction::RenameLocalAndDownloadRemote(conflict_path)
    }
}

/// Resolves conflicts by renaming the remote file.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenameRemoteStrategy;

impl ConflictResolver for RenameRemoteStrategy {
    fn policy(&self) -> ConflictPolicy {
        ConflictPolicy::RenameRemote
    }
    fn resolve(
        &self,
        rel_path: &str,
        _local_mtime: SystemTime,
        _remote_mtime: SystemTime,
    ) -> ConflictResolutionAction {
        let conflict_path = format!("{}.remote-conflict", rel_path);
        ConflictResolutionAction::RenameRemoteAndUploadLocal(conflict_path)
    }
}

/// Resolves conflicts by selecting whichever file has the newer modification time.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeepNewerStrategy;

impl ConflictResolver for KeepNewerStrategy {
    fn policy(&self) -> ConflictPolicy {
        ConflictPolicy::KeepNewer
    }
    fn resolve(
        &self,
        _rel_path: &str,
        local_mtime: SystemTime,
        remote_mtime: SystemTime,
    ) -> ConflictResolutionAction {
        if local_mtime > remote_mtime {
            ConflictResolutionAction::UploadLocal
        } else {
            ConflictResolutionAction::DownloadRemote
        }
    }
}

/// Resolves conflicts by keeping the local file.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeepLocalStrategy;

impl ConflictResolver for KeepLocalStrategy {
    fn policy(&self) -> ConflictPolicy {
        ConflictPolicy::KeepLocal
    }
    fn resolve(
        &self,
        _rel_path: &str,
        _local_mtime: SystemTime,
        _remote_mtime: SystemTime,
    ) -> ConflictResolutionAction {
        ConflictResolutionAction::UploadLocal
    }
}

/// Resolves conflicts by keeping the remote file.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeepRemoteStrategy;

impl ConflictResolver for KeepRemoteStrategy {
    fn policy(&self) -> ConflictPolicy {
        ConflictPolicy::KeepRemote
    }
    fn resolve(
        &self,
        _rel_path: &str,
        _local_mtime: SystemTime,
        _remote_mtime: SystemTime,
    ) -> ConflictResolutionAction {
        ConflictResolutionAction::DownloadRemote
    }
}

/// Factory function to dynamically create a `ConflictResolver` strategy from `ConflictPolicy`.
pub fn create_conflict_resolver(policy: ConflictPolicy) -> Box<dyn ConflictResolver> {
    match policy {
        ConflictPolicy::RenameLocal => Box::new(RenameLocalStrategy),
        ConflictPolicy::RenameRemote => Box::new(RenameRemoteStrategy),
        ConflictPolicy::KeepNewer => Box::new(KeepNewerStrategy),
        ConflictPolicy::KeepLocal => Box::new(KeepLocalStrategy),
        ConflictPolicy::KeepRemote => Box::new(KeepRemoteStrategy),
    }
}
