//! Pure Rust EXIF and media file metadata extractor.

use crate::models::{ExifMetadata, GeoLocation, MediaType};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Detects media type and specific format by file extension and magic bytes.
pub fn detect_media_format(path: &Path) -> (MediaType, String) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "jpg" | "jpeg" => (MediaType::Photo, "image/jpeg".to_string()),
        "png" => (MediaType::Photo, "image/png".to_string()),
        "webp" => (MediaType::Photo, "image/webp".to_string()),
        "heic" | "heif" => (MediaType::Photo, "image/heic".to_string()),
        "gif" => (MediaType::Photo, "image/gif".to_string()),
        "tif" | "tiff" => (MediaType::Photo, "image/tiff".to_string()),
        "raw" | "cr2" | "nef" | "arw" | "dng" => (MediaType::Photo, "image/x-raw".to_string()),
        "mp4" => (MediaType::Video, "video/mp4".to_string()),
        "mov" => (MediaType::Video, "video/quicktime".to_string()),
        "mkv" => (MediaType::Video, "video/x-matroska".to_string()),
        "webm" => (MediaType::Video, "video/webm".to_string()),
        "avi" => (MediaType::Video, "video/x-msvideo".to_string()),
        _ => (MediaType::Unknown, "application/octet-stream".to_string()),
    }
}

/// Computes the SHA-256 hash of a file for content deduplication.
pub fn compute_file_sha256(path: &Path) -> std::io::Result<String> {
    cloud_sync_std::checksum::compute_sha256_sync(path)
}

/// Parses an EXIF date string formatted as "YYYY:MM:DD HH:MM:SS".
pub fn parse_exif_datetime(s: &str) -> Option<DateTime<Utc>> {
    let clean = s.trim().trim_matches('\0');
    if clean.len() < 19 {
        return None;
    }
    let date_str = &clean[..19];
    if let Ok(naive) = NaiveDateTime::parse_from_str(date_str, "%Y:%m:%d %H:%M:%S") {
        return Some(Utc.from_utc_datetime(&naive));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        return Some(Utc.from_utc_datetime(&naive));
    }
    None
}

/// Extracts EXIF metadata from a media file (JPEG/TIFF/etc.), falling back gracefully.
pub fn extract_metadata(path: &Path) -> (ExifMetadata, Option<DateTime<Utc>>) {
    let mut exif = ExifMetadata::default();
    let mut date_taken = None;

    if let Ok(mut file) = File::open(path) {
        if let Ok(meta) = extract_jpeg_exif(&mut file) {
            if let Some(ref d_str) = meta.date_taken_str {
                date_taken = parse_exif_datetime(d_str);
            }
            exif = meta;
        }
    }

    (exif, date_taken)
}

/// Inspects a local media file and generates full MediaItem metadata.
pub fn inspect_media_file(path: &Path) -> std::io::Result<crate::models::MediaItem> {
    let metadata = std::fs::metadata(path)?;
    let file_size = metadata.len();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let (media_type, mime_type) = detect_media_format(path);
    let sha256_hash = compute_file_sha256(path)?;

    let modified_system = metadata
        .modified()
        .unwrap_or_else(|_| std::time::SystemTime::now());
    let modified_at: DateTime<Utc> = modified_system.into();

    let (exif, exif_date) = extract_metadata(path);
    let date_taken = exif_date.unwrap_or(modified_at);

    let id = sha256_hash.clone();

    Ok(crate::models::MediaItem {
        id,
        file_name,
        local_path: path.to_string_lossy().to_string(),
        remote_path: None,
        size_bytes: file_size,
        sha256_hash,
        media_type,
        mime_type,
        date_taken,
        modified_at,
        exif,
        album_ids: Vec::new(),
        is_favorite: false,
        tags: Vec::new(),
    })
}

