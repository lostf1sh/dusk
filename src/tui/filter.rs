use std::time::{Duration, SystemTime};

use crate::model::node::DiskNode;

/// Criteria for filtering the tree view.
#[derive(Debug, Clone)]
pub enum FilterCriteria {
    /// Filter by file extension (e.g., "rs", "txt").
    Extension(String),
    /// Filter by size range.
    SizeRange {
        min: Option<u64>,
        max: Option<u64>,
    },
    /// Filter files modified within last N days.
    ModifiedWithin(u64),
}

impl FilterCriteria {
    /// Check if a node matches this filter. Directories always match
    /// (to keep the tree structure visible), but they may be pruned if
    /// no descendants match.
    pub fn matches(&self, node: &DiskNode) -> bool {
        // Directories always pass — they'll be shown if they have matching children
        if node.node_type == crate::model::NodeType::Dir {
            return true;
        }

        match self {
            FilterCriteria::Extension(ext) => {
                let ext_lower = ext.to_lowercase();
                node.name
                    .rsplit_once('.')
                    .is_some_and(|(_, e)| e.to_lowercase() == ext_lower)
            }
            FilterCriteria::SizeRange { min, max } => {
                if let Some(min) = min {
                    if node.size < *min {
                        return false;
                    }
                }
                if let Some(max) = max {
                    if node.size > *max {
                        return false;
                    }
                }
                true
            }
            FilterCriteria::ModifiedWithin(days) => {
                if let Some(modified) = node.modified {
                    let cutoff = SystemTime::now() - Duration::from_secs(*days * 86400);
                    modified >= cutoff
                } else {
                    false
                }
            }
        }
    }

    pub fn label(&self) -> String {
        match self {
            FilterCriteria::Extension(ext) => format!("*.{ext}"),
            FilterCriteria::SizeRange { min, max } => {
                match (min, max) {
                    (Some(min), Some(max)) => {
                        format!(
                            "{}-{}",
                            humansize::format_size(*min, humansize::BINARY),
                            humansize::format_size(*max, humansize::BINARY),
                        )
                    }
                    (Some(min), None) => {
                        format!(">{}", humansize::format_size(*min, humansize::BINARY))
                    }
                    (None, Some(max)) => {
                        format!("<{}", humansize::format_size(*max, humansize::BINARY))
                    }
                    (None, None) => "all sizes".into(),
                }
            }
            FilterCriteria::ModifiedWithin(days) => format!("last {days}d"),
        }
    }
}

/// Predefined date filter options.
pub const DATE_PRESETS: &[(u64, &str)] = &[
    (1, "Last 24h"),
    (7, "Last 7 days"),
    (30, "Last 30 days"),
    (365, "Last year"),
];

/// Size filter presets.
pub const SIZE_PRESETS: &[(u64, &str)] = &[
    (1024 * 1024, "> 1 MiB"),
    (10 * 1024 * 1024, "> 10 MiB"),
    (100 * 1024 * 1024, "> 100 MiB"),
    (1024 * 1024 * 1024, "> 1 GiB"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{DiskNode, NodeType};

    #[test]
    fn test_extension_filter() {
        let filter = FilterCriteria::Extension("rs".into());
        let rs_file = DiskNode::new("main.rs".into(), 100, NodeType::File, 1);
        let txt_file = DiskNode::new("readme.txt".into(), 100, NodeType::File, 1);
        let dir = DiskNode::new("src".into(), 1000, NodeType::Dir, 1);

        assert!(filter.matches(&rs_file));
        assert!(!filter.matches(&txt_file));
        assert!(filter.matches(&dir)); // dirs always match
    }

    #[test]
    fn test_size_filter() {
        let filter = FilterCriteria::SizeRange {
            min: Some(50),
            max: Some(200),
        };
        let small = DiskNode::new("small".into(), 10, NodeType::File, 1);
        let medium = DiskNode::new("med".into(), 100, NodeType::File, 1);
        let large = DiskNode::new("large".into(), 500, NodeType::File, 1);

        assert!(!filter.matches(&small));
        assert!(filter.matches(&medium));
        assert!(!filter.matches(&large));
    }

    #[test]
    fn test_modified_filter() {
        let filter = FilterCriteria::ModifiedWithin(7);
        let mut recent = DiskNode::new("recent".into(), 100, NodeType::File, 1);
        recent.modified = Some(SystemTime::now());

        let mut old = DiskNode::new("old".into(), 100, NodeType::File, 1);
        old.modified = Some(SystemTime::UNIX_EPOCH);

        assert!(filter.matches(&recent));
        assert!(!filter.matches(&old));
    }
}
