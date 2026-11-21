//! Multi-Cloud Union / Virtual RAID Storage Backend (Rclone `union` equivalent).
//!
//! Combines multiple underlying [`StorageBackend`] instances into a single unified virtual
//! backend with configurable write/read policies:
//! - [`UnionPolicy::MirrorAll`]: Writes to all member backends (RAID-1 redundancy) and reads from the first healthy backend with automatic failover.
//! - [`UnionPolicy::FirstAvailable`]: Writes to the first available upstream, falling back to subsequent upstreams on failure.
//! - [`UnionPolicy::RoundRobin`]: Distributes uploads evenly across member backends to pool storage quotas across accounts.

use crate::traits::{StorageBackend, StorageCapabilities, StorageError, StorageItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Write and allocation policy for a [`UnionBackend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UnionPolicy {
    /// Replicate writes to all upstreams simultaneously (RAID-1 cloud mirroring).
    #[default]
    MirrorAll,
    /// Write to the first healthy upstream in priority order (Active-Passive failover).
    FirstAvailable,
    /// Distribute new uploads across upstreams in round-robin order (Storage quota pooling).
    RoundRobin,
}

/// Virtual storage backend that unifies multiple upstream [`StorageBackend`] instances.
pub struct UnionBackend {
    name: String,
    upstreams: Vec<Arc<dyn StorageBackend>>,
    policy: UnionPolicy,
    rr_counter: AtomicUsize,
}

impl UnionBackend {
    /// Creates a new `UnionBackend` combining the provided upstream backends under `policy`.
    pub fn new(
        name: impl Into<String>,
        upstreams: Vec<Arc<dyn StorageBackend>>,
        policy: UnionPolicy,
    ) -> Self {
        Self {
            name: name.into(),
            upstreams,
            policy,
            rr_counter: AtomicUsize::new(0),
        }
    }

    /// Returns the list of member upstreams.
    pub fn upstreams(&self) -> &[Arc<dyn StorageBackend>] {
        &self.upstreams
    }

    /// Returns the active union policy.
    pub fn policy(&self) -> UnionPolicy {
        self.policy
    }
}

#[async_trait]
impl StorageBackend for UnionBackend {
    fn name(&self) -> &str {
        &self.name
    }

    async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError> {
        if self.upstreams.is_empty() {
            return Ok(Vec::new());
        }

        let mut merged: HashMap<String, StorageItem> = HashMap::new();
        let mut last_err = None;
        let mut any_ok = false;

        for upstream in &self.upstreams {
            match upstream.list(remote_path).await {
                Ok(items) => {
                    any_ok = true;
                    for item in items {
                        let key = item.path.to_string_lossy().replace('\\', "/");
                        merged
                            .entry(key)
                            .and_modify(|existing| {
                                if item.modified > existing.modified {
                                    *existing = item.clone();
                                }
                            })
                            .or_insert(item);
                    }
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }

        if !any_ok {
            if let Some(e) = last_err {
                return Err(e);
            }
        }

        let mut list: Vec<StorageItem> = merged.into_values().collect();
        list.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(list)
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError> {
        if self.upstreams.is_empty() {
            return Err(StorageError::Provider {
                message: "UnionBackend has no configured upstreams".into(),
                status: None,
            });
        }

        match self.policy {
            UnionPolicy::MirrorAll => {
                let mut last_err = None;
                let mut success_count = 0;
                for upstream in &self.upstreams {
                    match upstream.upload(local_path, remote_path).await {
                        Ok(()) => success_count += 1,
                        Err(e) => last_err = Some(e),
                    }
                }
                if success_count == self.upstreams.len() {
                    Ok(())
                } else {
                    Err(last_err.unwrap())
                }
            }
            UnionPolicy::FirstAvailable => {
                let mut last_err = None;
                for upstream in &self.upstreams {
                    match upstream.upload(local_path, remote_path).await {
                        Ok(()) => return Ok(()),
                        Err(e) => last_err = Some(e),
                    }
                }
                Err(last_err.unwrap())
            }
            UnionPolicy::RoundRobin => {
                let n = self.upstreams.len();
                let start = self.rr_counter.fetch_add(1, Ordering::Relaxed) % n;
                let mut last_err = None;
                for offset in 0..n {
                    let idx = (start + offset) % n;
                    match self.upstreams[idx].upload(local_path, remote_path).await {
                        Ok(()) => return Ok(()),
                        Err(e) => last_err = Some(e),
                    }
                }
                Err(last_err.unwrap())
            }
        }
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError> {
        let mut last_err = None;
        for upstream in &self.upstreams {
            match upstream.download(remote_path, local_path).await {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| StorageError::NotFound(remote_path.to_string())))
    }

    async fn delete(&self, remote_path: &str) -> Result<(), StorageError> {
        let mut any_deleted = false;
        let mut last_err = None;
        for upstream in &self.upstreams {
            match upstream.delete(remote_path).await {
                Ok(()) => any_deleted = true,
                Err(e) => last_err = Some(e),
            }
        }
        if any_deleted {
            Ok(())
        } else {
            Err(last_err.unwrap_or_else(|| StorageError::NotFound(remote_path.to_string())))
        }
    }

    async fn create_folder(&self, remote_path: &str) -> Result<(), StorageError> {
        let mut any_ok = false;
        let mut last_err = None;
        for upstream in &self.upstreams {
            match upstream.create_folder(remote_path).await {
                Ok(()) => any_ok = true,
                Err(e) => last_err = Some(e),
            }
        }
        if any_ok {
            Ok(())
        } else {
            Err(last_err.unwrap_or_else(|| StorageError::Provider {
                message: "create_folder failed on all union upstreams".into(),
                status: None,
            }))
        }
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let mut any_ok = false;
        let mut last_err = None;
        for upstream in &self.upstreams {
            match upstream.rename(from, to).await {
                Ok(()) => any_ok = true,
                Err(e) => last_err = Some(e),
            }
        }
        if any_ok {
            Ok(())
        } else {
            Err(last_err.unwrap_or_else(|| StorageError::NotFound(from.to_string())))
        }
    }

    fn capabilities(&self) -> StorageCapabilities {
        if let Some(first) = self.upstreams.first() {
            let mut caps = first.capabilities();
            for u in self.upstreams.iter().skip(1) {
                caps = StorageCapabilities(caps.0 & u.capabilities().0);
            }
            caps
        } else {
            StorageCapabilities::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_union_backend_mirror_all_and_failover() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        let local = tempdir().unwrap();

        let b1: Arc<dyn StorageBackend> = Arc::new(LocalSimulation::new(
            dir1.path().to_path_buf(),
            "CloudA".into(),
        ));
        let b2: Arc<dyn StorageBackend> = Arc::new(LocalSimulation::new(
            dir2.path().to_path_buf(),
            "CloudB".into(),
        ));

        let union = UnionBackend::new(
            "UnionMirror",
            vec![b1.clone(), b2.clone()],
            UnionPolicy::MirrorAll,
        );

        let src = local.path().join("hello.txt");
        tokio::fs::write(&src, b"redundant payload").await.unwrap();

        union.upload(&src, "hello.txt").await.unwrap();

        // Both upstreams must contain the file
        assert_eq!(b1.list("").await.unwrap().len(), 1);
        assert_eq!(b2.list("").await.unwrap().len(), 1);

        // Merged list deduplicates the file
        let listed = union.list("").await.unwrap();
        assert_eq!(listed.len(), 1);

        // Delete from primary upstream, verify union still downloads seamlessly from secondary
        b1.delete("hello.txt").await.unwrap();
        let dl = local.path().join("recovered.txt");
        union.download("hello.txt", &dl).await.unwrap();
        assert_eq!(
            tokio::fs::read_to_string(&dl).await.unwrap(),
            "redundant payload"
        );
    }

    #[tokio::test]
    async fn test_union_backend_round_robin_distribution() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        let local = tempdir().unwrap();

        let b1: Arc<dyn StorageBackend> = Arc::new(LocalSimulation::new(
            dir1.path().to_path_buf(),
            "Shard1".into(),
        ));
        let b2: Arc<dyn StorageBackend> = Arc::new(LocalSimulation::new(
            dir2.path().to_path_buf(),
            "Shard2".into(),
        ));

        let union = UnionBackend::new(
            "UnionSharded",
            vec![b1.clone(), b2.clone()],
            UnionPolicy::RoundRobin,
        );

        let f1 = local.path().join("f1.txt");
        let f2 = local.path().join("f2.txt");
        tokio::fs::write(&f1, b"part 1").await.unwrap();
        tokio::fs::write(&f2, b"part 2").await.unwrap();

        union.upload(&f1, "f1.txt").await.unwrap();
        union.upload(&f2, "f2.txt").await.unwrap();

        // Each shard receives 1 file, union lists all 2
        assert_eq!(b1.list("").await.unwrap().len(), 1);
        assert_eq!(b2.list("").await.unwrap().len(), 1);
        assert_eq!(union.list("").await.unwrap().len(), 2);
    }
}
