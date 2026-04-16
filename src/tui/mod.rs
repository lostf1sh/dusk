pub mod filter;
pub mod overlay;
pub mod text;
pub mod theme;
pub mod views;
pub mod widgets;

mod app;
mod browse;
mod render;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crate::config::bookmarks::BookmarkStore;
use crate::model::node::{DiskNode, SortConfig};
use crate::scanner::walker::{ScanUpdate, ScanWarnings};
use crate::tui::filter::FilterCriteria;
use crate::tui::overlay::Overlay;
use crate::tui::theme::Theme;
use crate::tui::views::bar::BarState;
use crate::tui::views::nav::ViewNavState;
use crate::tui::views::tree::{flatten_tree, FlatRow, TreeViewState};
use crate::tui::views::treemap::TreemapState;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Tree,
    Treemap,
    Bar,
}

impl ViewMode {
    fn label(self) -> &'static str {
        match self {
            ViewMode::Tree => "Tree",
            ViewMode::Treemap => "Map",
            ViewMode::Bar => "Bar",
        }
    }

    fn key(self) -> char {
        match self {
            ViewMode::Tree => '1',
            ViewMode::Treemap => '2',
            ViewMode::Bar => '3',
        }
    }

    const ALL: [ViewMode; 3] = [ViewMode::Tree, ViewMode::Treemap, ViewMode::Bar];
}

struct BrowsingState {
    root: Box<DiskNode>,
    tree_state: Box<TreeViewState>,
    flat_rows: Vec<FlatRow>,
    nav: ViewNavState,
    bar_state: BarState,
    treemap_state: TreemapState,
    sort_config: SortConfig,
    active_filter: Option<FilterCriteria>,
}

impl BrowsingState {
    fn from_root(root: DiskNode) -> Self {
        let tree_state = TreeViewState::new();
        let flat_rows = flatten_tree(&root, &tree_state.expanded);

        Self {
            root: Box::new(root),
            tree_state: Box::new(tree_state),
            flat_rows,
            nav: ViewNavState::new(),
            bar_state: BarState::new(),
            treemap_state: TreemapState::new(),
            sort_config: SortConfig::default(),
            active_filter: None,
        }
    }
}

enum AppState {
    Scanning {
        files_found: u64,
        bytes_found: u64,
        spinner_tick: usize,
    },
    Browsing(BrowsingState),
    Error(String),
}

pub struct App {
    state: AppState,
    view_mode: ViewMode,
    overlay: Option<Overlay>,
    scan_rx: Receiver<ScanUpdate>,
    root_path: PathBuf,
    scan_start: Instant,
    total_scan_time: Option<Duration>,
    theme: Theme,
    use_trash: bool,
    bookmarks: BookmarkStore,
    scan_warnings: ScanWarnings,
    startup_notices: Vec<String>,
}
