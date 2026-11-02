//! 3-Way Synchronization Reconciliation & Planning Engine.
//!
//! Evaluates Base ($B$), Local ($L$), and Remote ($R$) states to compute an
//! optimal, deterministic `SyncPlan` with rename detection and conflict resolution.

use cloud_sync_lib::{ConflictPolicy, StorageCapabilities, SyncPolicy, SyncState};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::engine::conflict::create_conflict_resolver;
use crate::engine::plan::{SyncAction, SyncPlan};
use crate::engine::rename_detector::{CandidateItem, RenameDetector};
use crate::engine::scanner::ScannedItem;

pub struct SyncPlanner;

impl SyncPlanner {
    #[allow(clippy::too_many_arguments)]
    pub fn plan(
        backend_name: &str,
        capabilities: StorageCapabilities,
        watch_dir: &Path,
        local_tree: &HashMap<String, ScannedItem>,
        remote_tree: &HashMap<String, ScannedItem>,
        base_state: &SyncState,
        policy: SyncPolicy,
        conflict_policy: ConflictPolicy,
    ) -> SyncPlan {
        let mut plan = SyncPlan::new(backend_name.to_string());
        let sync_both = policy.sync_both();
        let sync_deletions = policy.sync_deletions();

        // 1. Collect all distinct relative paths
        let mut all_paths = HashSet::new();
        all_paths.extend(local_tree.keys().cloned());
        all_paths.extend(remote_tree.keys().cloned());
        all_paths.extend(base_state.files.keys().cloned());

        let mut candidate_deletions = Vec::new();
        let mut candidate_additions = Vec::new();

        for rel_path in &all_paths {
            let local_opt = local_tree.get(rel_path);
            let remote_opt = remote_tree.get(rel_path);
            let state_opt = base_state.files.get(rel_path);

            match (local_opt, remote_opt, state_opt) {
                // Case 1: Exists everywhere -> check for modifications
                (Some(local), Some(remote), Some(state)) => {
                    let local_changed = local.size != state.size
                        || local.modified != state.local_modified
                        || (local.checksum.is_some()
                            && state.checksum.is_some()
                            && local.checksum != state.checksum);
                    let remote_changed = remote.size != state.size
                        || remote.modified != state.remote_modified
                        || (remote.checksum.is_some()
                            && state.checksum.is_some()
                            && remote.checksum != state.checksum);

                    if local_changed && remote_changed {
                        let same_content = match (&local.checksum, &remote.checksum) {
                            (Some(lc), Some(rc)) => lc == rc,
                            _ => local.size == remote.size && local.size == 0,
                        };

                        if same_content {
                            plan.add_action(SyncAction::ConvergeState {
                                rel_path: rel_path.clone(),
                                state: cloud_sync_lib::FileState {
                                    permissions: local.permissions.or(remote.permissions),
                                    size: local.size,
                                    local_modified: local.modified,
                                    remote_modified: remote.modified,
                                    is_dir: Some(local.is_dir),
                                    checksum: local
                                        .checksum
                                        .clone()
                                        .or_else(|| remote.checksum.clone()),
                                },
                            });
                        } else {
                            let resolver = create_conflict_resolver(conflict_policy);
                            let action =
                                resolver.resolve(rel_path, local.modified, remote.modified);
                            plan.add_action(SyncAction::Conflict {
                                rel_path: rel_path.clone(),
                                action,
                            });
                        }
                    } else if local_changed {
                        plan.add_action(SyncAction::Upload {
                            local_path: watch_dir.join(rel_path),
                            remote_path: rel_path.clone(),
                            size: local.size,
                        });
                    } else if remote_changed && sync_both {
                        plan.add_action(SyncAction::Download {
                            remote_path: rel_path.clone(),
                            local_path: watch_dir.join(rel_path),
                            size: remote.size,
                            checksum: remote.checksum.clone(),
                            permissions: remote.permissions,
                        });
                    }
                }

                // Case 2: Only local -> New local addition
                (Some(local), None, None) => {
                    candidate_additions.push(CandidateItem {
                        rel_path: rel_path.clone(),
                        size: local.size,
                        checksum: local.checksum.clone(),
                    });
                }

                // Case 3: Only remote -> New remote addition
                (None, Some(remote), None) => {
                    if sync_both {
                        plan.add_action(SyncAction::Download {
                            remote_path: rel_path.clone(),
                            local_path: watch_dir.join(rel_path),
                            size: remote.size,
                            checksum: remote.checksum.clone(),
                            permissions: remote.permissions,
                        });
                    }
                }

                // Case 4: Base + Remote, Local deleted
                (None, Some(_remote), Some(_state)) => {
                    if sync_deletions {
                        candidate_deletions.push(CandidateItem {
                            rel_path: rel_path.clone(),
                            size: _state.size,
                            checksum: _state.checksum.clone(),
                        });
                    }
                }

                // Case 5: Base + Local, Remote deleted
                (Some(_local), None, Some(_state)) => {
                    if sync_both && sync_deletions {
                        plan.add_action(SyncAction::DeleteLocal {
                            local_path: watch_dir.join(rel_path),
                        });
                    } else if !sync_both {
                        plan.add_action(SyncAction::Upload {
                            local_path: watch_dir.join(rel_path),
                            remote_path: rel_path.clone(),
                            size: _local.size,
                        });
                    }
                }

                // Case 6: Local and Remote newly added simultaneously without Base
                (Some(local), Some(remote), None) => {
                    let same_content = match (&local.checksum, &remote.checksum) {
                        (Some(lc), Some(rc)) => lc == rc,
                        _ => local.size == remote.size && local.size == 0,
                    };

                    if same_content {
                        plan.add_action(SyncAction::ConvergeState {
                            rel_path: rel_path.clone(),
                            state: cloud_sync_lib::FileState {
                                permissions: local.permissions.or(remote.permissions),
                                size: local.size,
                                local_modified: local.modified,
                                remote_modified: remote.modified,
                                is_dir: Some(local.is_dir),
                                checksum: local
                                    .checksum
                                    .clone()
                                    .or_else(|| remote.checksum.clone()),
                            },
                        });
                    } else {
                        let resolver = create_conflict_resolver(conflict_policy);
                        let action = resolver.resolve(rel_path, local.modified, remote.modified);
                        plan.add_action(SyncAction::Conflict {
                            rel_path: rel_path.clone(),
                            action,
                        });
                    }
                }

                _ => {}
            }
        }

        // 2. Perform Rename Detection if server-side move capability is available
        if capabilities.contains(StorageCapabilities::SERVER_SIDE_MOVE)
            && !candidate_deletions.is_empty()
            && !candidate_additions.is_empty()
        {
            let (moves, remaining_del, remaining_add) =
                RenameDetector::detect_moves(&candidate_deletions, &candidate_additions);

            for mv in moves {
                plan.add_action(SyncAction::ServerSideMove {
                    from: mv.from,
                    to: mv.to,
                });
            }

            for del in remaining_del {
                plan.add_action(SyncAction::DeleteRemote {
                    remote_path: del.rel_path,
                });
            }

            for add in remaining_add {
                let local_size = local_tree
                    .get(&add.rel_path)
                    .map(|i| i.size)
                    .unwrap_or(add.size);
                plan.add_action(SyncAction::Upload {
                    local_path: watch_dir.join(&add.rel_path),
                    remote_path: add.rel_path,
                    size: local_size,
                });
            }
        } else {
            for del in candidate_deletions {
                plan.add_action(SyncAction::DeleteRemote {
                    remote_path: del.rel_path,
                });
            }
            for add in candidate_additions {
                let local_size = local_tree
                    .get(&add.rel_path)
                    .map(|i| i.size)
                    .unwrap_or(add.size);
                plan.add_action(SyncAction::Upload {
                    local_path: watch_dir.join(&add.rel_path),
                    remote_path: add.rel_path,
                    size: local_size,
                });
            }
        }

        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn make_scanned(size: u64, secs: u64, hash: Option<&str>) -> ScannedItem {
        ScannedItem {
            size,
            modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs),
            is_dir: false,
            checksum: hash.map(|h| h.into()),
            permissions: Some(0o644),
        }
    }

