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

/// Resolves conflicts by skipping automatic resolution (leaves files for manual/interactive UI resolution).
#[derive(Debug, Clone, Copy, Default)]
pub struct ManualStrategy;

impl ConflictResolver for ManualStrategy {
    fn policy(&self) -> ConflictPolicy {
        ConflictPolicy::Manual
    }
    fn resolve(
        &self,
        _rel_path: &str,
        _local_mtime: SystemTime,
        _remote_mtime: SystemTime,
    ) -> ConflictResolutionAction {
        ConflictResolutionAction::Skip
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
        ConflictPolicy::Manual => Box::new(ManualStrategy),
    }
}

/// Category of conflict detected during 3-way synchronization reconciliation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    ConcurrentModification,
    LocalModifiedRemoteDeleted,
    RemoteModifiedLocalDeleted,
    ConcurrentAddition,
}

/// Detailed conflict record for interactive resolution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ConflictRecord {
    pub id: String,
    pub rel_path: String,
    pub provider: String,
    pub conflict_type: ConflictType,
    pub local_size: Option<u64>,
    pub local_mtime: Option<u64>,
    pub remote_size: Option<u64>,
    pub remote_mtime: Option<u64>,
    pub detected_at: u64,
}

/// Interactive user resolution choices.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionChoice {
    UseLocal,
    UseRemote,
    KeepBoth,
    Skip,
}

/// Thread-safe in-memory manager for tracking and resolving pending conflicts.
#[derive(Debug, Default, Clone)]
pub struct ConflictManager {
    inner: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, ConflictRecord>>>,
}

impl ConflictManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records or updates a conflict record for a relative file path.
    pub async fn record_conflict(&self, record: ConflictRecord) {
        let mut map = self.inner.lock().await;
        map.insert(record.rel_path.clone(), record);
    }

    /// Lists all current pending conflicts sorted by relative path.
    pub async fn list_conflicts(&self) -> Vec<ConflictRecord> {
        let map = self.inner.lock().await;
        let mut list: Vec<ConflictRecord> = map.values().cloned().collect();
        list.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        list
    }

    /// Retrieves a conflict by relative path or ID.
    pub async fn get_conflict(&self, id_or_path: &str) -> Option<ConflictRecord> {
        let map = self.inner.lock().await;
        if let Some(record) = map.get(id_or_path) {
            return Some(record.clone());
        }
        map.values().find(|r| r.id == id_or_path).cloned()
    }

    /// Removes a resolved conflict record.
    pub async fn remove_conflict(&self, id_or_path: &str) -> Option<ConflictRecord> {
        let mut map = self.inner.lock().await;
        if let Some(removed) = map.remove(id_or_path) {
            return Some(removed);
        }
        let key_to_remove = map
            .iter()
            .find(|(_, r)| r.id == id_or_path)
            .map(|(k, _)| k.clone());
        if let Some(k) = key_to_remove {
            map.remove(&k)
        } else {
            None
        }
    }

    /// Clears all recorded conflicts.
    pub async fn clear(&self) {
        let mut map = self.inner.lock().await;
        map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_conflict_manager_flow() {
        let mgr = ConflictManager::new();
        assert!(mgr.list_conflicts().await.is_empty());

        let record = ConflictRecord {
            id: "conf-1".to_string(),
            rel_path: "docs/spec.txt".to_string(),
            provider: "Google Drive".to_string(),
            conflict_type: ConflictType::ConcurrentModification,
            local_size: Some(1024),
            local_mtime: Some(100),
            remote_size: Some(2048),
            remote_mtime: Some(200),
            detected_at: 1700000000,
        };

        mgr.record_conflict(record.clone()).await;

        let list = mgr.list_conflicts().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].rel_path, "docs/spec.txt");

        let fetched = mgr.get_conflict("docs/spec.txt").await;
        assert_eq!(fetched, Some(record.clone()));

        let fetched_by_id = mgr.get_conflict("conf-1").await;
        assert_eq!(fetched_by_id, Some(record.clone()));

        let removed = mgr.remove_conflict("docs/spec.txt").await;
        assert!(removed.is_some());
        assert!(mgr.list_conflicts().await.is_empty());
    }

    #[test]
    fn test_conflict_resolvers() {
        let now = SystemTime::now();
        let past = now - std::time::Duration::from_secs(60);

        let rename_local = create_conflict_resolver(ConflictPolicy::RenameLocal);
        assert_eq!(
            rename_local.resolve("test.txt", now, past),
            ConflictResolutionAction::RenameLocalAndDownloadRemote("test.txt.local-conflict".to_string())
        );

        let rename_remote = create_conflict_resolver(ConflictPolicy::RenameRemote);
        assert_eq!(
            rename_remote.resolve("test.txt", now, past),
            ConflictResolutionAction::RenameRemoteAndUploadLocal("test.txt.remote-conflict".to_string())
        );

        let keep_local = create_conflict_resolver(ConflictPolicy::KeepLocal);
        assert_eq!(
            keep_local.resolve("test.txt", now, past),
            ConflictResolutionAction::UploadLocal
        );

        let keep_remote = create_conflict_resolver(ConflictPolicy::KeepRemote);
        assert_eq!(
            keep_remote.resolve("test.txt", now, past),
            ConflictResolutionAction::DownloadRemote
        );

        let keep_newer = create_conflict_resolver(ConflictPolicy::KeepNewer);
        assert_eq!(
            keep_newer.resolve("test.txt", now, past),
            ConflictResolutionAction::UploadLocal
        );
        assert_eq!(
            keep_newer.resolve("test.txt", past, now),
            ConflictResolutionAction::DownloadRemote
        );

        let manual = create_conflict_resolver(ConflictPolicy::Manual);
        assert_eq!(
            manual.resolve("test.txt", now, past),
            ConflictResolutionAction::Skip
        );
    }
}
