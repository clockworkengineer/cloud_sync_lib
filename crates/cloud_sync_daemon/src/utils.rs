use std::path::Path;

/// Returns the relative remote path by stripping the watch directory prefix from the local path.
pub fn get_remote_path(path: &Path, watch_dir: &Path) -> Option<String> {
    let relative_path = match path.strip_prefix(watch_dir) {
        Ok(p) => p.to_path_buf(),
        Err(_) => {
            let path_str = path.to_string_lossy();
            let watch_dir_str = watch_dir.to_string_lossy();
            if path_str.starts_with(&*watch_dir_str) {
                Path::new(&path_str[watch_dir_str.len()..]).to_path_buf()
            } else {
                return None;
            }
        }
    };
    Some(relative_path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_remote_path_matching() {
        let watch_dir = Path::new("/home/user/sync");
        let file_path = Path::new("/home/user/sync/docs/sub/notes.txt");
        let remote = get_remote_path(file_path, watch_dir);
        assert_eq!(remote, Some("docs/sub/notes.txt".to_string()));
    }

    #[test]
    fn test_get_remote_path_non_matching() {
        let watch_dir = Path::new("/home/user/sync");
        let outside = Path::new("/home/user/other/file.txt");
        assert_eq!(get_remote_path(outside, watch_dir), None);
    }
}
