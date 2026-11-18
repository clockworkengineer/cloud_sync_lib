//! Configuration and backend instantiation for the Google Photos engine.

use crate::organizer::OrganizeStrategy;
use cloud_sync_core::StorageBackend;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Configuration options for the photos sync subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotosConfig {
    /// Local directory containing photos/videos to watch and backup.
    /// Default on Android: `/storage/emulated/0/DCIM/Camera`
    /// Default on Desktop: `~/Pictures`
    pub source_folder: String,

    /// Destination cloud provider identifier:
    /// `google_drive`, `onedrive`, `dropbox`, `s3`, `mega`, `box`, `webdav`, `local`, etc.
    pub destination_provider: String,

    /// Cloud root directory under which organized photos are stored.
    pub destination_folder: String,

    /// Folder hierarchy organization strategy.
    #[serde(default)]
    pub strategy: OrganizeStrategy,

    /// Whether automatic periodic background sync is enabled.
    #[serde(default = "default_auto_sync")]
    pub auto_sync: bool,

    /// Sync interval in minutes.
    pub sync_interval_mins: Option<u64>,

    /// Provider-specific settings and credentials.
    #[serde(default)]
    pub provider_settings: HashMap<String, String>,
}

fn default_auto_sync() -> bool {
    true
}

impl Default for PhotosConfig {
    fn default() -> Self {
        #[cfg(target_os = "android")]
        let default_source = "/storage/emulated/0/DCIM/Camera".to_string();

        #[cfg(not(target_os = "android"))]
        let default_source = dirs_or_fallback_pictures();

        Self {
            source_folder: default_source,
            destination_provider: "google_drive".to_string(),
            destination_folder: "GooglePhotos_Backup".to_string(),
            strategy: OrganizeStrategy::YearMonth,
            auto_sync: true,
            sync_interval_mins: Some(60),
            provider_settings: HashMap::new(),
        }
    }
}

#[allow(dead_code)]
fn dirs_or_fallback_pictures() -> String {
    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        let p = PathBuf::from(user_profile).join("Pictures");
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join("Pictures");
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
    }
    "./photos_library".to_string()
}

impl PhotosConfig {
    /// Loads photos configuration from a TOML file.
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str(&content).map_err(|e| e.to_string())
    }

    /// Saves photos configuration to a TOML file.
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, content).map_err(|e| e.to_string())
    }
}

/// Instantiates a StorageBackend from provider identifier and settings.
pub async fn instantiate_backend(
    provider: &str,
    settings: &HashMap<String, String>,
) -> Result<Arc<dyn StorageBackend>, String> {
    let json_val = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    let sim_root = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("cloud_simulation");

    cloud_sync_lib::DynamicBackendRegistry::global()
        .create_backend(provider, &json_val, sim_root)
        .map_err(|e| e.to_string())
}
