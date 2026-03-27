use std::time::SystemTime;

/// Type of filesystem entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    File,
    Dir,
    Symlink,
}

/// A node in the disk usage tree.
#[derive(Debug, Clone)]
pub struct DiskNode {
    pub name: String,
    pub size: u64,
    pub node_type: NodeType,
    pub children: Vec<DiskNode>,
    pub depth: usize,
    pub modified: Option<SystemTime>,
}

impl DiskNode {
    pub fn new(name: String, size: u64, node_type: NodeType, depth: usize) -> Self {
        Self {
            name,
            size,
            node_type,
            children: Vec::new(),
            depth,
            modified: None,
        }
    }

    /// Total number of file nodes in this subtree (recursive).
    pub fn total_files(&self) -> u64 {
        match self.node_type {
            NodeType::File => 1,
            NodeType::Symlink => 1,
            NodeType::Dir => self.children.iter().map(|c| c.total_files()).sum(),
        }
    }

    /// Total number of directory nodes in this subtree (recursive, excluding self).
    pub fn total_dirs(&self) -> u64 {
        self.children
            .iter()
            .map(|c| {
                let self_count = if c.node_type == NodeType::Dir { 1 } else { 0 };
                self_count + c.total_dirs()
            })
            .sum()
    }

    /// Recursively sort children by size (largest first).
    pub fn sort_children_by_size(&mut self) {
        self.children.sort_by(|a, b| b.size.cmp(&a.size));
        for child in &mut self.children {
            child.sort_children_by_size();
        }
    }

    /// Recursively sort children by the given config.
    pub fn sort_children(&mut self, config: &SortConfig) {
        self.children.sort_by(|a, b| {
            let ord = match config.field {
                SortField::Size => b.size.cmp(&a.size),
                SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortField::Modified => {
                    let a_time = a.modified.unwrap_or(SystemTime::UNIX_EPOCH);
                    let b_time = b.modified.unwrap_or(SystemTime::UNIX_EPOCH);
                    b_time.cmp(&a_time) // newest first by default
                }
                SortField::FileCount => {
                    let a_count = a.total_files();
                    let b_count = b.total_files();
                    b_count.cmp(&a_count) // most files first by default
                }
            };
            if config.ascending {
                ord.reverse()
            } else {
                ord
            }
        });
        for child in &mut self.children {
            child.sort_children(config);
        }
    }

    /// Remove a child node at the given path and subtract its size from ancestors.
    /// Returns the removed node's size.
    pub fn remove_node(&mut self, path_indices: &[usize]) -> Option<u64> {
        if path_indices.is_empty() {
            return None; // can't remove root
        }
        if path_indices.len() == 1 {
            let idx = path_indices[0];
            if idx >= self.children.len() {
                return None;
            }
            let removed_size = self.children[idx].size;
            self.children.remove(idx);
            self.size = self.size.saturating_sub(removed_size);
            return Some(removed_size);
        }
        // Recurse to parent
        let child = self.children.get_mut(path_indices[0])?;
        let removed_size = child.remove_node(&path_indices[1..])?;
        self.size = self.size.saturating_sub(removed_size);
        Some(removed_size)
    }
}

/// Which field to sort by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Size,
    Name,
    Modified,
    FileCount,
}

impl SortField {
    #[allow(dead_code)]
    pub const ALL: [SortField; 4] = [
        SortField::Size,
        SortField::Name,
        SortField::Modified,
        SortField::FileCount,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortField::Size => "Size",
            SortField::Name => "Name",
            SortField::Modified => "Modified",
            SortField::FileCount => "Files",
        }
    }

    pub fn next(self) -> Self {
        match self {
            SortField::Size => SortField::Name,
            SortField::Name => SortField::Modified,
            SortField::Modified => SortField::FileCount,
            SortField::FileCount => SortField::Size,
        }
    }
}

/// Sorting configuration.
#[derive(Debug, Clone, Copy)]
pub struct SortConfig {
    pub field: SortField,
    pub ascending: bool,
}

impl Default for SortConfig {
    fn default() -> Self {
        Self {
            field: SortField::Size,
            ascending: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tree() -> DiskNode {
        let mut root = DiskNode::new("root".into(), 0, NodeType::Dir, 0);

        let mut big_dir = DiskNode::new("big_dir".into(), 0, NodeType::Dir, 1);
        big_dir.children.push(DiskNode::new("a.dat".into(), 300, NodeType::File, 2));
        big_dir.children.push(DiskNode::new("b.dat".into(), 200, NodeType::File, 2));
        big_dir.size = 500;

        let mut small_dir = DiskNode::new("small_dir".into(), 0, NodeType::Dir, 1);
        small_dir.children.push(DiskNode::new("c.txt".into(), 10, NodeType::File, 2));
        small_dir.size = 10;

        let readme = DiskNode::new("readme.md".into(), 1, NodeType::File, 1);

        root.children.push(small_dir);
        root.children.push(readme);
        root.children.push(big_dir);
        root.size = 511;

        root
    }

    #[test]
    fn test_total_files() {
        let tree = sample_tree();
        assert_eq!(tree.total_files(), 4); // a.dat, b.dat, c.txt, readme.md
    }

    #[test]
    fn test_total_dirs() {
        let tree = sample_tree();
        assert_eq!(tree.total_dirs(), 2); // big_dir, small_dir
    }

    #[test]
    fn test_sort_children_by_size() {
        let mut tree = sample_tree();
        tree.sort_children_by_size();

        assert_eq!(tree.children[0].name, "big_dir");
        assert_eq!(tree.children[1].name, "small_dir");
        assert_eq!(tree.children[2].name, "readme.md");

        // Children of big_dir should also be sorted
        assert_eq!(tree.children[0].children[0].name, "a.dat");
        assert_eq!(tree.children[0].children[1].name, "b.dat");
    }

    #[test]
    fn test_sort_by_name() {
        let mut tree = sample_tree();
        let config = SortConfig {
            field: SortField::Name,
            ascending: false,
        };
        tree.sort_children(&config);

        assert_eq!(tree.children[0].name, "big_dir");
        assert_eq!(tree.children[1].name, "readme.md");
        assert_eq!(tree.children[2].name, "small_dir");
    }

    #[test]
    fn test_sort_ascending() {
        let mut tree = sample_tree();
        let config = SortConfig {
            field: SortField::Size,
            ascending: true,
        };
        tree.sort_children(&config);

        assert_eq!(tree.children[0].name, "readme.md"); // smallest first
        assert_eq!(tree.children[2].name, "big_dir"); // largest last
    }

    #[test]
    fn test_remove_node() {
        let mut tree = sample_tree();
        tree.sort_children_by_size();
        // tree.children: [big_dir(500), small_dir(10), readme.md(1)]
        assert_eq!(tree.size, 511);

        // Remove readme.md (index 2)
        let removed = tree.remove_node(&[2]);
        assert_eq!(removed, Some(1));
        assert_eq!(tree.children.len(), 2);
        assert_eq!(tree.size, 510);

        // Remove a.dat from big_dir (path [0, 0])
        let removed = tree.remove_node(&[0, 0]);
        assert_eq!(removed, Some(300));
        assert_eq!(tree.size, 210);
        assert_eq!(tree.children[0].size, 200);
    }
}
