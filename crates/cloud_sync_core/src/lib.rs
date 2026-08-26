#![no_std]

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

pub mod traits;
pub mod state;
pub mod path;
#[cfg(feature = "std")]
pub mod sys;

pub use traits::{StorageBackend, StorageError, StorageItem, SyncPolicy, SyncMode, ConflictPolicy, StorageReader, StorageWriter, DirectoryLister, FolderOps, ChecksumOps};
pub use state::{SyncState, FileState};
pub use path::{NormalizedPath, normalize_remote_path, format_relative_path, format_absolute_path};
#[cfg(feature = "std")]
pub use path::get_permissions;
#[cfg(feature = "std")]
pub use sys::{Clock, SystemClock, FileSystem, RealFileSystem};