// Internal pure-Rust TIFF/EXIF parser for JPEG files
fn extract_jpeg_exif(file: &mut File) -> std::io::Result<ExifMetadata> {
    let mut soi = [0u8; 2];
    file.read_exact(&mut soi)?;
    if soi != [0xFF, 0xD8] {
        return Ok(ExifMetadata::default());
    }

    let mut buf = [0u8; 4];
    loop {
        if file.read_exact(&mut buf[..2]).is_err() {
            break;
        }
        if buf[0] != 0xFF {
            break;
        }
        let marker = buf[1];
        if marker == 0xDA || marker == 0xD9 {
            // Start of scan or end of image
            break;
        }

        if file.read_exact(&mut buf[2..4]).is_err() {
            break;
        }
        let length = u16::from_be_bytes([buf[2], buf[3]]) as usize;
        if length < 2 {
            break;
        }
        let data_len = length - 2;

        if marker == 0xE1 {
            // APP1 marker (potential EXIF)
            let mut app1_data = vec![0u8; data_len];
            file.read_exact(&mut app1_data)?;

            if app1_data.len() > 6 && &app1_data[..6] == b"Exif\0\0" {
                return Ok(parse_exif_tiff(&app1_data[6..]));
            }
        } else {
            file.seek(SeekFrom::Current(data_len as i64))?;
        }
    }

    Ok(ExifMetadata::default())
}

