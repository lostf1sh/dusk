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
}

impl DiskNode {
    pub fn new(name: String, size: u64, node_type: NodeType, depth: usize) -> Self {
        Self {
            name,
            size,
            node_type,
            children: Vec::new(),
            depth,
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
}
