#![no_std]

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

pub mod path;
pub mod state;
#[cfg(feature = "std")]
pub mod sys;
pub mod traits;

#[cfg(feature = "std")]
pub use path::get_permissions;
pub use path::{format_absolute_path, format_relative_path, normalize_remote_path, NormalizedPath};
pub use state::{FileState, SyncState};
#[cfg(feature = "std")]
pub use sys::{Clock, FileSystem, RealFileSystem, SystemClock};
pub use traits::{
    ChecksumOps, ConflictPolicy, DirectoryLister, FolderOps, RemoteChange, StorageBackend,
    StorageError, StorageItem, StorageReader, StorageWriter, SyncMode, SyncPolicy,
};
