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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ExifMetadata, GeoLocation, MediaType, SearchFilter};
    use chrono::{Duration, TimeZone, Utc};
    use tempfile::NamedTempFile;

    fn sample_item(id: &str, name: &str, hash: &str, days_ago: i64) -> MediaItem {
        let dt = Utc::now() - Duration::days(days_ago);
        MediaItem {
            id: id.to_string(),
            file_name: name.to_string(),
            local_path: format!("/photos/{}", name),
            remote_path: None,
            size_bytes: 2048,
            sha256_hash: hash.to_string(),
            media_type: MediaType::Photo,
            mime_type: "image/jpeg".to_string(),
            date_taken: dt,
            modified_at: dt,
            exif: ExifMetadata {
                make: Some("Sony".to_string()),
                model: Some("A7IV".to_string()),
                location: Some(GeoLocation::new(40.7128, -74.0060)), // NYC
                ..Default::default()
            },
            album_ids: Vec::new(),
            is_favorite: false,
            tags: vec!["vacation".to_string(), "travel".to_string()],
        }
    }

    #[test]
    fn test_catalog_crud_and_indexing() {
        let mut cat = PhotoCatalog::default();
        assert_eq!(cat.version, CATALOG_VERSION);
        assert!(cat.items.is_empty());

        let item1 = sample_item("item-1", "sunset.jpg", "hash111", 5);
        let item2 = sample_item("item-2", "beach.jpg", "hash222", 10);

        cat.insert_item(item1);
        cat.insert_item(item2);

        assert!(cat.contains_hash("hash111"));
        assert!(cat.contains_hash("hash222"));
        assert!(!cat.contains_hash("hash333"));

        assert_eq!(cat.get_by_hash("hash111").unwrap().file_name, "sunset.jpg");

        // Toggle favorite
        assert_eq!(cat.toggle_favorite("item-1"), Some(true));
        assert!(cat.items.get("item-1").unwrap().is_favorite);
        assert_eq!(cat.toggle_favorite("item-1"), Some(false));
        assert!(!cat.items.get("item-1").unwrap().is_favorite);
        assert_eq!(cat.toggle_favorite("non-existent"), None);

        // Rebuild indices
        cat.rebuild_indices();
        assert_eq!(cat.hash_index.len(), 2);
    }

    #[test]
    fn test_catalog_albums_and_stats() {
        let mut cat = PhotoCatalog::default();
        let item1 = sample_item("item-1", "pic1.jpg", "hash1", 100);
        let mut item2 = sample_item("item-2", "vid1.mp4", "hash2", 50);
        item2.media_type = MediaType::Video;
        item2.is_favorite = true;

        cat.insert_item(item1);
        cat.insert_item(item2);

        let album_id = cat.create_album("Holidays".to_string(), Some("Summer 2024".to_string()));
        assert!(cat.albums.contains_key(&album_id));

        let added = cat.add_to_album(&album_id, &["item-1".to_string()]);
        assert!(added);
        assert_eq!(cat.albums.get(&album_id).unwrap().cover_item_id, Some("item-1".to_string()));
        assert!(cat.items.get("item-1").unwrap().album_ids.contains(&album_id));

        // Non-existent album
        assert!(!cat.add_to_album("fake_album", &["item-1".to_string()]));

        // Stats
        let stats = cat.compute_stats();
        assert_eq!(stats.total_items, 2);
        assert_eq!(stats.photo_count, 1);
        assert_eq!(stats.video_count, 1);
        assert_eq!(stats.favorites_count, 1);
        assert_eq!(stats.albums_count, 1);
        assert_eq!(stats.total_bytes, 4096);
    }

    #[test]
    fn test_catalog_timeline_and_search() {
        let mut cat = PhotoCatalog::default();
        let dt1 = Utc.with_ymd_and_hms(2025, 6, 15, 10, 0, 0).unwrap();
        let dt2 = Utc.with_ymd_and_hms(2025, 7, 20, 15, 0, 0).unwrap();

        let mut item1 = sample_item("item-1", "june.jpg", "h1", 0);
        item1.date_taken = dt1;
        let mut item2 = sample_item("item-2", "july.jpg", "h2", 0);
        item2.date_taken = dt2;
        item2.is_favorite = true;

        cat.insert_item(item1);
        cat.insert_item(item2);

        let timeline = cat.get_timeline();
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0].group_key, "2025-07");
        assert_eq!(timeline[0].display_title, "July 2025");
        assert_eq!(timeline[1].group_key, "2025-06");
        assert_eq!(timeline[1].display_title, "June 2025");

        // Filter search: favorite only
        let filter_fav = SearchFilter {
            favorite_only: true,
            ..Default::default()
        };
        let res = cat.search(&filter_fav);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "item-2");

        // Filter search: date range
        let filter_date = SearchFilter {
            start_date: Some(Utc.with_ymd_and_hms(2025, 7, 1, 0, 0, 0).unwrap()),
            ..Default::default()
        };
        let res_date = cat.search(&filter_date);
        assert_eq!(res_date.len(), 1);
        assert_eq!(res_date[0].id, "item-2");

        // Filter search: geo proximity (NYC center, 10km radius)
        let filter_geo = SearchFilter {
            geo_center: Some(GeoLocation::new(40.7128, -74.0060)),
            geo_radius_km: Some(10.0),
            ..Default::default()
        };
        assert_eq!(cat.search(&filter_geo).len(), 2);

        // Geo far away (Tokyo)
        let filter_geo_far = SearchFilter {
            geo_center: Some(GeoLocation::new(35.6762, 139.6503)),
            geo_radius_km: Some(50.0),
            ..Default::default()
        };
        assert!(cat.search(&filter_geo_far).is_empty());

        // Text query search
        let filter_query = SearchFilter {
            query: Some("june".to_string()),
            ..Default::default()
        };
        assert_eq!(cat.search(&filter_query).len(), 1);
    }

    #[test]
    fn test_catalog_merge_and_stores() {
        let mut local = PhotoCatalog::default();
        let item1 = sample_item("item-1", "first.jpg", "h1", 10);
        local.insert_item(item1);

        let mut remote = PhotoCatalog::default();
        let mut item1_updated = sample_item("item-1", "first.jpg", "h1", 5);
        item1_updated.is_favorite = true;
        let item2 = sample_item("item-2", "second.jpg", "h2", 2);
        remote.insert_item(item1_updated);
        remote.insert_item(item2);

        local.merge(remote);
        assert_eq!(local.items.len(), 2);
        assert!(local.items.get("item-1").unwrap().is_favorite);

        // Disk roundtrip
        let tmp = NamedTempFile::new().unwrap();
        let json_store = JsonPhotoCatalogStore::default();
        json_store.save(tmp.path(), &local).unwrap();

        let loaded = json_store.load(tmp.path()).unwrap();
        assert_eq!(loaded.items.len(), 2);

        // In-memory store
        let mem_store = InMemoryPhotoCatalogStore::new();
        mem_store.save(Path::new("dummy"), &local).unwrap();
        let mem_loaded = mem_store.load(Path::new("dummy")).unwrap();
        assert_eq!(mem_loaded.items.len(), 2);
    }
}
