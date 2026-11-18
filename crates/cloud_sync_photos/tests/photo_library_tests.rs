use chrono::{TimeZone, Utc};
use cloud_sync_photos::engine::PhotoLibrary;
use cloud_sync_photos::metadata::{detect_media_format, inspect_media_file, parse_exif_datetime};
use cloud_sync_photos::models::{GeoLocation, MediaType, SearchFilter};
use cloud_sync_photos::organizer::{generate_remote_path, OrganizeStrategy};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_media_detection_and_exif_parsing() {
    let (m_type, mime) = detect_media_format(std::path::Path::new("photo.jpg"));
    assert_eq!(m_type, MediaType::Photo);
    assert_eq!(mime, "image/jpeg");

    let (v_type, v_mime) = detect_media_format(std::path::Path::new("video.mp4"));
    assert_eq!(v_type, MediaType::Video);
    assert_eq!(v_mime, "video/mp4");

    let dt = parse_exif_datetime("2024:08:15 14:30:00");
    assert!(dt.is_some());
    let unwrapped = dt.unwrap();
    assert_eq!(
        unwrapped,
        Utc.with_ymd_and_hms(2024, 8, 15, 14, 30, 0).unwrap()
    );
}

#[test]
fn test_haversine_distance() {
    // London
    let london = GeoLocation::new(51.5074, -0.1278);
    // Paris
    let paris = GeoLocation::new(48.8566, 2.3522);

    let distance = london.haversine_distance_km(&paris);
    // Approximately 343 km
    assert!(
        distance > 330.0 && distance < 360.0,
        "Distance was {}",
        distance
    );
}

#[test]
fn test_path_organizer_strategies() {
    let dir = tempdir().unwrap();
    let sample = dir.path().join("IMG_1234.jpg");
    std::fs::write(&sample, b"dummy photo content").unwrap();

    let mut item = inspect_media_file(&sample).unwrap();
    item.date_taken = Utc.with_ymd_and_hms(2024, 11, 18, 10, 0, 0).unwrap();

    let path_ym = generate_remote_path(&item, OrganizeStrategy::YearMonth);
    assert_eq!(path_ym, "2024/11/IMG_1234.jpg");

    let path_ymd = generate_remote_path(&item, OrganizeStrategy::YearMonthDay);
    assert_eq!(path_ymd, "2024/11/18/IMG_1234.jpg");

    let path_flat = generate_remote_path(&item, OrganizeStrategy::Flat);
    assert_eq!(path_flat, "IMG_1234.jpg");
}

#[tokio::test]
async fn test_photo_library_sync_deduplication_and_albums() {
    let dir = tempdir().unwrap();
    let local_photos = dir.path().join("dcim");
    std::fs::create_dir_all(&local_photos).unwrap();

    // Create 3 test files (two identical content, one distinct)
    let photo1 = local_photos.join("vacation_1.jpg");
    let photo2 = local_photos.join("vacation_1_duplicate.jpg");
    let photo3 = local_photos.join("nature.png");

    std::fs::write(&photo1, b"EXACT_SAME_BYTES_12345").unwrap();
    std::fs::write(&photo2, b"EXACT_SAME_BYTES_12345").unwrap(); // Duplicate!
    std::fs::write(&photo3, b"DIFFERENT_PICTURE_BYTES_9999").unwrap();

    // Set up a mock/local cloud backend using cloud_sync_lib
    let cloud_root = dir.path().join("cloud_storage");
    std::fs::create_dir_all(&cloud_root).unwrap();

    let backend = Arc::new(cloud_sync_lib::LocalSimulation::new(
        cloud_root,
        "local".to_string(),
    ));
    let catalog_file = dir.path().join("photos_catalog.json");

    let library = PhotoLibrary::new(
        backend,
        "GooglePhotos",
        catalog_file.clone(),
        OrganizeStrategy::YearMonth,
    )
    .await
    .unwrap();

    // First sync pass
    let res = library.sync_folder(&local_photos).await.unwrap();
    assert_eq!(res.files_scanned, 3);
    assert_eq!(
        res.files_imported, 2,
        "Only 2 unique files should be imported"
    );
    assert_eq!(
        res.files_skipped_duplicates, 1,
        "Duplicate should be skipped"
    );

    // Second sync pass: no new files should be uploaded
    let res2 = library.sync_folder(&local_photos).await.unwrap();
    assert_eq!(res2.files_imported, 0);
    assert_eq!(res2.files_skipped_duplicates, 3);

    // Verify timeline
    let timeline = library.get_timeline().await;
    assert!(!timeline.is_empty());
    assert_eq!(timeline[0].items.len(), 2);

    // Create virtual album and add photos
    let album_id = library
        .create_album("Summer Trip".into(), Some("Trip to the lake".into()))
        .await;
    let item_ids: Vec<String> = timeline[0].items.iter().map(|i| i.id.clone()).collect();
    let added = library.add_to_album(&album_id, &item_ids).await;
    assert!(added);

    // Toggle favorite
    let fav_status = library.toggle_favorite(&item_ids[0]).await;
    assert_eq!(fav_status, Some(true));

    // Search for favorites
    let favs = library
        .search(&SearchFilter {
            favorite_only: true,
            ..Default::default()
        })
        .await;
    assert_eq!(favs.len(), 1);

    // Check stats
    let stats = library.get_stats().await;
    assert_eq!(stats.total_items, 2);
    assert_eq!(stats.favorites_count, 1);
    assert_eq!(stats.albums_count, 1);
}
