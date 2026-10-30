//! Structured Synchronization Plan and Action specifications.
//!
//! Provides inspectable, serializable sync execution plans supporting
//! dry-run previews, user inspection, and decoupled execution.

use chrono::{DateTime, Utc};
use cloud_sync_lib::FileState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::engine::conflict::ConflictResolutionAction;

/// An atomic operation to be performed during synchronization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SyncAction {
    Upload {
        local_path: PathBuf,
        remote_path: String,
        size: u64,
    },
    Download {
        remote_path: String,
        local_path: PathBuf,
        size: u64,
        checksum: Option<String>,
        permissions: Option<u32>,
    },
    ServerSideMove {
        from: String,
        to: String,
    },
    ServerSideCopy {
        from: String,
        to: String,
    },
    DeleteLocal {
        local_path: PathBuf,
    },
    DeleteRemote {
        remote_path: String,
    },
    ArchiveToTrash {
        path: PathBuf,
        is_remote: bool,
    },
    Conflict {
        rel_path: String,
        action: ConflictResolutionAction,
    },
    ConvergeState {
        rel_path: String,
        state: FileState,
    },
}

/// A complete, ordered synchronization plan for a backend.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncPlan {
    pub backend_name: String,
    pub created_at: DateTime<Utc>,
    pub actions: Vec<SyncAction>,
    pub total_upload_bytes: u64,
    pub total_download_bytes: u64,
    pub conflicts_count: usize,
}

impl SyncPlan {
    pub fn new(backend_name: String) -> Self {
        Self {
            backend_name,
            created_at: Utc::now(),
            actions: Vec::new(),
            total_upload_bytes: 0,
            total_download_bytes: 0,
            conflicts_count: 0,
        }
    }

    pub fn add_action(&mut self, action: SyncAction) {
        match &action {
            SyncAction::Upload { size, .. } => {
                self.total_upload_bytes = self.total_upload_bytes.saturating_add(*size);
            }
            SyncAction::Download { size, .. } => {
                self.total_download_bytes = self.total_download_bytes.saturating_add(*size);
            }
            SyncAction::Conflict { .. } => {
                self.conflicts_count = self.conflicts_count.saturating_add(1);
            }
            _ => {}
        }
        self.actions.push(action);
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    pub fn action_count(&self) -> usize {
        self.actions.len()
    }
}