    #[test]
    fn test_sync_planner_local_addition() {
        let watch_dir = Path::new("/sync");
        let mut local_tree = HashMap::new();
        local_tree.insert("new_file.txt".into(), make_scanned(100, 1000, Some("h1")));
        let remote_tree = HashMap::new();
        let base_state = SyncState::default();

        let plan = SyncPlanner::plan(
            "test_backend",
            StorageCapabilities::from_bits_truncate(0),
            watch_dir,
            &local_tree,
            &remote_tree,
            &base_state,
            SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
            ConflictPolicy::RenameLocal,
        );

        assert_eq!(plan.action_count(), 1);
        match &plan.actions[0] {
            SyncAction::Upload {
                remote_path, size, ..
            } => {
                assert_eq!(remote_path, "new_file.txt");
                assert_eq!(*size, 100);
            }
            other => panic!("expected Upload, got {:?}", other),
        }
    }

    #[test]
    fn test_sync_planner_remote_addition() {
        let watch_dir = Path::new("/sync");
        let local_tree = HashMap::new();
        let mut remote_tree = HashMap::new();
        remote_tree.insert("from_cloud.txt".into(), make_scanned(250, 2000, Some("h2")));
        let base_state = SyncState::default();

        let plan = SyncPlanner::plan(
            "test_backend",
            StorageCapabilities::from_bits_truncate(0),
            watch_dir,
            &local_tree,
            &remote_tree,
            &base_state,
            SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
            ConflictPolicy::RenameLocal,
        );

        assert_eq!(plan.action_count(), 1);
        match &plan.actions[0] {
            SyncAction::Download {
                remote_path, size, ..
            } => {
                assert_eq!(remote_path, "from_cloud.txt");
                assert_eq!(*size, 250);
            }
            other => panic!("expected Download, got {:?}", other),
        }
    }

