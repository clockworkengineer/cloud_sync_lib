//! Cloud storage path strategies for chronological photo organization.

use crate::models::MediaItem;
use chrono::Datelike;
use serde::{Deserialize, Serialize};

/// Pluggable path organization strategy trait for determining remote directory hierarchy (OCP).
pub trait PhotoPathOrganizer: Send + Sync {
    /// Generates the destination remote relative path for a media item.
    fn organize_path(&self, item: &MediaItem) -> String;
}

/// Strategy used to determine remote directory hierarchy in cloud storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrganizeStrategy {
    /// Organizes as `YYYY/MM/filename.ext` (e.g. `2024/11/IMG_001.jpg`).
    YearMonth,
    /// Organizes as `YYYY/MM/DD/filename.ext` (e.g. `2024/11/18/IMG_001.jpg`).
    YearMonthDay,
    /// Organizes as `YYYY/MM/<hash>_filename.ext`.
    ChronologicalHash,
    /// Keeps flat root: `filename.ext`.
    Flat,
}

impl Default for OrganizeStrategy {
    fn default() -> Self {
        Self::YearMonth
    }
}

impl PhotoPathOrganizer for OrganizeStrategy {
    fn organize_path(&self, item: &MediaItem) -> String {
        generate_remote_path(item, *self)
    }
}

/// Generates the destination remote relative path for a media item.
pub fn generate_remote_path(item: &MediaItem, strategy: OrganizeStrategy) -> String {
    let year = item.date_taken.year();
    let month = item.date_taken.month();
    let day = item.date_taken.day();

    // Sanitize item.file_name against path traversal, leading slashes, and illegal chars
    let raw_name = std::path::Path::new(&item.file_name)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "image.jpg".to_string());
    let norm = cloud_sync_core::NormalizedPath::new(&raw_name)
        .unwrap_or_else(|_| cloud_sync_core::NormalizedPath::empty().join(&raw_name));
    let clean_name = if norm.is_empty() { "image.jpg" } else { norm.as_str() };

    match strategy {
        OrganizeStrategy::YearMonth => {
            format!("{:04}/{:02}/{}", year, month, clean_name)
        }
        OrganizeStrategy::YearMonthDay => {
            format!("{:04}/{:02}/{:02}/{}", year, month, day, clean_name)
        }
        OrganizeStrategy::ChronologicalHash => {
            let short_hash = if item.sha256_hash.len() >= 8 {
                &item.sha256_hash[..8]
            } else {
                &item.sha256_hash
            };
            format!("{:04}/{:02}/{}_{}", year, month, short_hash, clean_name)
        }
        OrganizeStrategy::Flat => clean_name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ExifMetadata, MediaType};
    use chrono::{TimeZone, Utc};

    fn make_item(name: &str, hash: &str, year: i32, month: u32, day: u32) -> MediaItem {
        let dt = Utc.with_ymd_and_hms(year, month, day, 12, 0, 0).unwrap();
        MediaItem {
            id: hash.to_string(),
            file_name: name.to_string(),
            local_path: format!("/tmp/{}", name),
            remote_path: None,
            size_bytes: 1024,
            sha256_hash: hash.to_string(),
            media_type: MediaType::Photo,
            mime_type: "image/jpeg".to_string(),
            date_taken: dt,
            modified_at: dt,
            exif: ExifMetadata::default(),
            album_ids: Vec::new(),
            is_favorite: false,
            tags: Vec::new(),
        }
    }

    #[test]
    fn test_organize_strategies() {
        let item = make_item("photo.jpg", "abcdef0123456789", 2026, 11, 24);

        assert_eq!(
            OrganizeStrategy::YearMonth.organize_path(&item),
            "2026/11/photo.jpg"
        );
        assert_eq!(
            OrganizeStrategy::YearMonthDay.organize_path(&item),
            "2026/11/24/photo.jpg"
        );
        assert_eq!(
            OrganizeStrategy::ChronologicalHash.organize_path(&item),
            "2026/11/abcdef01_photo.jpg"
        );
        assert_eq!(
            OrganizeStrategy::Flat.organize_path(&item),
            "photo.jpg"
        );
        assert_eq!(OrganizeStrategy::default(), OrganizeStrategy::YearMonth);

        // Short hash fallback
        let short_hash_item = make_item("pic.png", "abc", 2025, 5, 1);
        assert_eq!(
            OrganizeStrategy::ChronologicalHash.organize_path(&short_hash_item),
            "2025/05/abc_pic.png"
        );
    }
}
