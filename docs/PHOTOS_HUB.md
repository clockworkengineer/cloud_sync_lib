# Google Photos Equivalence Engine & Media Management Hub (`cloud_sync_photos`)

The `cloud_sync_photos` crate provides a high-performance, vendor-neutral media management, chronological timeline organization, metadata extraction, deduplication, and multi-cloud backup engine. It functions as a self-hosted alternative to Google Photos, Apple iCloud Photos, and Amazon Photos, operating seamlessly over any of the 17 storage providers supported by `cloud_sync_lib`.

---

## 1. Architecture & Design Principles

```mermaid
graph TD
    subgraph Media_Sources [Input Sources]
        local_disk[Local Folders / DCIM / External Drives]
        android_roll[Android Camera Roll / Storage Access Framework]
    end

    subgraph cloud_sync_photos [cloud_sync_photos Core Engine]
        scanner[Media Scanner & File Filter]
        exif_engine[Pure-Rust EXIF & Metadata Parser]
        dedupe_calc[Content-Addressable SHA-256 Hasher]
        organizer[Chronological Path Formatter]
        catalog_mgr[Catalog & Index Manager (.photos_catalog.json)]
        search_engine[Multi-Criteria & Haversine GPS Search Engine]
    end

    subgraph Storage_Layer [Storage & Remote Backends]
        local_catalog[(sync_state/photos_catalog.json)]
        remote_backend[Any StorageBackend: GDrive, S3, Nextcloud, Union, etc.]
    end

    local_disk --> scanner
    android_roll --> scanner
    scanner --> exif_engine
    exif_engine --> dedupe_calc
    dedupe_calc --> organizer
    organizer --> catalog_mgr
    catalog_mgr <--> local_catalog
    catalog_mgr <--> remote_backend
    search_engine --> catalog_mgr
```

### Key Technical Pillars
1. **Pure-Rust EXIF & Metadata Extraction**: Eliminates dependency on external C libraries (such as `libexif` or ImageMagick), ensuring 100% safe compilation for cross-platform desktop and Android mobile targets.
2. **Content-Addressable SHA-256 Deduplication**: Generates a cryptographic digest before transferring files. Duplicate files located across different directories or camera dumps are recognized and skipped immediately without network overhead.
3. **Cross-Platform Catalog Synchronization**: The engine maintains a serialized `.photos_catalog.json` index. When synchronizing, the catalog merges remote and local metadata so albums, favorites, and timeline groupings remain unified across devices.
4. **Offline Local Simulation Support**: Fully compatible with `LocalSimulation` backends for test suites and offline development.

---

## 2. Media Metadata & EXIF Extraction

The parser ([`metadata.rs`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_photos/src/metadata.rs)) extracts rich EXIF and filesystem properties without decoding entire pixel bitmaps:

### Extracted Attributes
- **Capture Timestamp (`DateTimeOriginal` / `CreateDate`)**: Accurately parsed into UTC `chrono::DateTime<Utc>`. Falls back gracefully to filesystem creation or modification time if EXIF tags are missing.
- **Camera Make & Model**: E.g., `Apple iPhone 15 Pro`, `Sony ILCE-7M4`, `Google Pixel 8`.
- **Lens Specification & Focal Length**: Captures hardware lens identifiers.
- **Exposure Parameters**: ISO sensitivity, F-number / aperture, exposure time / shutter speed.
- **Spatial Geometry**: Width and height dimensions in pixels, orientation flag (1–8 EXIF rotation tags).
- **Geographic Coordinates (`GeoLocation`)**: Latitude, longitude, and GPS altitude extracted from EXIF GPS tags.

### Supported File Formats
- **Still Photography**: JPEG, JPG, PNG, WebP, HEIC/HEIF, TIFF, DNG, RAW formats.
- **Video Media**: MP4, MOV, MKV, AVI, WebM.

---

## 3. Chronological Organization Strategies

When synchronizing photos from a source directory (e.g. SD card or DCIM folder) to cloud storage, the engine automatically formats remote destination paths according to a chosen `OrganizeStrategy` ([`organizer.rs`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_photos/src/organizer.rs)):

| Strategy | Destination Path Structure | Example Output |
| :--- | :--- | :--- |
| **`YearMonth` (Default)** | `<DEST>/YYYY/MM/<filename>` | `GooglePhotos_Backup/2026/11/IMG_4021.JPG` |
| **`YearMonthDay`** | `<DEST>/YYYY/MM/DD/<filename>` | `GooglePhotos_Backup/2026/11/24/IMG_4021.JPG` |
| **`CameraModel`** | `<DEST>/<CameraModel>/YYYY/<filename>` | `GooglePhotos_Backup/iPhone_15_Pro/2026/IMG_4021.JPG` |
| **`Flat`** | `<DEST>/<filename>` | `GooglePhotos_Backup/IMG_4021.JPG` |

---

## 4. Search & Geographic Filtering Engine

