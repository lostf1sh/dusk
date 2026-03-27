use crate::model::node::DiskNode;
use crate::tui::views::tree::resolve_node;

/// Shared navigation state for non-tree views (bar, treemap, sunburst).
/// Represents "we're looking at the children of `view_root_path`, with
/// `selected_child` highlighted."
pub struct ViewNavState {
    /// Path indices from scan root to the directory we're viewing.
    /// Empty = scan root itself.
    pub view_root_path: Vec<usize>,
    /// Index of the selected child within the view root's children.
    pub selected_child: usize,
}

impl ViewNavState {
    pub fn new() -> Self {
        Self {
            view_root_path: Vec::new(),
            selected_child: 0,
        }
    }

    pub fn move_next(&mut self, child_count: usize) {
        if child_count == 0 {
            return;
        }
        self.selected_child = (self.selected_child + 1).min(child_count - 1);
    }

    pub fn move_prev(&mut self) {
        self.selected_child = self.selected_child.saturating_sub(1);
    }

    /// Drill into the selected child if it's a directory with children.
    pub fn drill_in(&mut self, root: &DiskNode) {
        if let Some(view_node) = self.resolve_view_root(root) {
            if let Some(child) = view_node.children.get(self.selected_child) {
                if child.node_type == crate::model::NodeType::Dir && !child.children.is_empty() {
                    self.view_root_path.push(self.selected_child);
                    self.selected_child = 0;
                }
            }
        }
    }

    /// Drill out to parent directory.
    pub fn drill_out(&mut self) {
        if let Some(idx) = self.view_root_path.pop() {
            self.selected_child = idx;
        }
    }

    /// Get the full path to the selected child.
    pub fn selected_path(&self) -> Vec<usize> {
        let mut path = self.view_root_path.clone();
        path.push(self.selected_child);
        path
    }

    /// Resolve the view root node from the scan root.
    pub fn resolve_view_root<'a>(&self, root: &'a DiskNode) -> Option<&'a DiskNode> {
        if self.view_root_path.is_empty() {
            Some(root)
        } else {
            resolve_node(root, &self.view_root_path)
        }
    }

    /// How many children does the current view root have?
    pub fn child_count(&self, root: &DiskNode) -> usize {
        self.resolve_view_root(root)
            .map(|n| n.children.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{DiskNode, NodeType};

    fn sample_tree() -> DiskNode {
        let mut root = DiskNode::new("root".into(), 511, NodeType::Dir, 0);
        let mut big_dir = DiskNode::new("big_dir".into(), 500, NodeType::Dir, 1);
        big_dir
            .children
            .push(DiskNode::new("a.dat".into(), 300, NodeType::File, 2));
        big_dir
            .children
            .push(DiskNode::new("b.dat".into(), 200, NodeType::File, 2));
        let small_dir = DiskNode::new("small_dir".into(), 10, NodeType::Dir, 1);
        let readme = DiskNode::new("readme.md".into(), 1, NodeType::File, 1);
        root.children.push(big_dir);
        root.children.push(small_dir);
        root.children.push(readme);
        root
    }

    #[test]
    fn test_nav_basic() {
        let tree = sample_tree();
        let mut nav = ViewNavState::new();
        assert_eq!(nav.child_count(&tree), 3);
        assert_eq!(nav.selected_child, 0);

        nav.move_next(3);
        assert_eq!(nav.selected_child, 1);
        nav.move_next(3);
        assert_eq!(nav.selected_child, 2);
        nav.move_next(3); // clamp
        assert_eq!(nav.selected_child, 2);

        nav.move_prev();
        assert_eq!(nav.selected_child, 1);
    }

    #[test]
    fn test_drill_in_out() {
        let tree = sample_tree();
        let mut nav = ViewNavState::new();

        // Drill into big_dir (index 0)
        nav.drill_in(&tree);
        assert_eq!(nav.view_root_path, vec![0]);
        assert_eq!(nav.selected_child, 0);
        assert_eq!(nav.child_count(&tree), 2); // a.dat, b.dat

        // Can't drill into a file
        nav.drill_in(&tree);
        assert_eq!(nav.view_root_path, vec![0]); // unchanged

        // Drill out
        nav.drill_out();
        assert_eq!(nav.view_root_path, Vec::<usize>::new());
        assert_eq!(nav.selected_child, 0); // restored
    }
}
