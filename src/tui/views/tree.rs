use std::collections::HashSet;

use humansize::{format_size, BINARY};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, StatefulWidget};

use crate::model::node::{DiskNode, NodeType};
use crate::tui::filter::FilterCriteria;
use crate::tui::text::display_width;
use crate::tui::theme::Theme;

/// A single visible row in the flattened tree.
#[derive(Debug, Clone)]
pub struct FlatRow {
    pub depth: usize,
    pub name: String,
    pub size: u64,
    pub node_type: NodeType,
    pub has_children: bool,
    pub is_expanded: bool,
    /// Index path from root to this node (e.g., [0, 2, 1] = root.children[0].children[2].children[1]).
    pub path_indices: Vec<usize>,
    /// Name path from root to this node (stable across sort/delete).
    pub name_path: Vec<String>,
}

/// Navigation and display state for the tree view.
pub struct TreeViewState {
    pub cursor: usize,
    pub expanded: HashSet<Vec<String>>,
    pub list_state: ListState,
}

impl Default for TreeViewState {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeViewState {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            cursor: 0,
            expanded: HashSet::new(),
            list_state,
        }
    }

    pub fn move_down(&mut self, row_count: usize) {
        if row_count == 0 {
            return;
        }
        self.cursor = (self.cursor + 1).min(row_count - 1);
        self.list_state.select(Some(self.cursor));
    }

    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        self.list_state.select(Some(self.cursor));
    }

    /// Toggle expand/collapse for the directory at cursor.
    pub fn toggle_expand(&mut self, rows: &[FlatRow]) {
        if let Some(row) = rows.get(self.cursor) {
            if row.node_type == NodeType::Dir && row.has_children {
                let name_path = row.name_path.clone();
                if !self.expanded.remove(&name_path) {
                    self.expanded.insert(name_path);
                }
            }
        }
    }

    /// Expand the directory at cursor and move cursor to its first child.
    pub fn drill_in(&mut self, rows: &[FlatRow]) {
        if let Some(row) = rows.get(self.cursor) {
            if row.node_type == NodeType::Dir && row.has_children {
                let name_path = row.name_path.clone();
                self.expanded.insert(name_path);
                // After re-flatten, the first child will be at cursor + 1
                self.cursor += 1;
                self.list_state.select(Some(self.cursor));
            }
        }
    }

    /// Collapse current directory or move to parent.
    pub fn drill_out(&mut self, rows: &[FlatRow]) {
        if let Some(row) = rows.get(self.cursor) {
            // If we're on an expanded dir, collapse it
            if row.node_type == NodeType::Dir && self.expanded.contains(&row.name_path) {
                self.expanded.remove(&row.name_path);
                return;
            }

            // Otherwise, find and move to parent
            if row.name_path.len() > 1 {
                let parent_name_path: Vec<String> =
                    row.name_path[..row.name_path.len() - 1].to_vec();
                // Find the parent row
                for (i, r) in rows.iter().enumerate() {
                    if r.name_path == parent_name_path {
                        self.cursor = i;
                        self.list_state.select(Some(self.cursor));
                        return;
                    }
                }
            }
        }
    }

    /// Clamp cursor to valid range after tree mutation.
    pub fn clamp_cursor(&mut self, row_count: usize) {
        if row_count == 0 {
            self.cursor = 0;
        } else if self.cursor >= row_count {
            self.cursor = row_count - 1;
        }
        self.list_state.select(Some(self.cursor));
    }
}

/// Flatten the tree into visible rows based on which directories are expanded.
pub fn flatten_tree(root: &DiskNode, expanded: &HashSet<Vec<String>>) -> Vec<FlatRow> {
    flatten_tree_filtered(root, expanded, None)
}

/// Flatten the tree with optional filter.
pub fn flatten_tree_filtered(
    root: &DiskNode,
    expanded: &HashSet<Vec<String>>,
    filter: Option<&FilterCriteria>,
) -> Vec<FlatRow> {
    let mut rows = Vec::new();
    flatten_children(
        &root.children,
        expanded,
        filter,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut rows,
    );
    rows
}

fn flatten_children(
    children: &[DiskNode],
    expanded: &HashSet<Vec<String>>,
    filter: Option<&FilterCriteria>,
    parent_indices: &mut Vec<usize>,
    parent_names: &mut Vec<String>,
    rows: &mut Vec<FlatRow>,
) {
    for (i, child) in children.iter().enumerate() {
        // Apply filter: skip non-matching files, but always show dirs
        if let Some(f) = filter {
            if !f.matches(child) {
                continue;
            }
            // For directories: skip if no descendants match
            if child.node_type == NodeType::Dir && !has_matching_descendant(child, f) {
                continue;
            }
        }

        parent_indices.push(i);
        parent_names.push(child.name.clone());

        let path_indices = parent_indices.clone();
        let name_path = parent_names.clone();
        let is_expanded = expanded.contains(&name_path);

        rows.push(FlatRow {
            depth: child.depth,
            name: child.name.clone(),
            size: child.size,
            node_type: child.node_type.clone(),
            has_children: !child.children.is_empty(),
            is_expanded,
            path_indices,
            name_path,
        });

        if is_expanded {
            flatten_children(
                &child.children,
                expanded,
                filter,
                parent_indices,
                parent_names,
                rows,
            );
        }

        parent_indices.pop();
        parent_names.pop();
    }
}