The search module ([`models.rs`](file:///C:/Projects/cloud_sync_lib/crates/cloud_sync_photos/src/models.rs)) powers instant multi-criteria queries over indexed catalogs:

### Available Query Filters
- **Date Ranges**: Filter items taken between `from` and `to` timestamps.
- **Media Type**: Restrict results to `MediaType::Photo` or `MediaType::Video`.
- **Favorites & Starred**: Retrieve curated favorites (`is_favorite: true`).
- **Camera Hardware**: Substring matching on camera make or model (e.g. `"Sony"`, `"Pixel"`).
- **Album Membership**: Filter by assigned album name (e.g. `"Summer 2026 Vacation"`).
- **Haversine Proximity Search**: Search within a geographic radius in kilometers from a given latitude/longitude coordinate:
  
  $$d = 2R \arcsin\left(\sqrt{\sin^2\left(\frac{\Delta\phi}{2}\right) + \cos(\phi_1)\cos(\phi_2)\sin^2\left(\frac{\Delta\lambda}{2}\right)}\right)$$

---

## 5. Standalone CLI Reference

The workspace includes the standalone `cloud_sync_photos` executable binary.

```bash
cargo run --bin cloud_sync_photos -- [SUBCOMMAND] [OPTIONS]
```

### 5.1 Common Global Flags
- `--config <PATH>`: Custom path to `photos_config.toml` (default: `photos_config.toml`).
- `--catalog <PATH>`: Custom path to local catalog JSON (default: `sync_state/photos_catalog.json`).

### 5.2 Subcommands

#### `sync` — Ingest & Back Up Photos
Scans a local directory, extracts metadata, checks deduplication digests, organizes files into the target hierarchy, and uploads them to the configured cloud backend.
```bash
# Sync default source folder from photos_config.toml
cargo run --bin cloud_sync_photos -- sync

# Sync specific camera card or DCIM folder
cargo run --bin cloud_sync_photos -- sync --source "D:\DCIM\100EOSR5"
```

#### `timeline` — Display Chronological Feed
Renders an interactive chronological grouping of your photo library in the terminal, aggregated by Year and Month.
```bash
cargo run --bin cloud_sync_photos -- timeline
```

#### `list` — Enumerate Indexed Media
Lists indexed photos with optional sorting and filtering flags:
```bash
# List all videos
cargo run --bin cloud_sync_photos -- list --media-type video

# List only favorites
cargo run --bin cloud_sync_photos -- list --favorites
```

#### `search` — Multi-Parameter Filtering
```bash
# Search photos taken in a specific date range with a specific camera
cargo run --bin cloud_sync_photos -- search --from 2026-06-01 --to 2026-08-31 --camera "Pixel 8"

# Search photos taken within 25 km of London (Lat: 51.5074, Lon: -0.1278)
cargo run --bin cloud_sync_photos -- search --near-lat 51.5074 --near-lon -0.1278 --radius-km 25.0
```

#### `albums` — Manage Virtual Albums
Create albums, assign photos, and list album contents without duplicating files on the underlying storage.
```bash
# Create a new album
cargo run --bin cloud_sync_photos -- albums create "Family Vacation 2026"

# Add a photo to an album
cargo run --bin cloud_sync_photos -- albums add "Family Vacation 2026" "sha256_hash_of_photo"

# List all albums
cargo run --bin cloud_sync_photos -- albums list
```

---

## 6. Programmatic Rust API Example

```rust,no_run
use cloud_sync_lib::LocalSimulation;
use cloud_sync_photos::{OrganizeStrategy, PhotoLibrary, SearchFilter, GeoLocation};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize any StorageBackend from cloud_sync_lib
    let backend = Arc::new(LocalSimulation::new(
        PathBuf::from("./cloud_simulation/photos"),
        "PhotosStorage".into(),
    ));

    // 2. Instantiate PhotoLibrary
    let library = PhotoLibrary::new(
        backend,
        "GooglePhotos_Backup",
        PathBuf::from("./sync_state/photos_catalog.json"),
        OrganizeStrategy::YearMonth,
    ).await?;

    // 3. Ingest and sync a folder of new photos
    let report = library.sync_folder(Path::new("./DCIM/Camera")).await?;
    println!("Scanned {} files, uploaded {}, skipped {} duplicates.",
        report.files_scanned, report.files_imported, report.files_skipped_duplicates);

    // 4. Perform Haversine proximity search (within 50 km of San Francisco)
    let filter = SearchFilter {
        near_location: Some((GeoLocation { latitude: 37.7749, longitude: -122.4194, altitude: None }, 50.0)),
        ..Default::default()
    };

    let results = library.search(&filter).await;
    println!("Found {} photos matching geographic criteria.", results.len());

    Ok(())
}
```

---

## 7. Tauri Desktop & Android Integration

In the desktop and mobile interface (`cloud_sync_tauri`), the Photos Hub provides:
- **Automatic Camera Roll Discovery**: Automatically identifies default DCIM directories on Android (`/storage/emulated/0/DCIM/Camera`).
- **Embedded Visual Timeline**: Interactive masonry photo grid with date sticky headers.
- **Zero Local Footprint Mode**: Stream previews directly from remote storage or thumbnail caches without downloading original multi-gigabyte video files.
