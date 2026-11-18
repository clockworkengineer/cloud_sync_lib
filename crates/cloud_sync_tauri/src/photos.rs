//! Tauri IPC commands for Google Photos equivalence (Android & Desktop).

use chrono::{NaiveDate, TimeZone, Utc};
use cloud_sync_photos::config::instantiate_backend;
use cloud_sync_photos::models::SearchFilter;
use cloud_sync_photos::{
    Album, LibraryStats, MediaItem, PhotoLibrary, PhotoSyncResult, PhotosConfig, TimelineGroup,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Global cached photo library reference.
static PHOTO_LIBRARY: Mutex<Option<Arc<PhotoLibrary>>> = Mutex::const_new(None);

/// Resolves the storage path for photos configuration.
pub fn get_photos_config_path() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        let base = PathBuf::from("/data/user/0/com.cloudsync.app/files");
        if base.exists() {
            return base.join("photos_config.toml");
        }
    }
    PathBuf::from("photos_config.toml")
}

/// Resolves the storage path for the local photo catalog index.
pub fn get_photos_catalog_path() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        let base = PathBuf::from("/data/user/0/com.cloudsync.app/files");
        if base.exists() {
            return base.join("photos_catalog.json");
        }
    }
    PathBuf::from("photos_catalog.json")
}

/// Helper to get or initialize the active PhotoLibrary.
async fn get_or_init_photo_library(config: &PhotosConfig) -> Result<Arc<PhotoLibrary>, String> {
    let mut lock = PHOTO_LIBRARY.lock().await;
    if let Some(ref lib) = *lock {
        return Ok(lib.clone());
    }

    let backend =
        instantiate_backend(&config.destination_provider, &config.provider_settings).await?;
    let catalog_path = get_photos_catalog_path();
    let library = Arc::new(
        PhotoLibrary::new(
            backend,
            &config.destination_folder,
            catalog_path,
            config.strategy,
        )
        .await?,
    );

    *lock = Some(library.clone());
    Ok(library)
}

#[tauri::command]
pub async fn photos_get_config() -> Result<PhotosConfig, String> {
    let path = get_photos_config_path();
    if path.exists() {
        PhotosConfig::load_from_file(&path)
    } else {
        let def = PhotosConfig::default();
        let _ = def.save_to_file(&path);
        Ok(def)
    }
}

#[tauri::command]
pub async fn photos_save_config(config: PhotosConfig) -> Result<serde_json::Value, String> {
    let path = get_photos_config_path();
    config.save_to_file(&path)?;

    // Reset cached library so new settings take effect
    let mut lock = PHOTO_LIBRARY.lock().await;
    *lock = None;

    Ok(serde_json::json!({ "status": "saved", "config": config }))
}

#[tauri::command]
pub async fn photos_sync() -> Result<PhotoSyncResult, String> {
    let config = photos_get_config().await?;
    let library = get_or_init_photo_library(&config).await?;

    let source = PathBuf::from(&config.source_folder);
    if !source.exists() {
        return Err(format!(
            "Source photos folder does not exist: {}. On Android, verify DCIM access or camera permissions.",
            config.source_folder
        ));
    }

    library.sync_folder(&source).await
}

#[tauri::command]
pub async fn photos_get_timeline() -> Result<Vec<TimelineGroup>, String> {
    let config = photos_get_config().await?;
    let library = get_or_init_photo_library(&config).await?;
    Ok(library.get_timeline().await)
}

#[tauri::command]
pub async fn photos_get_albums() -> Result<Vec<Album>, String> {
    let config = photos_get_config().await?;
    let library = get_or_init_photo_library(&config).await?;
    let cat = library.catalog();
    let cat_lock = cat.read().await;
    let albums: Vec<Album> = cat_lock.albums.values().cloned().collect();
    Ok(albums)
}

#[tauri::command]
pub async fn photos_create_album(
    name: String,
    description: Option<String>,
) -> Result<String, String> {
    let config = photos_get_config().await?;
    let library = get_or_init_photo_library(&config).await?;
    Ok(library.create_album(name, description).await)
}

#[tauri::command]
pub async fn photos_add_to_album(album_id: String, item_ids: Vec<String>) -> Result<bool, String> {
    let config = photos_get_config().await?;
    let library = get_or_init_photo_library(&config).await?;
    Ok(library.add_to_album(&album_id, &item_ids).await)
}

#[tauri::command]
pub async fn photos_toggle_favorite(item_id: String) -> Result<Option<bool>, String> {
    let config = photos_get_config().await?;
    let library = get_or_init_photo_library(&config).await?;
    Ok(library.toggle_favorite(&item_id).await)
}

#[tauri::command]
pub async fn photos_search(
    query: Option<String>,
    camera: Option<String>,
    favorite_only: bool,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<Vec<MediaItem>, String> {
    let config = photos_get_config().await?;
    let library = get_or_init_photo_library(&config).await?;

    let mut filter = SearchFilter {
        query,
        camera_model: camera,
        favorite_only,
        ..Default::default()
    };
    if let Some(ref s) = start_date {
        if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            filter.start_date = Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap()));
        }
    }
    if let Some(ref e) = end_date {
        if let Ok(d) = NaiveDate::parse_from_str(e, "%Y-%m-%d") {
            filter.end_date = Some(Utc.from_utc_datetime(&d.and_hms_opt(23, 59, 59).unwrap()));
        }
    }

    Ok(library.search(&filter).await)
}

#[tauri::command]
pub async fn photos_get_stats() -> Result<LibraryStats, String> {
    let config = photos_get_config().await?;
    let library = get_or_init_photo_library(&config).await?;
    Ok(library.get_stats().await)
}
