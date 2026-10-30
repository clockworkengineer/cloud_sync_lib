//! Heuristic and hash-based rename/move detection.

#[derive(Debug, Clone)]
pub struct CandidateItem {
    pub rel_path: String,
    pub size: u64,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedMove {
    pub from: String,
    pub to: String,
    pub size: u64,
}

pub struct RenameDetector;

impl RenameDetector {
    /// Detects rename/move operations by matching deleted items with added items
    /// using content checksums and matching file sizes.
    pub fn detect_moves(
        deletions: &[CandidateItem],
        additions: &[CandidateItem],
    ) -> (Vec<DetectedMove>, Vec<CandidateItem>, Vec<CandidateItem>) {
        let mut moves = Vec::new();
        let mut unmatched_deletions = Vec::new();
        let mut unmatched_additions = additions.to_vec();

        // Build lookup map for additions by (checksum, size)
        for del in deletions {
            let mut matched_idx = None;

            for (idx, add) in unmatched_additions.iter().enumerate() {
                let size_matches = del.size == add.size && del.size > 0;
                let checksum_matches = match (&del.checksum, &add.checksum) {
                    (Some(c1), Some(c2)) => c1 == c2,
                    _ => false,
                };

                // Match by exact hash or identical size + identical filename across different folders
                let filename_matches = {
                    let p1 = std::path::Path::new(&del.rel_path).file_name();
                    let p2 = std::path::Path::new(&add.rel_path).file_name();
                    p1.is_some() && p1 == p2
                };

                if (checksum_matches && size_matches) || (size_matches && filename_matches) {
                    matched_idx = Some(idx);
                    break;
                }
            }

            if let Some(idx) = matched_idx {
                let matched_add = unmatched_additions.remove(idx);
                moves.push(DetectedMove {
                    from: del.rel_path.clone(),
                    to: matched_add.rel_path,
                    size: del.size,
                });
            } else {
                unmatched_deletions.push(del.clone());
            }
        }

        (moves, unmatched_deletions, unmatched_additions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rename_detection_by_checksum() {
        let deletions = vec![CandidateItem {
            rel_path: "docs/old_report.pdf".to_string(),
            size: 1048576,
            checksum: Some("hash123".to_string()),
        }];
        let additions = vec![CandidateItem {
            rel_path: "archive/new_report.pdf".to_string(),
            size: 1048576,
            checksum: Some("hash123".to_string()),
        }];

        let (moves, remaining_del, remaining_add) =
            RenameDetector::detect_moves(&deletions, &additions);

        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].from, "docs/old_report.pdf");
        assert_eq!(moves[0].to, "archive/new_report.pdf");
        assert!(remaining_del.is_empty());
        assert!(remaining_add.is_empty());
    }

    #[test]
    fn test_rename_detection_by_filename_and_size() {
        let deletions = vec![CandidateItem {
            rel_path: "folder_a/data.bin".to_string(),
            size: 2048,
            checksum: None,
        }];
        let additions = vec![CandidateItem {
            rel_path: "folder_b/data.bin".to_string(),
            size: 2048,
            checksum: None,
        }];

        let (moves, remaining_del, remaining_add) =
            RenameDetector::detect_moves(&deletions, &additions);

        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].from, "folder_a/data.bin");
        assert_eq!(moves[0].to, "folder_b/data.bin");
        assert!(remaining_del.is_empty());
        assert!(remaining_add.is_empty());
    }
}
