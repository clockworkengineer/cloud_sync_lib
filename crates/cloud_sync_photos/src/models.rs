//! Data models for the Google Photos equivalence engine.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Structured domain error for `cloud_sync_photos` operations (Attribute 1: Intuitive API Design).
#[derive(Debug)]
pub enum PhotosError {
    Io(std::io::Error),
    Storage(cloud_sync_core::StorageError),
    Config(String),
    InvalidPath(String),
    NotFound(String),
}

impl std::fmt::Display for PhotosError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhotosError::Io(e) => write!(f, "Photos I/O error: {}", e),
            PhotosError::Storage(e) => write!(f, "Photos cloud storage error: {}", e),
            PhotosError::Config(msg) => write!(f, "Photos configuration error: {}", msg),
            PhotosError::InvalidPath(msg) => write!(f, "Invalid media path: {}", msg),
            PhotosError::NotFound(msg) => write!(f, "Media item not found: {}", msg),
        }
    }
}

impl std::error::Error for PhotosError {}

impl From<std::io::Error> for PhotosError {
    fn from(e: std::io::Error) -> Self {
        PhotosError::Io(e)
    }
}

impl From<cloud_sync_core::StorageError> for PhotosError {
    fn from(e: cloud_sync_core::StorageError) -> Self {
        PhotosError::Storage(e)
    }
}

impl From<String> for PhotosError {
    fn from(s: String) -> Self {
        PhotosError::Config(s)
    }
}

impl From<PhotosError> for String {
    fn from(e: PhotosError) -> Self {
        e.to_string()
    }
}

/// Type of media item (photo or video).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaType {
    Photo,
    Video,
    Unknown,
}

/// Image formats supported for extraction and indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFormat {
    Jpeg,
    Png,
    WebP,
    Heic,
    Tiff,
    Gif,
    Raw,
    Other,
}

/// Video formats supported for extraction and indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoFormat {
    Mp4,
    Mov,
    Mkv,
    Webm,
    Avi,
    Other,
}

/// Geographical GPS coordinates with altitude and distance calculation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_meters: Option<f64>,
}

impl GeoLocation {
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
            altitude_meters: None,
        }
    }

    /// Computes the great-circle distance between two GPS coordinates using the Haversine formula (km).
    pub fn haversine_distance_km(&self, other: &GeoLocation) -> f64 {
        let r = 6371.0; // Earth radius in km
        let d_lat = (other.latitude - self.latitude).to_radians();
        let d_lon = (other.longitude - self.longitude).to_radians();
        let lat1 = self.latitude.to_radians();
        let lat2 = other.latitude.to_radians();

        let a = (d_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();

        r * c
    }
}

/// Extracted EXIF and camera metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExifMetadata {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens_model: Option<String>,
    pub iso: Option<u32>,
    pub f_number: Option<f32>,
    pub exposure_time: Option<String>,
    pub focal_length_mm: Option<f32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub location: Option<GeoLocation>,
    pub date_taken_str: Option<String>,
}

/// A media file indexed in the photo library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    /// Unique identifier (usually SHA-256 hash or UUID).
    pub id: String,
    /// Original file name on local storage.
    pub file_name: String,
    /// Relative or absolute path where the file is stored locally.
    pub local_path: String,
    /// Cloud destination path (e.g., `2024/11/IMG_20241118_120000.jpg`).
    pub remote_path: Option<String>,
    /// File size in bytes.
    pub size_bytes: u64,
    /// SHA-256 content checksum for deduplication.
    pub sha256_hash: String,
    /// Media classification (Photo or Video).
    pub media_type: MediaType,
    /// MIME type string (e.g. `image/jpeg`).
    pub mime_type: String,
    /// Primary chronological timestamp (EXIF Date Taken or fallback to file modified).
    pub date_taken: DateTime<Utc>,
    /// File system modification timestamp.
    pub modified_at: DateTime<Utc>,
    /// Extracted EXIF camera metadata.
    pub exif: ExifMetadata,
    /// Virtual albums this media item belongs to.
    pub album_ids: Vec<String>,
    /// Whether the user marked this photo as favorite.
    pub is_favorite: bool,
    /// User or AI-generated tags (e.g. "nature", "vacation").
    pub tags: Vec<String>,
}

/// Virtual album containing a collection of media items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub cover_item_id: Option<String>,
    pub item_ids: Vec<String>,
}

/// Group of media items for chronological timeline rendering (e.g., "November 2024").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineGroup {
    pub group_key: String,
    pub display_title: String,
    pub year: i32,
    pub month: u32,
    pub items: Vec<MediaItem>,
}

/// Search and filter criteria for finding photos in the library.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilter {
    pub query: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub favorite_only: bool,
    pub camera_model: Option<String>,
    pub media_type: Option<MediaType>,
    pub geo_center: Option<GeoLocation>,
    pub geo_radius_km: Option<f64>,
    pub album_id: Option<String>,
}

/// Overall photo library statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryStats {
    pub total_items: usize,
    pub photo_count: usize,
    pub video_count: usize,
    pub total_bytes: u64,
    pub oldest_date: Option<DateTime<Utc>>,
    pub newest_date: Option<DateTime<Utc>>,
    pub albums_count: usize,
    pub favorites_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_photos_error_display_and_conversions() {
        let io_err = PhotosError::from(std::io::Error::other("disk error"));
        assert!(io_err.to_string().contains("Photos I/O error"));

        let storage_err =
            PhotosError::from(cloud_sync_core::StorageError::NotFound("lost".to_string()));
        assert!(storage_err
            .to_string()
            .contains("Photos cloud storage error"));

        let config_err = PhotosError::from("config syntax".to_string());
        assert!(config_err
            .to_string()
            .contains("Photos configuration error"));

        let path_err = PhotosError::InvalidPath("bad/path".to_string());
        assert!(path_err.to_string().contains("Invalid media path"));

        let not_found_err = PhotosError::NotFound("img123".to_string());
        assert!(not_found_err.to_string().contains("Media item not found"));

        let str_conv: String = not_found_err.into();
        assert!(str_conv.contains("img123"));
    }

    #[test]
    fn test_geolocation_and_enums_serde() {
        let loc1 = GeoLocation::new(48.8566, 2.3522); // Paris
        let loc2 = GeoLocation::new(51.5074, -0.1278); // London
        let dist = loc1.haversine_distance_km(&loc2);
        assert!(dist > 300.0 && dist < 400.0);

        let photo_type = MediaType::Photo;
        let json = serde_json::to_string(&photo_type).unwrap();
        assert_eq!(
            serde_json::from_str::<MediaType>(&json).unwrap(),
            MediaType::Photo
        );

        let img_fmt = ImageFormat::Heic;
        let json_fmt = serde_json::to_string(&img_fmt).unwrap();
        assert_eq!(
            serde_json::from_str::<ImageFormat>(&json_fmt).unwrap(),
            ImageFormat::Heic
        );

        let vid_fmt = VideoFormat::Mp4;
        let json_vid = serde_json::to_string(&vid_fmt).unwrap();
        assert_eq!(
            serde_json::from_str::<VideoFormat>(&json_vid).unwrap(),
            VideoFormat::Mp4
        );
    }
}
