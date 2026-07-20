#![no_std]

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

pub mod traits;
pub mod state;
pub mod path;

pub use traits::{StorageBackend, StorageError, StorageItem, SyncPolicy, SyncMode, ConflictPolicy};
pub use state::{SyncState, FileState};
pub use path::{normalize_remote_path, format_relative_path, format_absolute_path};
#[cfg(feature = "std")]
pub use path::get_permissions;
