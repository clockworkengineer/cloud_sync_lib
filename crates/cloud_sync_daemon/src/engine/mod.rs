pub mod conflict;
pub mod executor;
pub mod filter;
pub mod orchestrator;
pub mod plan;
pub mod planner;
pub mod rename_detector;
pub mod scanner;
pub mod state_repository;
pub mod versioning;

pub use conflict::{
    create_conflict_resolver, ConflictManager, ConflictRecord, ConflictResolutionAction,
    ConflictResolver, ConflictType, KeepLocalStrategy, KeepNewerStrategy, KeepRemoteStrategy,
    ManualStrategy, RenameLocalStrategy, RenameRemoteStrategy, ResolutionChoice,
};
pub use executor::{ExecutionResult, PlanExecutor};
pub use filter::FileFilter;
pub use orchestrator::{sync_bidirectional, SyncOrchestrator};
pub use plan::{SyncAction, SyncPlan};
pub use planner::SyncPlanner;
pub use rename_detector::{CandidateItem, DetectedMove, RenameDetector};
pub use scanner::{DirectoryScanner, ScannedItem};
pub use state_repository::{
    migrate_legacy_state_files, resolve_state_file_path, SyncStateRepository,
};
pub use versioning::{TrashManager, VersioningConfig};
