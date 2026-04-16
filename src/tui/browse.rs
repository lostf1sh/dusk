use crossterm::event::{KeyCode, KeyEvent};

use crate::model::node::DiskNode;
use crate::tui::filter::FilterCriteria;
use crate::tui::overlay::SearchEntry;
use crate::tui::views::bar::BarState;
use crate::tui::views::nav::ViewNavState;
use crate::tui::views::tree::{
    filter_visible_child_indices, flatten_tree_filtered, FlatRow, TreeViewState,
};
use crate::tui::views::treemap::{visible_treemap_child_indices, TreemapState};

use super::{BrowsingState, ViewMode};

/// Pre-collect all node names from the tree (done once when search opens).
pub(super) fn collect_all_entries(
    node: &DiskNode,
    path_indices: &mut Vec<usize>,
    name_path: &mut Vec<String>,
    entries: &mut Vec<SearchEntry>,
) {
    for (i, child) in node.children.iter().enumerate() {
        path_indices.push(i);
        name_path.push(child.name.clone());

        entries.push(SearchEntry {
            name: child.name.clone(),
            path_indices: path_indices.clone(),
            name_path: name_path.clone(),
        });

        if !child.children.is_empty() {
            collect_all_entries(child, path_indices, name_path, entries);
        }

        path_indices.pop();
        name_path.pop();
    }
}

/// Get the path_indices of the currently selected node.
pub(super) fn selected_path_from_state(
    view_mode: ViewMode,
    browsing: &BrowsingState,
) -> Option<Vec<usize>> {
    match view_mode {
        ViewMode::Tree => browsing
            .flat_rows
            .get(browsing.tree_state.cursor)
            .map(|row| row.path_indices.clone()),
        ViewMode::Bar | ViewMode::Treemap => browsing.nav.path_indices(&browsing.root),
    }
}

pub(super) fn sync_bar_list_selection(
    nav: &ViewNavState,
    root: &DiskNode,
    bar_state: &mut BarState,
    filter: Option<&FilterCriteria>,
) {
    let Some(view_node) = nav.resolve_view_root(root) else {
        bar_state.list_state.select(None);
        return;
    };

    let visible = filter_visible_child_indices(view_node, filter);
    let pos = visible
        .iter()
        .position(|&i| view_node.children[i].name == nav.selected_name)
        .unwrap_or(0);
    bar_state.sync_selection(pos);
}

pub(super) fn sync_treemap_selection(
    nav: &mut ViewNavState,
    root: &DiskNode,
    filter: Option<&FilterCriteria>,
) {
    let Some(view_node) = nav.resolve_view_root(root) else {
        nav.selected_name.clear();
        return;
    };

    let visible = visible_treemap_child_indices(view_node, filter);
    if visible.is_empty() {
        nav.selected_name.clear();
    } else if !visible
        .iter()
        .any(|&i| view_node.children[i].name == nav.selected_name)
    {
        nav.selected_name = view_node.children[visible[0]].name.clone();
    }
}

pub(super) fn handle_tree_key(
    key: KeyEvent,
    root: &DiskNode,
    tree_state: &mut TreeViewState,
    flat_rows: &mut Vec<FlatRow>,
    active_filter: Option<&FilterCriteria>,
) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => tree_state.move_down(flat_rows.len()),
        KeyCode::Char('k') | KeyCode::Up => tree_state.move_up(),
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            tree_state.drill_in(flat_rows);
            *flat_rows = flatten_tree_filtered(root, &tree_state.expanded, active_filter);
        }
        KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => {
            tree_state.drill_out(flat_rows);
            *flat_rows = flatten_tree_filtered(root, &tree_state.expanded, active_filter);
        }
        KeyCode::Char(' ') => {
            tree_state.toggle_expand(flat_rows);
            *flat_rows = flatten_tree_filtered(root, &tree_state.expanded, active_filter);
        }
        _ => {}
    }
}

pub(super) fn handle_nav_key(
    key: KeyEvent,
    nav: &mut ViewNavState,
    root: &DiskNode,
    filter: Option<&FilterCriteria>,
) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => nav.move_next(root, filter),
        KeyCode::Char('k') | KeyCode::Up => nav.move_prev(root, filter),
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => nav.drill_in(root, filter),
        KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => nav.drill_out(root, filter),
        _ => {}
    }
}

pub(super) fn handle_treemap_key(
    key: KeyEvent,
    nav: &mut ViewNavState,
    root: &DiskNode,
    treemap_state: &mut TreemapState,
    filter: Option<&FilterCriteria>,
) {
    sync_treemap_selection(nav, root, filter);
    let Some(view_node) = nav.resolve_view_root(root) else {
        return;
    };
    let selected_idx = view_node
        .children
        .iter()
        .position(|child| child.name == nav.selected_name)
        .unwrap_or(0);

    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            let new_idx = treemap_state.navigate(selected_idx, 0, 1);
            if let Some(child) = view_node.children.get(new_idx) {
                nav.selected_name = child.name.clone();
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let new_idx = treemap_state.navigate(selected_idx, 0, -1);
            if let Some(child) = view_node.children.get(new_idx) {
                nav.selected_name = child.name.clone();
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            let new_idx = treemap_state.navigate(selected_idx, 1, 0);
            if let Some(child) = view_node.children.get(new_idx) {
                nav.selected_name = child.name.clone();
            }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            let new_idx = treemap_state.navigate(selected_idx, -1, 0);
            if let Some(child) = view_node.children.get(new_idx) {
                nav.selected_name = child.name.clone();
            }
        }
        KeyCode::Enter => {
            nav.drill_in(root, filter);
            treemap_state.invalidate();
        }
        KeyCode::Backspace => {
            nav.drill_out(root, filter);
            treemap_state.invalidate();
        }
        _ => {}
    }
}