/// Child indices of `node` that should appear in bar/treemap under the active filter
/// (same rules as the tree view).
pub fn filter_visible_child_indices(
    node: &DiskNode,
    filter: Option<&FilterCriteria>,
) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, child) in node.children.iter().enumerate() {
        if let Some(f) = filter {
            if !f.matches(child) {
                continue;
            }
            if child.node_type == NodeType::Dir && !has_matching_descendant(child, f) {
                continue;
            }
        }
        out.push(i);
    }
    out
}

/// Check if a directory has any descendant that matches the filter.
fn has_matching_descendant(node: &DiskNode, filter: &FilterCriteria) -> bool {
    for child in &node.children {
        if child.node_type != NodeType::Dir && filter.matches(child) {
            return true;
        }
        if child.node_type == NodeType::Dir && has_matching_descendant(child, filter) {
            return true;
        }
    }
    false
}

/// The tree view widget. Renders the flat rows as a navigable list.
pub struct TreeView<'a> {
    pub rows: &'a [FlatRow],
    pub root_size: u64,
    pub theme: &'a Theme,
}

impl StatefulWidget for TreeView<'_> {
    type State = ListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|row| {
                let indent = "  ".repeat(row.depth.saturating_sub(1));
                let icon = match (&row.node_type, row.has_children) {
                    (NodeType::Dir, true) if row.is_expanded => "▼ ",
                    (NodeType::Dir, true) => "▶ ",
                    (NodeType::Dir, false) => "▶ ",
                    (NodeType::Symlink, _) => "→ ",
                    (NodeType::File, _) => "  ",
                };

                let name_style = self.theme.node_style(&row.node_type);
                let size_str = format_size(row.size, BINARY);
                let size_style = self.theme.size_style(row.size, self.root_size);
                let rendered_name = format!("{indent}{icon}{}", row.name);

                let name_span = Span::styled(rendered_name.clone(), name_style);

                // Calculate padding to right-align size
                let used = display_width(&rendered_name);
                let available = area.width as usize;
                let size_len = size_str.len();
                let padding = if used + size_len + 1 < available {
                    available - used - size_len
                } else {
                    1
                };

                let line = Line::from(vec![
                    name_span,
                    Span::raw(" ".repeat(padding)),
                    Span::styled(size_str, size_style),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(self.theme.selected_style)
            .highlight_symbol("│");

        StatefulWidget::render(list, area, buf, state);
    }
}

/// Resolve a path_indices to the actual DiskNode reference.
pub fn resolve_node<'a>(root: &'a DiskNode, path: &[usize]) -> Option<&'a DiskNode> {
    let mut node = root;
    for &idx in path {
        node = node.children.get(idx)?;
    }
    Some(node)
}

/// Resolve a path_indices to a mutable DiskNode reference.
#[allow(dead_code)]
pub fn resolve_node_mut<'a>(root: &'a mut DiskNode, path: &[usize]) -> Option<&'a mut DiskNode> {
    let mut node = root;
    for &idx in path {
        node = node.children.get_mut(idx)?;
    }
    Some(node)
}

/// Build the absolute filesystem path for a node identified by path_indices.
pub fn resolve_fs_path(
    root_path: &std::path::Path,
    root: &DiskNode,
    path_indices: &[usize],
) -> Option<std::path::PathBuf> {
    let mut fs_path = root_path.to_path_buf();
    let mut node = root;
    for &idx in path_indices {
        node = node.children.get(idx)?;
        fs_path.push(&node.name);
    }
    Some(fs_path)
}

/// Resolve a filesystem path from a chain of child names under the scan root (stable across sort).
pub fn resolve_fs_path_by_name_path(
    root_path: &std::path::Path,
    root: &DiskNode,
    name_path: &[String],
) -> Option<std::path::PathBuf> {
    let mut fs_path = root_path.to_path_buf();
    let mut node = root;
    for name in name_path {
        let idx = node.children.iter().position(|c| c.name == *name)?;
        node = &node.children[idx];
        fs_path.push(name);
    }
    Some(fs_path)
}