    #[test]
    fn test_sync_planner_both_modified_conflict() {
        let watch_dir = Path::new("/sync");
        let mut local_tree = HashMap::new();
        local_tree.insert(
            "shared.txt".into(),
            make_scanned(100, 2000, Some("hash_local")),
        );
        let mut remote_tree = HashMap::new();
        remote_tree.insert(
            "shared.txt".into(),
            make_scanned(200, 3000, Some("hash_remote")),
        );

        let mut base_state = SyncState::default();
        base_state.files.insert(
            "shared.txt".into(),
            cloud_sync_lib::FileState {
                size: 50,
                local_modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000),
                remote_modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000),
                is_dir: Some(false),
                checksum: Some("hash_base".into()),
                permissions: Some(0o644),
            },
        );

        let plan = SyncPlanner::plan(
            "test_backend",
            StorageCapabilities::from_bits_truncate(0),
            watch_dir,
            &local_tree,
            &remote_tree,
            &base_state,
            SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
            ConflictPolicy::RenameLocal,
        );

        assert_eq!(plan.conflicts_count, 1);
        let conflict_action = plan
            .actions
            .iter()
            .find(|a| matches!(a, SyncAction::Conflict { .. }));
        assert!(conflict_action.is_some());
    }

    #[test]
    fn test_sync_planner_local_deletion() {
        let watch_dir = Path::new("/sync");
        let local_tree = HashMap::new(); // deleted locally
        let mut remote_tree = HashMap::new();
        remote_tree.insert("del.txt".into(), make_scanned(80, 1000, Some("h_del")));

        let mut base_state = SyncState::default();
        base_state.files.insert(
            "del.txt".into(),
            cloud_sync_lib::FileState {
                size: 80,
                local_modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000),
                remote_modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000),
                is_dir: Some(false),
                checksum: Some("h_del".into()),
                permissions: Some(0o644),
            },
        );

        let plan = SyncPlanner::plan(
            "test_backend",
            StorageCapabilities::from_bits_truncate(0),
            watch_dir,
            &local_tree,
            &remote_tree,
            &base_state,
            SyncPolicy::new(cloud_sync_lib::SyncMode::TwoWay),
            ConflictPolicy::RenameLocal,
        );

        assert_eq!(plan.action_count(), 1);
        match &plan.actions[0] {
            SyncAction::DeleteRemote { remote_path } => {
                assert_eq!(remote_path, "del.txt");
            }
            other => panic!("expected DeleteRemote, got {:?}", other),
        }
    }
}
