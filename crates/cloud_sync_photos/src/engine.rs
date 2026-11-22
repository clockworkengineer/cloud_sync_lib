//! Synchronization and library orchestration engine for Google Photos equivalence.

use crate::catalog::PhotoCatalog;
use crate::metadata::inspect_media_file;
use crate::models::{LibraryStats, MediaItem, SearchFilter, TimelineGroup};
use crate::organizer::{generate_remote_path, OrganizeStrategy};
use cloud_sync_core::{NormalizedPath, StorageBackend};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Summary of a photo synchronization pass.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PhotoSyncResult {
    pub files_scanned: usize,
    pub files_imported: usize,
    pub files_skipped_duplicates: usize,
    pub bytes_uploaded: u64,
    pub errors: Vec<String>,
}

/// The core Google Photos engine managing local catalog and remote cloud sync.
pub struct PhotoLibrary {
    backend: Arc<dyn StorageBackend>,
    remote_root: NormalizedPath,
    local_catalog_path: PathBuf,
    catalog: Arc<RwLock<PhotoCatalog>>,
    strategy: OrganizeStrategy,
}

impl PhotoLibrary {
    /// Creates a new photo library bound to any storage backend.
    pub async fn new(
        backend: Arc<dyn StorageBackend>,
        remote_root: &str,
        local_catalog_path: PathBuf,
        strategy: OrganizeStrategy,
    ) -> Result<Self, String> {
        let root_path = NormalizedPath::new(remote_root).map_err(|e| e.to_string())?;

        let catalog = if local_catalog_path.exists() {
            PhotoCatalog::load_from_file(&local_catalog_path)
                .unwrap_or_else(|_| PhotoCatalog::default())
        } else {
            PhotoCatalog::default()
        };

        Ok(Self {
            backend,
            remote_root: root_path,
            local_catalog_path,
            catalog: Arc::new(RwLock::new(catalog)),
            strategy,
        })
    }

    /// Returns a reference to the in-memory catalog.
    pub fn catalog(&self) -> Arc<RwLock<PhotoCatalog>> {
        self.catalog.clone()
    }

    /// Saves the current catalog to the local disk.
    pub async fn save_local_catalog(&self) -> Result<(), String> {
        let cat = self.catalog.read().await;
        cat.save_to_file(&self.local_catalog_path)
            .map_err(|e| e.to_string())
    }

    /// Downloads and merges the remote catalog (`.photos_catalog.json`) from the cloud backend.
    pub async fn pull_remote_catalog(&self) -> Result<(), String> {
        let catalog_filename = ".photos_catalog.json";
        let remote_catalog_path = if self.remote_root.as_str().is_empty() {
            NormalizedPath::new(catalog_filename).map_err(|e| e.to_string())?
        } else {
            NormalizedPath::new(&format!(
                "{}/{}",
                self.remote_root.as_str(),
                catalog_filename
            ))
            .map_err(|e| e.to_string())?
        };

        let temp_guard = cloud_sync_lib::TempFileGuard::new("photos_cat_pull");
        if self
            .backend
            .download(remote_catalog_path.as_str(), temp_guard.path())
            .await
            .is_ok()
        {
            if let Ok(remote_catalog) = PhotoCatalog::load_from_file(temp_guard.path()) {
                let mut local = self.catalog.write().await;
                local.merge(remote_catalog);
            }
        }

        self.save_local_catalog().await
    }

