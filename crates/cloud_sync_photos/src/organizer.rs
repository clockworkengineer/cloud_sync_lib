//! Cloud storage path strategies for chronological photo organization.

use crate::models::MediaItem;
use chrono::Datelike;
use serde::{Deserialize, Serialize};

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

/// Generates the destination remote relative path for a media item.
pub fn generate_remote_path(item: &MediaItem, strategy: OrganizeStrategy) -> String {
    let year = item.date_taken.year();
    let month = item.date_taken.month();
    let day = item.date_taken.day();

    match strategy {
        OrganizeStrategy::YearMonth => {
            format!("{:04}/{:02}/{}", year, month, item.file_name)
        }
        OrganizeStrategy::YearMonthDay => {
            format!("{:04}/{:02}/{:02}/{}", year, month, day, item.file_name)
        }
        OrganizeStrategy::ChronologicalHash => {
            let short_hash = if item.sha256_hash.len() >= 8 {
                &item.sha256_hash[..8]
            } else {
                &item.sha256_hash
            };
            format!("{:04}/{:02}/{}_{}", year, month, short_hash, item.file_name)
        }
        OrganizeStrategy::Flat => item.file_name.clone(),
    }
}
