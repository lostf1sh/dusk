use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::config::bookmarks::BookmarkStore;
use crate::model::node::{DiskNode, NodeType, SortConfig};
use crate::scanner::walker::ScanWarnings;
use crate::tui::overlay::Overlay;
use crate::tui::theme::Theme;
use crate::tui::views::bar::BarState;
use crate::tui::views::nav::ViewNavState;
use crate::tui::views::tree::{flatten_tree, TreeViewState};
use crate::tui::views::treemap::TreemapState;

use super::{App, AppState, BrowsingState, ViewMode};

fn sample_tree() -> DiskNode {
    let mut root = DiskNode::new("root".into(), 11, NodeType::Dir, 0);
    let mut src = DiskNode::new("src".into(), 10, NodeType::Dir, 1);
    src.children
        .push(DiskNode::new("main.rs".into(), 10, NodeType::File, 2));
    let readme = DiskNode::new("README.md".into(), 1, NodeType::File, 1);
    root.children.push(src);
    root.children.push(readme);
    root
}

fn browsing_app(root_path: PathBuf) -> App {
    let root = sample_tree();
    let tree_state = TreeViewState::new();
    let flat_rows = flatten_tree(&root, &tree_state.expanded);
    let (_tx, rx) = mpsc::channel();

    App {
        state: AppState::Browsing(BrowsingState {
            root: Box::new(root),
            tree_state: Box::new(tree_state),
            flat_rows,
            nav: ViewNavState::new(),
            bar_state: BarState::new(),
            treemap_state: TreemapState::new(),
            sort_config: SortConfig::default(),
            active_filter: None,
        }),
        view_mode: ViewMode::Tree,
        overlay: None,
        scan_rx: rx,
        root_path,
        scan_start: Instant::now(),
        total_scan_time: Some(Duration::from_secs(1)),
        theme: Theme::default(),
        use_trash: true,
        bookmarks: BookmarkStore::default(),
        scan_warnings: ScanWarnings::default(),
        startup_notices: Vec::new(),
    }
}

#[test]
fn test_jump_to_root_bookmark_resets_navigation() {
    let root_path = PathBuf::from("/tmp/project");
    let mut app = browsing_app(root_path.clone());

    if let AppState::Browsing(browsing) = &mut app.state {
        browsing.tree_state.cursor = browsing.flat_rows.len().saturating_sub(1);
        browsing
            .tree_state
            .list_state
            .select((!browsing.flat_rows.is_empty()).then_some(browsing.tree_state.cursor));
        browsing.nav.view_dir_name_path = vec!["src".into()];
        browsing.nav.selected_name = "main.rs".into();
    }

    app.jump_to_bookmark(&root_path);

    if let AppState::Browsing(browsing) = &app.state {
        assert_eq!(browsing.tree_state.cursor, 0);
        assert_eq!(
            browsing.tree_state.list_state.selected(),
            (!browsing.flat_rows.is_empty()).then_some(0)
        );
        assert!(browsing.nav.view_dir_name_path.is_empty());
        assert_eq!(browsing.nav.selected_name, "src");
    } else {
        panic!("expected browsing state");
    }
}

#[test]
fn test_flush_notices_shows_flash_overlay() {
    let mut app = browsing_app(PathBuf::from("/tmp/project"));
    app.startup_notices = vec!["first".into(), "second".into()];

    app.flush_notices();

    match app.overlay {
        Some(Overlay::Flash { message }) => assert_eq!(message, "first | second"),
        _ => panic!("expected flash overlay"),
    }
    assert!(app.startup_notices.is_empty());
}
