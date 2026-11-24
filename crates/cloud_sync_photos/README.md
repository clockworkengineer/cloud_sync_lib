# `cloud_sync_photos` — Multi-Cloud Google Photos Equivalence Engine

`cloud_sync_photos` provides a self-contained photo and video management, deduplication, and backup engine that operates over any of the 17 storage backends supported by `cloud_sync_lib` (Google Drive, OneDrive, Dropbox, S3, Nextcloud, WebDAV, Box, MEGA, Azure Blob, GCS, Backblaze B2, pCloud, IPFS, FTP/FTPS, HTTP/HTTPS, SMB/CIFS, or Multi-Cloud Union).

---

## Key Features

- **Pure-Rust EXIF & Media Metadata Extraction**: Extracts date taken (`DateTimeOriginal`), camera make & model, image dimensions, and GPS coordinates (`latitude`, `longitude`, `altitude`) without external C dependencies.
- **Content-Addressable SHA-256 Deduplication**: Computes SHA-256 digests prior to upload so duplicate photos or videos across folders are never uploaded twice.
- **Chronological Path Organization**:
  - `YearMonth` (`YYYY/MM/IMG_0001.JPG`)
  - `YearMonthDay` (`YYYY/MM/DD/IMG_0001.JPG`)
  - `CameraModel` (`Camera_Model/YYYY/IMG_0001.JPG`)
  - `Flat` (`IMG_0001.JPG`)
- **Cross-Device Catalog Synchronization**: Synchronizes `.photos_catalog.json` with the remote storage backend so timelines, favorites, and virtual albums merge across desktop and Android devices.
- **Multi-Criteria Search**: Filter by date ranges, camera model, media type (Photo/Video), favorite status, album membership, or Haversine GPS radius (`GeoLocation::distance_km`).

---

## Standalone CLI Commands

```bash
# Ingest and backup camera roll / folder
cargo run --bin cloud_sync_photos -- sync --source "D:\DCIM\Camera"

# Chronological timeline feed grouped by Year and Month
cargo run --bin cloud_sync_photos -- timeline

# List indexed photos & videos
cargo run --bin cloud_sync_photos -- list --media-type photo --favorites

# Multi-criteria search (camera, date range, Haversine GPS radius)
cargo run --bin cloud_sync_photos -- search --camera "Sony" --near-lat 37.7749 --near-lon -122.4194 --radius-km 50.0

# Virtual album management
cargo run --bin cloud_sync_photos -- albums create "Vacation 2026"
```

For detailed architecture, EXIF tags, and Android mobile setup, see the **[Photos Hub Guide](../../docs/PHOTOS_HUB.md)**.

---

## Rust API Example

```rust,no_run
use cloud_sync_lib::LocalSimulation;
use cloud_sync_photos::{OrganizeStrategy, PhotoLibrary};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), String> {
    let backend = Arc::new(LocalSimulation::new(
        PathBuf::from("./cloud_simulation/photos"),
        "PhotosCloud".into(),
    ));

    let library = PhotoLibrary::new(
        backend,
        "GooglePhotos_Backup",
        PathBuf::from("./sync_state/photos_catalog.json"),
        OrganizeStrategy::YearMonth,
    )
    .await?;

    let summary = library.sync_folder(Path::new("./DCIM/Camera")).await?;
    println!(
        "Imported {} new photos ({} duplicates skipped)",
        summary.files_imported, summary.files_skipped_duplicates
    );
    Ok(())
}
```
