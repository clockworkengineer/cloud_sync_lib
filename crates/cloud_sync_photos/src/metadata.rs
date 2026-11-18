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
    use ring::digest::{Context, SHA256};
    let mut file = File::open(path)?;
    let mut context = Context::new(&SHA256);
    let mut buffer = [0u8; 65536];

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        context.update(&buffer[..count]);
    }

    let digest = context.finish();
    let hex_str: String = digest
        .as_ref()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    Ok(hex_str)
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
                        0x0004 => {
                            // GPSLongitude (3 rationals: degrees, minutes, seconds)
                            if val_offset + 24 <= data.len() {
                                if let (Some(d), Some(m), Some(s)) = (
                                    read_rational(val_offset),
                                    read_rational(val_offset + 8),
                                    read_rational(val_offset + 16),
                                ) {
                                    lon_deg = Some(d + (m / 60.0) + (s / 3600.0));
                                }
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
