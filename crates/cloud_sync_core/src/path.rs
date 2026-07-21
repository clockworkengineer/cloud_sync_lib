use alloc::borrow::Cow;
use alloc::string::String;
use alloc::string::ToString;
use alloc::format;

/// Guarantee standard Unix slashes `/` on remote backend path operations.
pub fn normalize_remote_path(path: &str) -> Cow<'_, str> {
    if path.contains('\\') {
        Cow::Owned(path.replace('\\', "/"))
    } else {
        Cow::Borrowed(path)
    }
}

/// Formats a relative remote path, incorporating an optional destination folder prefix.
pub fn format_relative_path<'a>(remote_path: &'a str, destination_folder: Option<&str>) -> Cow<'a, str> {
    let normalized = normalize_remote_path(remote_path);
    let has_backslash = matches!(normalized, Cow::Owned(_));
    let clean_path = normalized.trim_start_matches('/');

    if let Some(dest_folder) = destination_folder {
        let clean_dest = normalize_remote_path(dest_folder);
        let clean_dest_trimmed = clean_dest.trim_matches('/');
        if !clean_dest_trimmed.is_empty() {
            if clean_path.is_empty() {
                return Cow::Owned(clean_dest_trimmed.to_string());
            } else {
                return Cow::Owned(format!("{}/{}", clean_dest_trimmed, clean_path));
            }
        }
    }

    if has_backslash || clean_path.len() != remote_path.len() {
        Cow::Owned(clean_path.to_string())
    } else {
        Cow::Borrowed(remote_path)
    }
}

/// Formats an absolute remote path starting with a slash, incorporating an optional destination folder prefix.
pub fn format_absolute_path<'a>(remote_path: &'a str, destination_folder: Option<&str>) -> Cow<'a, str> {
    let normalized = normalize_remote_path(remote_path);
    let clean_path = normalized.trim_start_matches('/');
    let mut full_path = String::new();

    if let Some(dest_folder) = destination_folder {
        let clean_dest = normalize_remote_path(dest_folder);
        let clean_dest_trimmed = clean_dest.trim_matches('/');
        if !clean_dest_trimmed.is_empty() {
            full_path.push('/');
            full_path.push_str(clean_dest_trimmed);
        }
    }

    if !clean_path.is_empty() {
        full_path.push('/');
        full_path.push_str(clean_path);
    }

    Cow::Owned(full_path)
}

#[cfg(feature = "std")]
pub fn strip_destination_prefix(item_path: &std::path::Path, destination_folder: Option<&str>) -> std::path::PathBuf {
    if let Some(dest_folder) = destination_folder {
        let clean_dest = dest_folder.trim_matches('/');
        if !clean_dest.is_empty() {
            if let Ok(stripped) = item_path.strip_prefix(clean_dest) {
                return stripped.to_path_buf();
            }
        }
    }
    item_path.to_path_buf()
}

/// URL encodes a string according to RFC 3986 unreserved characters (A-Z, a-z, 0-9, -, _, ., ~).
pub fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

/// URL encodes a path string, preserving unreserved characters and directory slashes `/`.
pub fn url_encode_path(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

#[cfg(feature = "std")]
pub fn get_permissions(permissions: &std::fs::Permissions) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(permissions.mode())
    }
    #[cfg(not(unix))]
    {
        if permissions.readonly() {
            Some(0o444)
        } else {
            Some(0o666)
        }
    }
}
