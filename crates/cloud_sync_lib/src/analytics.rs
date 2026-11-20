//! Remote storage analytics and directory tree builder (Rclone `size` & `tree` equivalent).
//!
//! Provides statistical insights into remote storage hierarchies: total file count, byte distribution,
//! extension breakdowns, top largest files, and hierarchical tree visualization.

use crate::traits::{StorageBackend, StorageError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Storage statistics aggregated by file extension.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ExtensionStats {
    pub count: usize,
    pub bytes: u64,
}

/// Representation of an analyzed file in storage analytics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalyzedFile {
    pub path: String,
    pub size: u64,
    pub checksum: Option<String>,
}

/// Comprehensive analytics report for a storage backend path.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct StorageAnalytics {
    pub total_files: usize,
    pub total_folders: usize,
    pub total_bytes: u64,
    pub extension_breakdown: HashMap<String, ExtensionStats>,
    pub largest_files: Vec<AnalyzedFile>,
    pub max_directory_depth: usize,
}

/// Hierarchical tree node representing files and directories.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub children: Vec<TreeNode>,
}

/// Computes comprehensive storage analytics for the target remote path.
pub async fn compute_storage_analytics(
    backend: &(dyn StorageBackend + Send + Sync),
    remote_path: &str,
) -> Result<StorageAnalytics, StorageError> {
    let mut analytics = StorageAnalytics::default();
    let mut all_files: Vec<AnalyzedFile> = Vec::new();

    collect_analytics_recursive(backend, remote_path, 0, &mut analytics, &mut all_files).await?;

    // Sort and retain top 10 largest files
    all_files.sort_by(|a, b| b.size.cmp(&a.size));
    all_files.truncate(10);
    analytics.largest_files = all_files;

    Ok(analytics)
}

async fn collect_analytics_recursive(
    backend: &(dyn StorageBackend + Send + Sync),
    current_path: &str,
    current_depth: usize,
    analytics: &mut StorageAnalytics,
    all_files: &mut Vec<AnalyzedFile>,
) -> Result<(), StorageError> {
    analytics.max_directory_depth = analytics.max_directory_depth.max(current_depth);
    let items = backend.list(current_path).await?;

    for item in items {
        let path_str = item.path.to_string_lossy().replace('\\', "/");
        if item.is_dir {
            analytics.total_folders += 1;
            Box::pin(collect_analytics_recursive(
                backend,
                &path_str,
                current_depth + 1,
                analytics,
                all_files,
            ))
            .await?;
        } else {
            analytics.total_files += 1;
            analytics.total_bytes += item.size;

            let file_name = item.path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            let ext = Path::new(file_name)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("no_ext")
                .to_lowercase();

            let entry = analytics.extension_breakdown.entry(ext).or_default();
            entry.count += 1;
            entry.bytes += item.size;

            all_files.push(AnalyzedFile {
                path: path_str,
                size: item.size,
                checksum: item.checksum,
            });
        }
    }
    Ok(())
}

/// Constructs a hierarchical directory [`TreeNode`] of the remote path up to `max_depth`.
pub async fn build_remote_tree(
    backend: &(dyn StorageBackend + Send + Sync),
    remote_path: &str,
    max_depth: Option<usize>,
) -> Result<TreeNode, StorageError> {
    let clean_path = remote_path.trim_matches('/').to_string();
    let name = if clean_path.is_empty() {
        "root".to_string()
    } else {
        clean_path.split('/').last().unwrap_or("root").to_string()
    };

    build_tree_recursive(backend, &clean_path, name, 0, max_depth).await
}

async fn build_tree_recursive(
    backend: &(dyn StorageBackend + Send + Sync),
    current_path: &str,
    name: String,
    depth: usize,
    max_depth: Option<usize>,
) -> Result<TreeNode, StorageError> {
    let mut node = TreeNode {
        name,
        path: current_path.to_string(),
        is_dir: true,
        size: 0,
        children: Vec::new(),
    };

    if let Some(limit) = max_depth {
        if depth >= limit {
            return Ok(node);
        }
    }

    let items = backend.list(current_path).await?;
    let mut total_size = 0;

    for item in items {
        let path_str = item.path.to_string_lossy().replace('\\', "/");
        let item_name = item
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("item")
            .to_string();

        if item.is_dir {
            let child = Box::pin(build_tree_recursive(
                backend,
                &path_str,
                item_name,
                depth + 1,
                max_depth,
            ))
            .await?;
            total_size += child.size;
            node.children.push(child);
        } else {
            total_size += item.size;
            node.children.push(TreeNode {
                name: item_name,
                path: path_str,
                is_dir: false,
                size: item.size,
                children: Vec::new(),
            });
        }
    }

    node.size = total_size;
    node.children
        .sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    Ok(node)
}

/// Formats a [`TreeNode`] as an ASCII visual tree (similar to UNIX `tree`).
pub fn format_ascii_tree(node: &TreeNode) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}/ ({})\n", node.name, format_bytes(node.size)));
    render_ascii_children(&node.children, "", &mut out);
    out
}

fn render_ascii_children(children: &[TreeNode], prefix: &str, out: &mut String) {
    for (i, child) in children.iter().enumerate() {
        let is_last = i == children.len() - 1;
        let branch = if is_last { "└── " } else { "├── " };
        let next_prefix = if is_last { "    " } else { "│   " };

        if child.is_dir {
            out.push_str(&format!(
                "{}{}{}/ ({})\n",
                prefix,
                branch,
                child.name,
                format_bytes(child.size)
            ));
            render_ascii_children(&child.children, &format!("{}{}", prefix, next_prefix), out);
        } else {
            out.push_str(&format!(
                "{}{}{} ({})\n",
                prefix,
                branch,
                child.name,
                format_bytes(child.size)
            ));
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_sim::LocalSimulation;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_storage_analytics_and_tree() {
        let temp_local = tempdir().unwrap();
        let temp_remote = tempdir().unwrap();

        let backend = LocalSimulation::new(temp_remote.path().to_path_buf(), "test".to_string());

        let doc = temp_local.path().join("report.pdf");
        tokio::fs::write(&doc, vec![0u8; 5000]).await.unwrap();
        backend.upload(&doc, "docs/report.pdf").await.unwrap();

        let img = temp_local.path().join("photo.jpg");
        tokio::fs::write(&img, vec![0u8; 12000]).await.unwrap();
        backend.upload(&img, "images/photo.jpg").await.unwrap();

        let analytics = compute_storage_analytics(&backend, "").await.unwrap();
        assert_eq!(analytics.total_files, 2);
        assert_eq!(analytics.total_folders, 2);
        assert_eq!(analytics.total_bytes, 17000);
        assert_eq!(analytics.extension_breakdown.get("pdf").unwrap().count, 1);
        assert_eq!(analytics.extension_breakdown.get("jpg").unwrap().count, 1);
        assert_eq!(analytics.largest_files[0].size, 12000);

        let tree = build_remote_tree(&backend, "", None).await.unwrap();
        assert_eq!(tree.size, 17000);
        assert_eq!(tree.children.len(), 2);

        let ascii = format_ascii_tree(&tree);
        assert!(ascii.contains("docs/"));
        assert!(ascii.contains("photo.jpg"));
    }
}
