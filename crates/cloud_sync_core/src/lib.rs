#![no_std]

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

pub mod path;
pub mod state;
#[cfg(feature = "std")]
pub mod state_io;
#[cfg(feature = "std")]
pub mod sys;
#[cfg(feature = "std")]
pub mod testing;
pub mod traits;

#[cfg(feature = "std")]
pub use path::get_permissions;
pub use path::{format_absolute_path, format_relative_path, normalize_remote_path, NormalizedPath};
pub use state::{FileState, SyncState};
#[cfg(feature = "std")]
pub use state_io::{JsonStateStore, PostcardStateStore, StateStore};
#[cfg(feature = "std")]
pub use sys::{Clock, FileSystem, RealFileSystem, SystemClock};
#[cfg(feature = "std")]
pub use testing::verify_backend_compliance;
#[cfg(feature = "std")]
pub use traits::{ResumableUploadSession, ResumableUploader};
pub use traits::{
    ChangeFeed, ChecksumOps, ConflictPolicy, DirectoryLister, FolderOps, RemoteChange,
    ServerSideOperations, StorageBackend, StorageCapabilities, StorageError, StorageItem,
    StorageReader, StorageWriter, SyncMode, SyncPolicy,
};
