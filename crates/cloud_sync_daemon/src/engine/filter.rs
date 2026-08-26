use cloud_sync_lib::SyncIgnore;
use std::path::Path;

/// Single Responsibility component handling file filtering, security checks, and path selection.
#[derive(Debug, Clone)]
pub struct FileFilter<'a> {
    gitignore: &'a SyncIgnore,
    selective_sync: Option<&'a [String]>,
}

impl<'a> FileFilter<'a> {
    pub fn new(gitignore: &'a SyncIgnore, selective_sync: Option<&'a [String]>) -> Self {
        Self {
            gitignore,
            selective_sync,
        }
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
            || (path.starts_with(".sync_state_") && (path.ends_with(".json") || path.ends_with(".bin")))
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
