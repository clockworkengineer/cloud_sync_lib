//! In-memory and persistent catalog for photos, albums, and metadata indexing.

use crate::models::{Album, LibraryStats, MediaItem, SearchFilter, TimelineGroup};
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Version identifier for the photo catalog file format.
pub const CATALOG_VERSION: u32 = 1;

/// The central catalog indexing all photos, videos, albums, and relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoCatalog {
    pub version: u32,
    pub updated_at: chrono::DateTime<Utc>,
    pub items: HashMap<String, MediaItem>,
    pub albums: HashMap<String, Album>,
    #[serde(skip)]
    pub hash_index: HashMap<String, String>,
}

impl Default for PhotoCatalog {
    fn default() -> Self {
        Self {
            version: CATALOG_VERSION,
            updated_at: Utc::now(),
            items: HashMap::new(),
            albums: HashMap::new(),
            hash_index: HashMap::new(),
        }
    }
}

impl PhotoCatalog {
    /// Rebuilds internal secondary indices (like the SHA-256 hash lookup).
    pub fn rebuild_indices(&mut self) {
        self.hash_index.clear();
        for (id, item) in &self.items {
            self.hash_index.insert(item.sha256_hash.clone(), id.clone());
        }
    }

    /// Checks if a media item with the specified content hash already exists.
    pub fn contains_hash(&self, hash: &str) -> bool {
        self.hash_index.contains_key(hash) || self.items.values().any(|it| it.sha256_hash == hash)
    }

    /// Retrieves an existing item matching the SHA-256 content hash.
    pub fn get_by_hash(&self, hash: &str) -> Option<&MediaItem> {
        if let Some(id) = self.hash_index.get(hash) {
            return self.items.get(id);
        }
        self.items.values().find(|it| it.sha256_hash == hash)
    }

    /// Inserts or updates a media item in the catalog.
    pub fn insert_item(&mut self, item: MediaItem) {
        self.hash_index
            .insert(item.sha256_hash.clone(), item.id.clone());
        self.items.insert(item.id.clone(), item);
        self.updated_at = Utc::now();
    }

    /// Toggles the favorite status of a photo by ID.
    pub fn toggle_favorite(&mut self, item_id: &str) -> Option<bool> {
        if let Some(item) = self.items.get_mut(item_id) {
            item.is_favorite = !item.is_favorite;
            self.updated_at = Utc::now();
            Some(item.is_favorite)
        } else {
            None
        }
    }

