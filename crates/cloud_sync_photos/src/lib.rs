//! `cloud_sync_photos` is a Google Photos equivalence engine that operates
//! over any supported cloud storage backend (Google Drive, OneDrive, S3, Dropbox, Mega, etc.).
//!
//! Features:
//! - Pure Rust EXIF and camera metadata extraction (GPS coordinates, camera model, date taken).
//! - Fast SHA-256 content deduplication (never uploads or duplicates identical photos).
//! - Chronological organization (`YYYY/MM/filename.ext`, `YYYY/MM/DD/filename.ext`, etc.).
//! - In-memory and persistent catalog indexing with timeline views and multi-criteria search.
//! - Virtual albums and favorites management without duplicating cloud files.
//! - Remote catalog sync (`.photos_catalog.json`) across any cloud provider.

pub mod catalog;
pub mod cli;
pub mod config;
pub mod engine;
pub mod metadata;
pub mod models;
pub mod organizer;

pub use catalog::PhotoCatalog;
pub use config::PhotosConfig;
pub use engine::{PhotoLibrary, PhotoSyncResult};
pub use models::{
    Album, ExifMetadata, GeoLocation, LibraryStats, MediaItem, MediaType, SearchFilter,
    TimelineGroup,
};
pub use organizer::OrganizeStrategy;
