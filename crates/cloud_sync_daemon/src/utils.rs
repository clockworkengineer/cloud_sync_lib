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