    /// Pushes the updated local catalog (`.photos_catalog.json`) up to the cloud backend.
    pub async fn push_remote_catalog(&self) -> Result<(), String> {
        self.save_local_catalog().await?;

        let catalog_filename = ".photos_catalog.json";
        let remote_catalog_path = if self.remote_root.as_str().is_empty() {
            NormalizedPath::new(catalog_filename).map_err(|e| e.to_string())?
        } else {
            NormalizedPath::new(&format!(
                "{}/{}",
                self.remote_root.as_str(),
                catalog_filename
            ))
            .map_err(|e| e.to_string())?
        };

        let temp_guard = cloud_sync_lib::TempFileGuard::new("photos_cat_push");
        {
            let cat = self.catalog.read().await;
            cat.save_to_file(temp_guard.path())
                .map_err(|e| e.to_string())?;
        }

        self.backend
            .upload(temp_guard.path(), remote_catalog_path.as_str())
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Scans a local folder (e.g. Android DCIM or local Pictures), deduplicates against the catalog,
    /// uploads new photos to the remote cloud backend, and updates the catalog.
    pub async fn sync_folder(&self, folder_path: &Path) -> Result<PhotoSyncResult, String> {
        let mut result = PhotoSyncResult::default();

        if !folder_path.exists() || !folder_path.is_dir() {
            return Err(format!(
                "Source folder does not exist or is not a directory: {:?}",
                folder_path
            ));
        }

        // 1. Pull remote catalog first for multi-device sync
        let _ = self.pull_remote_catalog().await;

        // 2. Discover local media files
        let mut media_paths = Vec::new();
        self.collect_media_files(folder_path, &mut media_paths);
        result.files_scanned = media_paths.len();

        // 3. Process each media file
        for path in media_paths {
            match self.import_file_with_status(&path).await {
                Ok((uploaded, bytes)) => {
                    if uploaded {
                        result.files_imported += 1;
                        result.bytes_uploaded += bytes;
                    } else {
                        result.files_skipped_duplicates += 1;
                    }
                }
                Err(e) => {
                    result
                        .errors
                        .push(format!("{}: {}", path.to_string_lossy(), e));
                }
            }
        }

        // 4. Save and push updated catalog
        let _ = self.push_remote_catalog().await;

        Ok(result)
    }

    /// Imports a single local file: parses metadata, deduplicates by hash, and uploads if new.
    /// Returns Ok((uploaded: bool, bytes_uploaded: u64)).
    pub async fn import_file_with_status(&self, path: &Path) -> Result<(bool, u64), String> {
        let mut item = inspect_media_file(path).map_err(|e| e.to_string())?;

        // Deduplication check: Has this exact photo content already been imported?
        {
            let cat = self.catalog.read().await;
            if cat.contains_hash(&item.sha256_hash) {
                return Ok((false, 0));
            }
        }

        // Determine remote path using selected organization strategy
        let rel_remote = generate_remote_path(&item, self.strategy);
        let full_remote_str = if self.remote_root.as_str().is_empty() {
            rel_remote.clone()
        } else {
            format!("{}/{}", self.remote_root.as_str(), rel_remote)
        };

        let remote_norm_path = NormalizedPath::new(&full_remote_str).map_err(|e| e.to_string())?;

        // Upload to the cloud backend
        self.backend
            .upload(path, remote_norm_path.as_str())
            .await
            .map_err(|e| format!("Failed to upload to cloud: {}", e))?;

        let bytes_uploaded = item.size_bytes;
        item.remote_path = Some(full_remote_str);

        // Record in catalog
        {
            let mut cat = self.catalog.write().await;
            cat.insert_item(item);
        }

        Ok((true, bytes_uploaded))
    }

    /// Downloads a media item from the cloud backend to a target local destination.
    pub async fn download_item(&self, item_id: &str, target_path: &Path) -> Result<(), String> {
        let remote_path = {
            let cat = self.catalog.read().await;
            let item = cat
                .items
                .get(item_id)
                .ok_or_else(|| format!("Media item not found in catalog: {}", item_id))?;
            item.remote_path
                .clone()
                .ok_or_else(|| "Media item has no remote cloud path".to_string())?
        };

        let norm_path = NormalizedPath::new(&remote_path).map_err(|e| e.to_string())?;
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        self.backend
            .download(norm_path.as_str(), target_path)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Generates the chronological timeline view.
    pub async fn get_timeline(&self) -> Vec<TimelineGroup> {
        let cat = self.catalog.read().await;
        cat.get_timeline()
    }

    /// Searches the library.
    pub async fn search(&self, filter: &SearchFilter) -> Vec<MediaItem> {
        let cat = self.catalog.read().await;
        cat.search(filter)
    }

    /// Computes library statistics.
    pub async fn get_stats(&self) -> LibraryStats {
        let cat = self.catalog.read().await;
        cat.compute_stats()
    }

    /// Toggles favorite flag for an item.
    pub async fn toggle_favorite(&self, item_id: &str) -> Option<bool> {
        let res = {
            let mut cat = self.catalog.write().await;
            cat.toggle_favorite(item_id)
        };
        if res.is_some() {
            let _ = self.save_local_catalog().await;
        }
        res
    }

    /// Creates a virtual album.
    pub async fn create_album(&self, name: String, description: Option<String>) -> String {
        let id = {
            let mut cat = self.catalog.write().await;
            cat.create_album(name, description)
        };
        let _ = self.save_local_catalog().await;
        id
    }

    /// Adds photos to an album.
    pub async fn add_to_album(&self, album_id: &str, item_ids: &[String]) -> bool {
        let ok = {
            let mut cat = self.catalog.write().await;
            cat.add_to_album(album_id, item_ids)
        };
        if ok {
            let _ = self.save_local_catalog().await;
        }
        ok
    }

    // Recursively discovers supported photo and video files
    fn collect_media_files(&self, dir: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    self.collect_media_files(&path, files);
                } else if path.is_file() {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();

                    let is_media = matches!(
                        ext.as_str(),
                        "jpg"
                            | "jpeg"
                            | "png"
                            | "webp"
                            | "heic"
                            | "heif"
                            | "gif"
                            | "tif"
                            | "tiff"
                            | "raw"
                            | "cr2"
                            | "nef"
                            | "arw"
                            | "dng"
                            | "mp4"
                            | "mov"
                            | "mkv"
                            | "webm"
                            | "avi"
                    );

                    if is_media {
                        files.push(path);
                    }
                }
            }
        }
    }
}
