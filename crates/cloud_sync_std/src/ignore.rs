use std::path::Path;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Matches paths against glob exclusions loaded from `.syncignore` and configuration.
#[derive(Debug, Clone)]
pub struct SyncIgnore {
    inner: Gitignore,
}

impl SyncIgnore {
    /// Builds a new `SyncIgnore` pattern matcher based on `.syncignore` and additional patterns.
    pub fn new(watch_dir: &Path, exclude_patterns: &[String]) -> Self {
        let mut builder = GitignoreBuilder::new(watch_dir);
        let syncignore_path = watch_dir.join(".syncignore");
        if syncignore_path.exists() {
            if let Some(err) = builder.add(&syncignore_path) {
                tracing::warn!("Error loading .syncignore at {:?}: {}", syncignore_path, err);
            }
        }
        for pattern in exclude_patterns {
            if let Err(e) = builder.add_line(None, pattern) {
                tracing::warn!("Error parsing exclude pattern '{}': {}", pattern, e);
            }
        }
        let inner = builder.build().unwrap_or_else(|_| Gitignore::empty());
        Self { inner }
    }

    /// Creates an empty pattern matcher.
    pub fn empty() -> Self {
        Self {
            inner: Gitignore::empty(),
        }
    }

    /// Returns true if the path matches any of the ignore patterns.
    pub fn is_ignored<P: AsRef<Path>>(&self, path: P, is_dir: bool) -> bool {
        self.inner.matched_path_or_any_parents(path.as_ref(), is_dir).is_ignore()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_ignore_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let watch_path = temp_dir.path();
        
        let gitignore = SyncIgnore::new(
            watch_path,
            &["*.log".to_string(), "target/".to_string()],
        );

        assert!(gitignore.is_ignored(watch_path.join("error.log"), false));
        assert!(!gitignore.is_ignored(watch_path.join("error.txt"), false));
        assert!(gitignore.is_ignored(watch_path.join("target/debug/app"), false));
    }

    #[test]
    fn test_sync_ignore_empty() {
        let gitignore = SyncIgnore::empty();
        assert!(!gitignore.is_ignored(Path::new("error.log"), false));
    }

    #[test]
    fn test_sync_ignore_invalid_patterns() {
        let temp_dir = tempfile::tempdir().unwrap();
        let watch_path = temp_dir.path();
        
        std::fs::create_dir(watch_path.join(".syncignore")).unwrap();

        let gitignore = SyncIgnore::new(
            watch_path,
            &["**/[a-".to_string()],
        );

        assert!(!gitignore.is_ignored(watch_path.join("test.txt"), false));
    }
}