    /// Creates a new virtual album.
    pub fn create_album(&mut self, name: String, description: Option<String>) -> String {
        let id = format!("album_{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
        let now = Utc::now();
        let album = Album {
            id: id.clone(),
            name,
            description,
            created_at: now,
            updated_at: now,
            cover_item_id: None,
            item_ids: Vec::new(),
        };
        self.albums.insert(id.clone(), album);
        self.updated_at = Utc::now();
        id
    }

    /// Adds media items to a virtual album.
    pub fn add_to_album(&mut self, album_id: &str, item_ids: &[String]) -> bool {
        if let Some(album) = self.albums.get_mut(album_id) {
            for item_id in item_ids {
                if !album.item_ids.contains(item_id) {
                    album.item_ids.push(item_id.clone());
                }
                if let Some(item) = self.items.get_mut(item_id) {
                    if !item.album_ids.contains(&album_id.to_string()) {
                        item.album_ids.push(album_id.to_string());
                    }
                }
            }
            if album.cover_item_id.is_none() && !album.item_ids.is_empty() {
                album.cover_item_id = Some(album.item_ids[0].clone());
            }
            album.updated_at = Utc::now();
            self.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    /// Generates chronological timeline groups sorted from newest to oldest.
    pub fn get_timeline(&self) -> Vec<TimelineGroup> {
        let mut grouped: BTreeMap<(i32, u32), Vec<MediaItem>> = BTreeMap::new();

        for item in self.items.values() {
            let year = item.date_taken.year();
            let month = item.date_taken.month();
            grouped.entry((year, month)).or_default().push(item.clone());
        }

        let mut timeline: Vec<TimelineGroup> = grouped
            .into_iter()
            .map(|((year, month), mut items)| {
                // Sort items within each month newest first
                items.sort_by(|a, b| b.date_taken.cmp(&a.date_taken));
                let group_key = format!("{:04}-{:02}", year, month);
                let month_name = match month {
                    1 => "January",
                    2 => "February",
                    3 => "March",
                    4 => "April",
                    5 => "May",
                    6 => "June",
                    7 => "July",
                    8 => "August",
                    9 => "September",
                    10 => "October",
                    11 => "November",
                    12 => "December",
                    _ => "Unknown",
                };
                let display_title = format!("{} {}", month_name, year);
                TimelineGroup {
                    group_key,
                    display_title,
                    year,
                    month,
                    items,
                }
            })
            .collect();

        // Newest months first
        timeline.reverse();
        timeline
    }

    /// Searches items based on structured filter parameters.
    pub fn search(&self, filter: &SearchFilter) -> Vec<MediaItem> {
        let mut matches = Vec::new();

        for item in self.items.values() {
            // Favorite check
            if filter.favorite_only && !item.is_favorite {
                continue;
            }

            // Media type check
            if let Some(mt) = filter.media_type {
                if item.media_type != mt {
                    continue;
                }
            }

            // Date range check
            if let Some(start) = filter.start_date {
                if item.date_taken < start {
                    continue;
                }
            }
            if let Some(end) = filter.end_date {
                if item.date_taken > end {
                    continue;
                }
            }

            // Album check
            if let Some(ref alb_id) = filter.album_id {
                if !item.album_ids.contains(alb_id) {
                    continue;
                }
            }

            // Camera model check
            if let Some(ref cam) = filter.camera_model {
                let m = item.exif.model.as_deref().unwrap_or_default();
                if !m.to_lowercase().contains(&cam.to_lowercase()) {
                    continue;
                }
            }

            // Geolocation proximity search
            if let (Some(center), Some(max_km)) = (&filter.geo_center, filter.geo_radius_km) {
                if let Some(ref item_loc) = item.exif.location {
                    let dist = center.haversine_distance_km(item_loc);
                    if dist > max_km {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Query text (filename, tags, camera)
            if let Some(ref q) = filter.query {
                let q_lower = q.to_lowercase();
                let name_match = item.file_name.to_lowercase().contains(&q_lower);
                let tag_match = item
                    .tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(&q_lower));
                let make_match = item
                    .exif
                    .make
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&q_lower);
                let model_match = item
                    .exif
                    .model
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&q_lower);

                if !name_match && !tag_match && !make_match && !model_match {
                    continue;
                }
            }

            matches.push(item.clone());
        }

        // Sort results newest first
        matches.sort_by(|a, b| b.date_taken.cmp(&a.date_taken));
        matches
    }

    /// Computes summary statistics across the whole catalog.
    pub fn compute_stats(&self) -> LibraryStats {
        let mut stats = LibraryStats {
            total_items: self.items.len(),
            albums_count: self.albums.len(),
            ..Default::default()
        };

        for item in self.items.values() {
            stats.total_bytes += item.size_bytes;
            match item.media_type {
                crate::models::MediaType::Photo => stats.photo_count += 1,
                crate::models::MediaType::Video => stats.video_count += 1,
                _ => {}
            }
            if item.is_favorite {
                stats.favorites_count += 1;
            }
            if stats.oldest_date.is_none() || Some(item.date_taken) < stats.oldest_date {
                stats.oldest_date = Some(item.date_taken);
            }
            if stats.newest_date.is_none() || Some(item.date_taken) > stats.newest_date {
                stats.newest_date = Some(item.date_taken);
            }
        }

        stats
    }

    /// Merges remote catalog changes into this local catalog.
    pub fn merge(&mut self, remote: PhotoCatalog) {
        for (id, remote_item) in remote.items {
            if let Some(local_item) = self.items.get_mut(&id) {
                // Keep local favorites and tags if modified later, otherwise sync
                if remote_item.modified_at > local_item.modified_at {
                    *local_item = remote_item;
                }
            } else {
                self.items.insert(id, remote_item);
            }
        }
        for (id, remote_album) in remote.albums {
            if let Some(local_album) = self.albums.get_mut(&id) {
                if remote_album.updated_at > local_album.updated_at {
                    *local_album = remote_album;
                }
            } else {
                self.albums.insert(id, remote_album);
            }
        }
        self.rebuild_indices();
        self.updated_at = Utc::now();
    }

    /// Loads the catalog from a JSON file.
    pub fn load_from_file(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut catalog: Self = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        catalog.rebuild_indices();
        Ok(catalog)
    }

    /// Saves the catalog to a JSON file atomically.
    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let temp_path = path.with_extension("tmp");
        std::fs::write(&temp_path, serialized.as_bytes())?;
        std::fs::rename(temp_path, path)?;
        Ok(())
    }
}

/// Abstract persistence store for [`PhotoCatalog`] instances (SRP & DIP).
pub trait PhotoCatalogStore: Send + Sync {
    /// Loads a `PhotoCatalog` from the given path.
    fn load(&self, path: &Path) -> Result<PhotoCatalog, crate::models::PhotosError>;

    /// Atomically persists a `PhotoCatalog` to the given path.
    fn save(&self, path: &Path, catalog: &PhotoCatalog) -> Result<(), crate::models::PhotosError>;
}

/// Default JSON file-backed implementation of [`PhotoCatalogStore`].
#[derive(Debug, Clone, Default)]
pub struct JsonPhotoCatalogStore;

impl PhotoCatalogStore for JsonPhotoCatalogStore {
    fn load(&self, path: &Path) -> Result<PhotoCatalog, crate::models::PhotosError> {
        PhotoCatalog::load_from_file(path).map_err(crate::models::PhotosError::Io)
    }

    fn save(&self, path: &Path, catalog: &PhotoCatalog) -> Result<(), crate::models::PhotosError> {
        catalog
            .save_to_file(path)
            .map_err(crate::models::PhotosError::Io)
    }
}

/// In-memory implementation of [`PhotoCatalogStore`] for isolated, disk-free unit testing (DIP).
#[derive(Debug, Default)]
pub struct InMemoryPhotoCatalogStore {
    inner: std::sync::RwLock<Option<PhotoCatalog>>,
}

impl InMemoryPhotoCatalogStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PhotoCatalogStore for InMemoryPhotoCatalogStore {
    fn load(&self, _path: &Path) -> Result<PhotoCatalog, crate::models::PhotosError> {
        let guard = self
            .inner
            .read()
            .map_err(|e| crate::models::PhotosError::Config(e.to_string()))?;
        Ok(guard.clone().unwrap_or_default())
    }

    fn save(&self, _path: &Path, catalog: &PhotoCatalog) -> Result<(), crate::models::PhotosError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|e| crate::models::PhotosError::Config(e.to_string()))?;
        *guard = Some(catalog.clone());
        Ok(())
    }
}