/// Path indices for a view directory plus selected child name (stable across sort).
pub fn path_indices_for_named_selection(
    root: &DiskNode,
    view_dir_name_path: &[String],
    selected_name: &str,
) -> Option<Vec<usize>> {
    if selected_name.is_empty() {
        return None;
    }
    let mut indices = Vec::new();
    let mut node = root;
    for name in view_dir_name_path {
        let idx = node.children.iter().position(|c| c.name == *name)?;
        indices.push(idx);
        node = &node.children[idx];
    }
    let idx = node.children.iter().position(|c| c.name == selected_name)?;
    indices.push(idx);
    Some(indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::DiskNode;

    fn sample_tree() -> DiskNode {
        let mut root = DiskNode::new("root".into(), 511, NodeType::Dir, 0);

        let mut big_dir = DiskNode::new("big_dir".into(), 500, NodeType::Dir, 1);
        big_dir
            .children
            .push(DiskNode::new("a.dat".into(), 300, NodeType::File, 2));
        big_dir
            .children
            .push(DiskNode::new("b.dat".into(), 200, NodeType::File, 2));

        let mut small_dir = DiskNode::new("small_dir".into(), 10, NodeType::Dir, 1);
        small_dir
            .children
            .push(DiskNode::new("c.txt".into(), 10, NodeType::File, 2));

        let readme = DiskNode::new("readme.md".into(), 1, NodeType::File, 1);

        root.children.push(big_dir);
        root.children.push(small_dir);
        root.children.push(readme);
        root
    }

    #[test]
    fn test_flatten_collapsed() {
        let tree = sample_tree();
        let expanded = HashSet::new();
        let rows = flatten_tree(&tree, &expanded);

        assert_eq!(rows.len(), 3); // big_dir, small_dir, readme.md
        assert_eq!(rows[0].name, "big_dir");
        assert!(!rows[0].is_expanded);
        assert_eq!(rows[0].name_path, vec!["big_dir"]);
        assert_eq!(rows[1].name, "small_dir");
        assert_eq!(rows[2].name, "readme.md");
    }

    #[test]
    fn test_flatten_expanded() {
        let tree = sample_tree();
        let mut expanded = HashSet::new();
        expanded.insert(vec!["big_dir".to_string()]); // expand big_dir by name

        let rows = flatten_tree(&tree, &expanded);

        assert_eq!(rows.len(), 5); // big_dir, a.dat, b.dat, small_dir, readme.md
        assert_eq!(rows[0].name, "big_dir");
        assert!(rows[0].is_expanded);
        assert_eq!(rows[1].name, "a.dat");
        assert_eq!(rows[1].depth, 2);
        assert_eq!(
            rows[1].name_path,
            vec!["big_dir".to_string(), "a.dat".to_string()]
        );
        assert_eq!(rows[2].name, "b.dat");
        assert_eq!(rows[3].name, "small_dir");
    }

    #[test]
    fn test_navigation() {
        let tree = sample_tree();
        let expanded = HashSet::new();
        let rows = flatten_tree(&tree, &expanded);
        let mut state = TreeViewState::new();

        assert_eq!(state.cursor, 0);
        state.move_down(rows.len());
        assert_eq!(state.cursor, 1);
        state.move_down(rows.len());
        assert_eq!(state.cursor, 2);
        state.move_down(rows.len()); // at end, should clamp
        assert_eq!(state.cursor, 2);
        state.move_up();
        assert_eq!(state.cursor, 1);
        state.move_up();
        assert_eq!(state.cursor, 0);
        state.move_up(); // at start, should clamp
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn test_drill_in_out() {
        let tree = sample_tree();
        let mut state = TreeViewState::new();

        let rows = flatten_tree(&tree, &state.expanded);
        assert_eq!(rows.len(), 3);

        // Drill into big_dir (index 0)
        state.drill_in(&rows);
        let rows = flatten_tree(&tree, &state.expanded);
        assert_eq!(rows.len(), 5);
        assert_eq!(state.cursor, 1); // moved to first child

        // Drill out from child
        state.drill_out(&rows);
        assert_eq!(state.cursor, 0); // back to big_dir
    }

    #[test]
    fn test_resolve_node() {
        let tree = sample_tree();
        let node = resolve_node(&tree, &[0]).unwrap();
        assert_eq!(node.name, "big_dir");

        let node = resolve_node(&tree, &[0, 1]).unwrap();
        assert_eq!(node.name, "b.dat");

        assert!(resolve_node(&tree, &[5]).is_none());
    }

    #[test]
    fn test_resolve_fs_path() {
        let tree = sample_tree();
        let root_path = std::path::Path::new("/tmp/test");
        let fs_path = resolve_fs_path(root_path, &tree, &[0, 1]).unwrap();
        assert_eq!(fs_path, std::path::PathBuf::from("/tmp/test/big_dir/b.dat"));
    }

    #[test]
    fn test_clamp_cursor() {
        let mut state = TreeViewState::new();
        state.cursor = 10;
        state.clamp_cursor(3);
        assert_eq!(state.cursor, 2);

        state.clamp_cursor(0);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn test_resolve_fs_path_with_unicode_name() {
        let mut tree = DiskNode::new("root".into(), 10, NodeType::Dir, 0);
        tree.children
            .push(DiskNode::new("日本語.txt".into(), 10, NodeType::File, 1));

        let root_path = std::path::Path::new("/tmp/test");
        let fs_path = resolve_fs_path(root_path, &tree, &[0]).unwrap();
        assert_eq!(fs_path, std::path::PathBuf::from("/tmp/test/日本語.txt"));
    }
}