fn parse_exif_tiff(data: &[u8]) -> ExifMetadata {
    let mut meta = ExifMetadata::default();
    if data.len() < 8 {
        return meta;
    }

    let is_little_endian = match &data[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return meta,
    };

    let read_u16 = |buf: &[u8], offset: usize| -> Option<u16> {
        if offset + 2 > buf.len() {
            return None;
        }
        if is_little_endian {
            Some(u16::from_le_bytes([buf[offset], buf[offset + 1]]))
        } else {
            Some(u16::from_be_bytes([buf[offset], buf[offset + 1]]))
        }
    };

    let read_u32 = |buf: &[u8], offset: usize| -> Option<u32> {
        if offset + 4 > buf.len() {
            return None;
        }
        if is_little_endian {
            Some(u32::from_le_bytes([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]))
        } else {
            Some(u32::from_be_bytes([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]))
        }
    };

    let ifd0_offset = match read_u32(data, 4) {
        Some(off) => off as usize,
        None => return meta,
    };

    if ifd0_offset >= data.len() {
        return meta;
    }

    let num_entries = match read_u16(data, ifd0_offset) {
        Some(n) => n as usize,
        None => return meta,
    };

    let mut exif_subifd_offset = None;
    let mut gps_subifd_offset = None;

    let read_ascii = |offset: usize, count: usize| -> Option<String> {
        if offset + count <= data.len() {
            let slice = &data[offset..offset + count];
            let clean: Vec<u8> = slice.iter().copied().take_while(|&c| c != 0).collect();
            String::from_utf8(clean).ok()
        } else {
            None
        }
    };

    // Scan IFD0 entries
    for i in 0..num_entries {
        let entry_offset = ifd0_offset + 2 + (i * 12);
        if entry_offset + 12 > data.len() {
            break;
        }

        let tag = read_u16(data, entry_offset).unwrap_or(0);
        let _format = read_u16(data, entry_offset + 2).unwrap_or(0);
        let count = read_u32(data, entry_offset + 4).unwrap_or(0) as usize;
        let value_or_offset = read_u32(data, entry_offset + 8).unwrap_or(0) as usize;

        let str_val = if count <= 4 {
            let slice = &data[entry_offset + 8..entry_offset + 8 + count];
            let clean: Vec<u8> = slice.iter().copied().take_while(|&c| c != 0).collect();
            String::from_utf8(clean).ok()
        } else {
            read_ascii(value_or_offset, count)
        };

        match tag {
            0x010F => meta.make = str_val,
            0x0110 => meta.model = str_val,
            0x0132 => {
                if meta.date_taken_str.is_none() {
                    meta.date_taken_str = str_val;
                }
            }
            0x8769 => exif_subifd_offset = Some(value_or_offset),
            0x8825 => gps_subifd_offset = Some(value_or_offset),
            _ => {}
        }
    }

    // Scan Exif SubIFD entries if present
    if let Some(sub_offset) = exif_subifd_offset {
        if sub_offset < data.len() {
            if let Some(sub_entries) = read_u16(data, sub_offset) {
                for i in 0..(sub_entries as usize) {
                    let entry_offset = sub_offset + 2 + (i * 12);
                    if entry_offset + 12 > data.len() {
                        break;
                    }
                    let tag = read_u16(data, entry_offset).unwrap_or(0);
                    let count = read_u32(data, entry_offset + 4).unwrap_or(0) as usize;
                    let val = read_u32(data, entry_offset + 8).unwrap_or(0) as usize;

                    let str_val = if count <= 4 {
                        let slice = &data[entry_offset + 8..entry_offset + 8 + count];
                        let clean: Vec<u8> =
                            slice.iter().copied().take_while(|&c| c != 0).collect();
                        String::from_utf8(clean).ok()
                    } else {
                        read_ascii(val, count)
                    };

                    match tag {
                        0x9003 | 0x9004 => {
                            if str_val.is_some() {
                                meta.date_taken_str = str_val;
                            }
                        }
                        0x8827 => meta.iso = Some(val as u32),
                        0xA002 => meta.width = Some(val as u32),
                        0xA003 => meta.height = Some(val as u32),
                        0xA434 => meta.lens_model = str_val,
                        _ => {}
                    }
                }
            }
        }
    }

    // Scan GPS SubIFD entries if present
    if let Some(gps_offset) = gps_subifd_offset {
        if gps_offset < data.len() {
            if let Some(gps_entries) = read_u16(data, gps_offset) {
                let mut lat_ref = None;
                let mut lon_ref = None;
                let mut lat_deg = None;
                let mut lon_deg = None;

                let read_rational = |offset: usize| -> Option<f64> {
                    let num = read_u32(data, offset)? as f64;
                    let den = read_u32(data, offset + 4)? as f64;
                    if den > 0.0 {
                        Some(num / den)
                    } else {
                        None
                    }
                };

                for i in 0..(gps_entries as usize) {
                    let entry_offset = gps_offset + 2 + (i * 12);
                    if entry_offset + 12 > data.len() {
                        break;
                    }
                    let tag = read_u16(data, entry_offset).unwrap_or(0);
                    let val_offset = read_u32(data, entry_offset + 8).unwrap_or(0) as usize;

                    match tag {
                        0x0001 => {
                            // GPSLatitudeRef ('N' or 'S')
                            let c = data[entry_offset + 8] as char;
                            lat_ref = Some(c);
                        }
                        0x0002 => {
                            // GPSLatitude (3 rationals: degrees, minutes, seconds)
                            if val_offset + 24 <= data.len() {
                                if let (Some(d), Some(m), Some(s)) = (
                                    read_rational(val_offset),
                                    read_rational(val_offset + 8),
                                    read_rational(val_offset + 16),
                                ) {
                                    lat_deg = Some(d + (m / 60.0) + (s / 3600.0));
                                }
                            }
                        }
                        0x0003 => {
                            // GPSLongitudeRef ('E' or 'W')
                            let c = data[entry_offset + 8] as char;
                            lon_ref = Some(c);
                        }
                        0x0004 if val_offset + 24 <= data.len() => {
                            // GPSLongitude (3 rationals: degrees, minutes, seconds)
                            if let (Some(d), Some(m), Some(s)) = (
                                read_rational(val_offset),
                                read_rational(val_offset + 8),
                                read_rational(val_offset + 16),
                            ) {
                                lon_deg = Some(d + (m / 60.0) + (s / 3600.0));
                            }
                        }
                        _ => {}
                    }
                }

                if let (Some(mut lat), Some(mut lon)) = (lat_deg, lon_deg) {
                    if let Some('S') = lat_ref {
                        lat = -lat;
                    }
                    if let Some('W') = lon_ref {
                        lon = -lon;
                    }
                    meta.location = Some(GeoLocation::new(lat, lon));
                }
            }
        }
    }

    meta
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_detect_media_format() {
        let cases = [
            ("img.jpg", MediaType::Photo, "image/jpeg"),
            ("img.jpeg", MediaType::Photo, "image/jpeg"),
            ("pic.png", MediaType::Photo, "image/png"),
            ("pic.webp", MediaType::Photo, "image/webp"),
            ("pic.heic", MediaType::Photo, "image/heic"),
            ("pic.heif", MediaType::Photo, "image/heic"),
            ("anim.gif", MediaType::Photo, "image/gif"),
            ("doc.tif", MediaType::Photo, "image/tiff"),
            ("doc.tiff", MediaType::Photo, "image/tiff"),
            ("raw.cr2", MediaType::Photo, "image/x-raw"),
            ("raw.nef", MediaType::Photo, "image/x-raw"),
            ("raw.arw", MediaType::Photo, "image/x-raw"),
            ("raw.dng", MediaType::Photo, "image/x-raw"),
            ("raw.raw", MediaType::Photo, "image/x-raw"),
            ("vid.mp4", MediaType::Video, "video/mp4"),
            ("vid.mov", MediaType::Video, "video/quicktime"),
            ("vid.mkv", MediaType::Video, "video/x-matroska"),
            ("vid.webm", MediaType::Video, "video/webm"),
            ("vid.avi", MediaType::Video, "video/x-msvideo"),
            ("file.bin", MediaType::Unknown, "application/octet-stream"),
            ("no_ext", MediaType::Unknown, "application/octet-stream"),
        ];

        for (filename, expected_media, expected_mime) in cases {
            let (media, mime) = detect_media_format(Path::new(filename));
            assert_eq!(
                media, expected_media,
                "Media type mismatch for {}",
                filename
            );
            assert_eq!(mime, expected_mime, "MIME mismatch for {}", filename);
        }
    }

    #[test]
    fn test_parse_exif_datetime() {
        assert!(parse_exif_datetime("").is_none());
        assert!(parse_exif_datetime("short").is_none());
        assert!(parse_exif_datetime("invalid date string here!").is_none());

        let dt1 = parse_exif_datetime("2024:11:24 14:30:00").expect("colon format");
        assert_eq!(dt1.to_rfc3339(), "2024-11-24T14:30:00+00:00");

        let dt2 = parse_exif_datetime("2024-11-24 14:30:00\0").expect("dash format with null");
        assert_eq!(dt2.to_rfc3339(), "2024-11-24T14:30:00+00:00");
    }

    #[test]
    fn test_inspect_media_file_non_jpeg() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"Hello World not a JPEG").unwrap();
        tmp.flush().unwrap();

        let item = inspect_media_file(tmp.path()).unwrap();
        assert_eq!(item.media_type, MediaType::Unknown);
        assert_eq!(item.size_bytes, 22);
        assert!(!item.sha256_hash.is_empty());
    }

    #[test]
    fn test_extract_jpeg_exif_and_tiff_parsing() {
        // Construct a synthetic JPEG with Exif APP1 segment
        // Little-endian TIFF structure:
        // offset 0: "II" (0x49, 0x49)
        // offset 2: 0x002A (42)
        // offset 4: offset to IFD0 = 8
        // IFD0 at offset 8:
        // count = 2 entries
        // entry 0: Tag 0x010F (Make), ASCII, count 6, offset 34 -> "Canon\0"
        // entry 1: Tag 0x0110 (Model), ASCII, count 4, inline -> "R5\0\0"
        let mut tiff = Vec::new();
        // Header (8 bytes)
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes()); // offset to IFD0

        // IFD0: 2 entries (2 bytes)
        tiff.extend_from_slice(&2u16.to_le_bytes());

        // Entry 0: 0x010F (Make), type 2 (ASCII), count 6, offset 38
        tiff.extend_from_slice(&0x010Fu16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&6u32.to_le_bytes());
        tiff.extend_from_slice(&38u32.to_le_bytes());

        // Entry 1: 0x0110 (Model), type 2 (ASCII), count 4, inline "R5\0\0"
        tiff.extend_from_slice(&0x0110u16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&4u32.to_le_bytes());
        tiff.extend_from_slice(b"R5\0\0");

        // Next IFD offset = 0
        tiff.extend_from_slice(&0u32.to_le_bytes());

        // Offset 38: "Canon\0"
        tiff.extend_from_slice(b"Canon\0");

        // Test direct TIFF parser
        let meta = parse_exif_tiff(&tiff);
        assert_eq!(meta.make.as_deref(), Some("Canon"));
        assert_eq!(meta.model.as_deref(), Some("R5"));

        // Now test via JPEG container
        let mut jpeg = Vec::new();
        // SOI marker
        jpeg.extend_from_slice(&[0xFF, 0xD8]);
        // APP1 marker
        jpeg.extend_from_slice(&[0xFF, 0xE1]);
        let app1_len = (2 + 6 + tiff.len()) as u16;
        jpeg.extend_from_slice(&app1_len.to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        // EOI marker
        jpeg.extend_from_slice(&[0xFF, 0xD9]);

        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&jpeg).unwrap();
        tmp.flush().unwrap();

        let (exif, dt) = extract_metadata(tmp.path());
        assert_eq!(exif.make.as_deref(), Some("Canon"));
        assert_eq!(exif.model.as_deref(), Some("R5"));
        assert!(dt.is_none());
    }

    #[test]
    fn test_parse_exif_tiff_big_endian_and_gps() {
        // Big-endian TIFF with GPS subIFD
        let mut tiff = Vec::new();
        // Header
        tiff.extend_from_slice(b"MM");
        tiff.extend_from_slice(&42u16.to_be_bytes());
        tiff.extend_from_slice(&8u32.to_be_bytes()); // IFD0 at 8

        // IFD0: 1 entry pointing to GPS SubIFD (0x8825)
        tiff.extend_from_slice(&1u16.to_be_bytes());
        tiff.extend_from_slice(&0x8825u16.to_be_bytes());
        tiff.extend_from_slice(&4u16.to_be_bytes()); // LONG
        tiff.extend_from_slice(&1u32.to_be_bytes());
        tiff.extend_from_slice(&26u32.to_be_bytes()); // offset to GPS IFD = 26
        tiff.extend_from_slice(&0u32.to_be_bytes()); // next IFD = 0

        // GPS IFD at offset 26
        // 4 entries: LatRef (0x0001), Lat (0x0002), LonRef (0x0003), Lon (0x0004)
        tiff.extend_from_slice(&4u16.to_be_bytes());

        // Entry 0: LatRef 'S' (value in-place)
        tiff.extend_from_slice(&0x0001u16.to_be_bytes());
        tiff.extend_from_slice(&2u16.to_be_bytes());
        tiff.extend_from_slice(&2u32.to_be_bytes());
        tiff.extend_from_slice(b"S\0\0\0");

        // Entry 1: Lat (3 rationals), offset 80
        tiff.extend_from_slice(&0x0002u16.to_be_bytes());
        tiff.extend_from_slice(&5u16.to_be_bytes());
        tiff.extend_from_slice(&3u32.to_be_bytes());
        tiff.extend_from_slice(&80u32.to_be_bytes());

        // Entry 2: LonRef 'W'
        tiff.extend_from_slice(&0x0003u16.to_be_bytes());
        tiff.extend_from_slice(&2u16.to_be_bytes());
        tiff.extend_from_slice(&2u32.to_be_bytes());
        tiff.extend_from_slice(b"W\0\0\0");

        // Entry 3: Lon (3 rationals), offset 104
        tiff.extend_from_slice(&0x0004u16.to_be_bytes());
        tiff.extend_from_slice(&5u16.to_be_bytes());
        tiff.extend_from_slice(&3u32.to_be_bytes());
        tiff.extend_from_slice(&104u32.to_be_bytes());

        // Next IFD = 0
        tiff.extend_from_slice(&0u32.to_be_bytes());

        // Offset 80: Latitude = 33 deg, 51 min, 0 sec (33.85 S)
        // 33/1
        tiff.extend_from_slice(&33u32.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());
        // 51/1
        tiff.extend_from_slice(&51u32.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());
        // 0/1
        tiff.extend_from_slice(&0u32.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());

        // Offset 104: Longitude = 151 deg, 12 min, 0 sec (151.20 W)
        // 151/1
        tiff.extend_from_slice(&151u32.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());
        // 12/1
        tiff.extend_from_slice(&12u32.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());
        // 0/1
        tiff.extend_from_slice(&0u32.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());

        let meta = parse_exif_tiff(&tiff);
        let loc = meta.location.expect("parsed location");
        assert!((loc.latitude - (-33.85)).abs() < 0.001);
        assert!((loc.longitude - (-151.20)).abs() < 0.001);
    }
}
