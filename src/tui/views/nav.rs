use crate::model::node::DiskNode;
use crate::model::NodeType;
use crate::tui::filter::FilterCriteria;
use crate::tui::views::tree::{filter_visible_child_indices, path_indices_for_named_selection};

/// Shared navigation state for non-tree views (bar, treemap, sunburst).
/// Uses stable name paths so sort/delete do not retarget selection.
pub struct ViewNavState {
    /// Path of directory names from the scan root down to the viewed directory (empty = root).
    pub view_dir_name_path: Vec<String>,
    /// Highlighted child name under the current view root.
    pub selected_name: String,
}

impl ViewNavState {
    pub fn new() -> Self {
        Self {
            view_dir_name_path: Vec::new(),
            selected_name: String::new(),
        }
    }

    /// Keep selection valid after filter changes, tree mutation, or first paint.
    pub fn ensure_valid_selection(&mut self, root: &DiskNode, filter: Option<&FilterCriteria>) {
        if self.resolve_view_root(root).is_none() {
            self.view_dir_name_path.clear();
            self.selected_name.clear();
            return;
        }
        let Some(view_node) = self.resolve_view_root(root) else {
            self.selected_name.clear();
            return;
        };
        let visible = filter_visible_child_indices(view_node, filter);
        let names: Vec<String> = visible
            .iter()
            .map(|&i| view_node.children[i].name.clone())
            .collect();
        if names.is_empty() {
            self.selected_name.clear();
        } else if !names.iter().any(|n| n == &self.selected_name) {
            self.selected_name = names[0].clone();
        }
    }

    pub fn move_next(&mut self, root: &DiskNode, filter: Option<&FilterCriteria>) {
        let Some(view_node) = self.resolve_view_root(root) else {
            return;
        };
        let visible = filter_visible_child_indices(view_node, filter);
        let names: Vec<String> = visible
            .iter()
            .map(|&i| view_node.children[i].name.clone())
            .collect();
        if names.is_empty() {
            return;
        }
        let pos = names
            .iter()
            .position(|n| n == &self.selected_name)
            .unwrap_or(0);
        let next = (pos + 1).min(names.len() - 1);
        self.selected_name = names[next].clone();
    }

    pub fn move_prev(&mut self, root: &DiskNode, filter: Option<&FilterCriteria>) {
        let Some(view_node) = self.resolve_view_root(root) else {
            return;
        };
        let visible = filter_visible_child_indices(view_node, filter);
        let names: Vec<String> = visible
            .iter()
            .map(|&i| view_node.children[i].name.clone())
            .collect();
        if names.is_empty() {
            return;
        }
        let pos = names
            .iter()
            .position(|n| n == &self.selected_name)
            .unwrap_or(0);
        let prev = pos.saturating_sub(1);
        self.selected_name = names[prev].clone();
    }

    /// Drill into the selected child if it's a directory with children.
    pub fn drill_in(&mut self, root: &DiskNode, filter: Option<&FilterCriteria>) {
        let Some(view_node) = self.resolve_view_root(root) else {
            return;
        };
        let visible = filter_visible_child_indices(view_node, filter);
        let Some(&idx) = visible
            .iter()
            .find(|&&i| view_node.children[i].name == self.selected_name)
        else {
            return;
        };
        let child = &view_node.children[idx];
        if child.node_type == NodeType::Dir && !child.children.is_empty() {
            self.view_dir_name_path.push(child.name.clone());
            self.pick_first_visible_child(child, filter);
        }
    }

    fn pick_first_visible_child(&mut self, node: &DiskNode, filter: Option<&FilterCriteria>) {
        let visible = filter_visible_child_indices(node, filter);
        if let Some(&i) = visible.first() {
            self.selected_name = node.children[i].name.clone();
        } else {
            self.selected_name.clear();
        }
    }

    /// Drill out to parent directory.
    pub fn drill_out(&mut self, root: &DiskNode, filter: Option<&FilterCriteria>) {
        if let Some(popped) = self.view_dir_name_path.pop() {
            self.selected_name = popped;
            self.ensure_valid_selection(root, filter);
        }
    }

    /// Path indices from scan root to the selected node (for FS ops, info, delete).
    pub fn path_indices(&self, root: &DiskNode) -> Option<Vec<usize>> {
        path_indices_for_named_selection(
            root,
            &self.view_dir_name_path,
            self.selected_name.as_str(),
        )
    }

    /// Resolve the view root node from the scan root.
    pub fn resolve_view_root<'a>(&self, root: &'a DiskNode) -> Option<&'a DiskNode> {
        let mut node = root;
        for name in &self.view_dir_name_path {
            let idx = node.children.iter().position(|c| c.name == *name)?;
            node = &node.children[idx];
        }
        Some(node)
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
        nav.ensure_valid_selection(&tree, None);
        assert_eq!(
            filter_visible_child_indices(nav.resolve_view_root(&tree).unwrap(), None).len(),
            3
        );
        assert_eq!(nav.selected_name, "big_dir");

        nav.move_next(&tree, None);
        assert_eq!(nav.selected_name, "small_dir");
        nav.move_next(&tree, None);
        assert_eq!(nav.selected_name, "readme.md");
        nav.move_next(&tree, None);
        assert_eq!(nav.selected_name, "readme.md");

        nav.move_prev(&tree, None);
        assert_eq!(nav.selected_name, "small_dir");
    }

    #[test]
    fn test_drill_in_out() {
        let tree = sample_tree();
        let mut nav = ViewNavState::new();
        nav.ensure_valid_selection(&tree, None);
        assert_eq!(nav.selected_name, "big_dir");

        nav.drill_in(&tree, None);
        assert_eq!(nav.view_dir_name_path, vec!["big_dir".to_string()]);
        assert_eq!(nav.selected_name, "a.dat");
        assert_eq!(
            filter_visible_child_indices(nav.resolve_view_root(&tree).unwrap(), None).len(),
            2
        );

        nav.drill_in(&tree, None);
        assert_eq!(nav.view_dir_name_path, vec!["big_dir".to_string()]);

        nav.drill_out(&tree, None);
        assert_eq!(nav.view_dir_name_path, Vec::<String>::new());
        assert_eq!(nav.selected_name, "big_dir");
    }
}
