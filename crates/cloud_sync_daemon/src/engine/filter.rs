use cloud_sync_lib::SyncIgnore;
use std::path::Path;

/// Single Responsibility component handling file filtering, security checks, and path selection.
#[derive(Debug, Clone)]
pub struct FileFilter<'a> {
    gitignore: &'a SyncIgnore,
    selective_sync: Option<&'a [String]>,
    min_size: Option<u64>,
    max_size: Option<u64>,
}

impl<'a> FileFilter<'a> {
    pub fn new(gitignore: &'a SyncIgnore, selective_sync: Option<&'a [String]>) -> Self {
        Self {
            gitignore,
            selective_sync,
            min_size: None,
            max_size: None,
        }
    }

    pub fn with_size_limits(mut self, min_size: Option<u64>, max_size: Option<u64>) -> Self {
        self.min_size = min_size;
        self.max_size = max_size;
        self
    }

    /// Checks if a file size falls within the configured min/max boundaries.
    pub fn is_size_allowed(&self, size: u64) -> bool {
        if let Some(min) = self.min_size {
            if size < min {
                return false;
            }
        }
        if let Some(max) = self.max_size {
            if size > max {
                return false;
            }
        }
        true
    }

    /// Checks whether a remote or local path contains unsafe path traversal characters.
    pub fn is_unsafe_path(path: &str) -> bool {
        path.contains("..") || path.contains("./") || path.starts_with('/')
    }

    /// Checks whether a path is an internal metadata state file.
    pub fn is_state_file(path: &str) -> bool {
        path == ".sync_state.json"
            || path == ".sync_state.bin"
            || path == ".syncignore"
            || (path.starts_with(".sync_state_")
                && (path.ends_with(".json") || path.ends_with(".bin")))
    }

    /// Checks if a given path is ignored by gitignore/syncignore rules.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        self.gitignore.is_ignored(path, is_dir)
    }

    /// Checks if a path is selected by selective sync settings.
    pub fn is_path_selected(&self, rel_path: &str) -> bool {
        if let Some(prefixes) = self.selective_sync {
            if prefixes.is_empty() {
                return true;
            }
            let clean_path = rel_path.trim_start_matches('/');
            for prefix in prefixes {
                let clean_prefix = prefix.trim_start_matches('/');
                if clean_path == clean_prefix
                    || clean_path.starts_with(&format!("{}/", clean_prefix))
                    || clean_prefix.starts_with(&format!("{}/", clean_path))
                {
                    return true;
                }
            }
            false
        } else {
            true
        }
    }
}
