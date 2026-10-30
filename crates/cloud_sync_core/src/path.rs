use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;

/// Strongly typed normalized remote relative path enforcing invariants:
/// - Unix forward slashes `/`
/// - No leading slashes
/// - Traversal segments (`..`, `./`) stripped or rejected
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct NormalizedPath(String);

fn is_reserved_dos_name(name: &str) -> bool {
    let stem = match name.split_once('.') {
        Some((prefix, _)) => prefix,
        None => name,
    };
    if stem.eq_ignore_ascii_case("CON")
        || stem.eq_ignore_ascii_case("PRN")
        || stem.eq_ignore_ascii_case("AUX")
        || stem.eq_ignore_ascii_case("NUL")
    {
        return true;
    }
    if stem.len() == 4 {
        let prefix = &stem[..3];
        let suffix = stem.as_bytes()[3];
        if (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
            && (b'1'..=b'9').contains(&suffix)
        {
            return true;
        }
    }
    false
}

impl NormalizedPath {
    pub fn new(path: &str) -> Result<Self, &'static str> {
        if path.contains('\0') {
            return Err("Null bytes are prohibited in paths");
        }
        let normalized = path.replace('\\', "/");
        let trimmed = normalized.trim_start_matches('/');
        if trimmed.is_empty() {
            return Ok(Self(String::new()));
        }

        for segment in trimmed.split('/') {
            if segment == "." || segment == ".." {
                return Err("Unsafe path traversal sequence detected");
            }
            if segment.contains(':') {
                return Err("Alternate Data Stream or colon prohibited in paths");
            }
            if is_reserved_dos_name(segment) {
                return Err("DOS reserved device name prohibited in paths");
            }
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn new_unchecked(path: &str) -> Self {
        let normalized = path.replace('\\', "/");
        let trimmed = normalized.trim_start_matches('/');
        Self(trimmed.to_string())
    }

    pub fn empty() -> Self {
        Self(String::new())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn join(&self, child: &str) -> Self {
        if self.0.is_empty() {
            Self::new_unchecked(child)
        } else {
            let child_clean = child.replace('\\', "/");
            let child_trimmed = child_clean.trim_matches('/');
            Self(format!("{}/{}", self.0, child_trimmed))
        }
    }
}

impl core::ops::Deref for NormalizedPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for NormalizedPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for NormalizedPath {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for NormalizedPath {
    fn from(s: &str) -> Self {
        Self::new_unchecked(s)
    }
}

impl From<String> for NormalizedPath {
    fn from(s: String) -> Self {
        Self::new_unchecked(&s)
    }
}

/// Guarantee standard Unix slashes `/` on remote backend path operations.
pub fn normalize_remote_path(path: &str) -> Cow<'_, str> {
    if path.contains('\\') {
        Cow::Owned(path.replace('\\', "/"))
    } else {
        Cow::Borrowed(path)
    }
}

/// Formats a relative remote path, incorporating an optional destination folder prefix.
pub fn format_relative_path<'a>(
    remote_path: &'a str,
    destination_folder: Option<&str>,
) -> Cow<'a, str> {
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
pub fn format_absolute_path<'a>(
    remote_path: &'a str,
    destination_folder: Option<&str>,
) -> Cow<'a, str> {
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
pub fn strip_destination_prefix(
    item_path: &std::path::Path,
    destination_folder: Option<&str>,
) -> std::path::PathBuf {
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

/// Extracts parent directory path and file name from a given path string.
/// If no parent directory exists, returns an empty string for the parent.
#[cfg(feature = "std")]
pub fn get_parent_and_filename(path_str: &str) -> (String, String) {
    let path = std::path::Path::new(path_str);
    let parent = path
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string();
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    (parent, file_name)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_remote_path() {
        assert_eq!(normalize_remote_path("foo\\bar"), "foo/bar");
        assert_eq!(normalize_remote_path("foo/bar"), "foo/bar");
    }

    #[test]
    fn test_normalized_path_security() {
        // Valid paths with double dots in filename must be allowed
        assert!(NormalizedPath::new("backup..v1.tar").is_ok());
        assert!(NormalizedPath::new("documents/report..final.pdf").is_ok());
        assert_eq!(NormalizedPath::new("docs/my..file.txt").unwrap().as_str(), "docs/my..file.txt");

        // Traversal sequences must be rejected
        assert!(NormalizedPath::new("../secret.txt").is_err());
        assert!(NormalizedPath::new("docs/../../etc/passwd").is_err());
        assert!(NormalizedPath::new("docs/./file.txt").is_err());

        // Windows Alternate Data Streams must be rejected
        assert!(NormalizedPath::new("file.txt:hidden").is_err());
        assert!(NormalizedPath::new("c:/path/file.txt:stream").is_err());

        // Reserved DOS device names must be rejected
        assert!(NormalizedPath::new("nul").is_err());
        assert!(NormalizedPath::new("folder/nul.txt").is_err());
        assert!(NormalizedPath::new("com1").is_err());
        assert!(NormalizedPath::new("nested/aux.dat").is_err());
        assert!(NormalizedPath::new("con").is_err());

        // Null bytes must be rejected
        assert!(NormalizedPath::new("file\0.txt").is_err());
    }

    #[test]
    fn test_format_relative_path() {
        assert_eq!(format_relative_path("foo/bar", None), "foo/bar");
        assert_eq!(format_relative_path("/foo/bar", None), "foo/bar");
        assert_eq!(format_relative_path("foo\\bar", None), "foo/bar");
        assert_eq!(
            format_relative_path("foo/bar", Some("dest")),
            "dest/foo/bar"
        );
        assert_eq!(format_relative_path("", Some("dest")), "dest");
    }

    #[test]
    fn test_format_absolute_path() {
        assert_eq!(format_absolute_path("foo/bar", None), "/foo/bar");
        assert_eq!(format_absolute_path("/foo/bar", None), "/foo/bar");
        assert_eq!(
            format_absolute_path("foo/bar", Some("dest")),
            "/dest/foo/bar"
        );
        assert_eq!(format_absolute_path("", Some("dest")), "/dest");
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_strip_destination_prefix() {
        use std::path::Path;
        let p = Path::new("dest/foo/bar.txt");
        assert_eq!(
            strip_destination_prefix(p, Some("dest")),
            Path::new("foo/bar.txt")
        );
        assert_eq!(strip_destination_prefix(p, None), p);
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_get_parent_and_filename() {
        let (parent, filename) = get_parent_and_filename("foo/bar/baz.txt");
        assert_eq!(parent, "foo/bar");
        assert_eq!(filename, "baz.txt");

        let (parent, filename) = get_parent_and_filename("baz.txt");
        assert_eq!(parent, "");
        assert_eq!(filename, "baz.txt");
    }

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("foo/bar"), "foo%2Fbar");
        assert_eq!(url_encode("abc-123_.~"), "abc-123_.~");
    }

    #[test]
    fn test_url_encode_path() {
        assert_eq!(url_encode_path("hello world/foo"), "hello%20world/foo");
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_get_permissions() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("perm.txt");
        std::fs::write(&file_path, "test").unwrap();
        let metadata = std::fs::metadata(file_path).unwrap();
        let perms = metadata.permissions();
        let mode = get_permissions(&perms);
        assert!(mode.is_some());
    }
}
