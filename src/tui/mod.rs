pub mod filter;
pub mod overlay;
pub mod theme;
pub mod views;
pub mod widgets;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use humansize::{format_size, BINARY};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, ListState, Paragraph};
use ratatui::Frame;

use crate::config::bookmarks::BookmarkStore;
use crate::model::metadata::load_metadata;
use crate::model::node::{DiskNode, SortConfig};
use crate::tui::filter::FilterCriteria;
use crate::scanner::walker::ScanUpdate;
use crate::tui::overlay::{
    handle_overlay_key, render_overlay, Overlay, OverlayAction, SearchEntry, SearchResult,
};
use crate::tui::widgets::text_input::TextInputState;
use crate::tui::theme::Theme;
use crate::tui::views::bar::{BarState, BarView};
use crate::tui::views::nav::ViewNavState;
use crate::tui::views::tree::{
    flatten_tree, flatten_tree_filtered, resolve_fs_path, resolve_node, FlatRow, TreeView,
    TreeViewState,
};
use crate::tui::views::treemap::{TreemapState, TreemapView};
use crate::tui::widgets::progress::ScanProgress;

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

enum AppState {
    Scanning {
        files_found: u64,
        bytes_found: u64,
        spinner_tick: usize,
    },
    Browsing {
        root: Box<DiskNode>,
        // Tree view state
        tree_state: Box<TreeViewState>,
        flat_rows: Vec<FlatRow>,
        // Shared nav for non-tree views
        nav: ViewNavState,
        // Per-view state
        bar_state: BarState,
        treemap_state: TreemapState,
        // Sort configuration
        sort_config: SortConfig,
        // Active filter
        active_filter: Option<FilterCriteria>,
    },
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
}

impl App {
    pub fn new(root_path: PathBuf, scan_rx: Receiver<ScanUpdate>, use_trash: bool) -> Self {
        Self {
            state: AppState::Scanning {
                files_found: 0,
                bytes_found: 0,
                spinner_tick: 0,
            },
            view_mode: ViewMode::Tree,
            overlay: None,
            scan_rx,
            root_path,
            scan_start: Instant::now(),
            total_scan_time: None,
            theme: Theme::default(),
            use_trash,
            bookmarks: BookmarkStore::load(),
        }
    }

    pub fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> anyhow::Result<()> {
        loop {
            self.process_scan_updates();

            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press && self.handle_key(key) {
                        break;
                    }
                }
            }

            // Advance spinner
            if let AppState::Scanning { spinner_tick, .. } = &mut self.state {
                *spinner_tick = spinner_tick.wrapping_add(1);
            }
        }
        Ok(())
    }

    fn process_scan_updates(&mut self) {
        loop {
            match self.scan_rx.try_recv() {
                Ok(ScanUpdate::Progress {
                    files_found,
                    bytes_found,
                }) => {
                    if let AppState::Scanning {
                        files_found: f,
                        bytes_found: b,
                        ..
                    } = &mut self.state
                    {
                        *f = files_found;
                        *b = bytes_found;
                    }
                }
                Ok(ScanUpdate::Complete(root)) => {
                    self.total_scan_time = Some(self.scan_start.elapsed());
                    let tree_state = TreeViewState::new();
                    let flat_rows = flatten_tree(&root, &tree_state.expanded);
                    self.state = AppState::Browsing {
                        root: Box::new(root),
                        tree_state: Box::new(tree_state),
                        flat_rows,
                        nav: ViewNavState::new(),
                        bar_state: BarState::new(),
                        treemap_state: TreemapState::new(),
                        sort_config: SortConfig::default(),
                        active_filter: None,
                    };
                }
                Ok(ScanUpdate::Error(msg)) => {
                    self.state = AppState::Error(msg);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if matches!(self.state, AppState::Scanning { .. }) {
                        self.state =
                            AppState::Error("Scanner terminated unexpectedly".to_string());
                    }
                    break;
                }
            }
        }
    }

    /// Returns true if the app should quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Overlays consume input first
        if let Some(ref mut overlay) = self.overlay {
            let action = handle_overlay_key(overlay, key);
            match action {
                OverlayAction::Consumed => return false,
                OverlayAction::Close => {
                    self.overlay = None;
                    return false;
                }
                OverlayAction::ConfirmDelete {
                    path_indices,
                    fs_path,
                    is_dir,
                } => {
                    self.overlay = None;
                    self.execute_delete(&path_indices, &fs_path, is_dir);
                    return false;
                }
                OverlayAction::BookmarkJump(path) => {
                    self.overlay = None;
                    self.jump_to_bookmark(&path);
                    return false;
                }
                OverlayAction::BookmarkRemove(index) => {
                    self.bookmarks.remove(index);
                    let _ = self.bookmarks.save();
                    // Refresh the overlay
                    let len = self.bookmarks.len();
                    if len == 0 {
                        self.overlay = None;
                    } else if let Some(Overlay::BookmarkList {
                        bookmarks,
                        selected,
                        list_state,
                    }) = &mut self.overlay
                    {
                        *bookmarks = self.bookmarks.bookmarks.clone();
                        if *selected >= len {
                            *selected = len - 1;
                        }
                        list_state.select(Some(*selected));
                    }
                    return false;
                }
                OverlayAction::SearchUpdate => {
                    self.update_search_results();
                    return false;
                }
                OverlayAction::SearchJump {
                    path_indices,
                    name_path,
                } => {
                    self.overlay = None;
                    self.jump_to_search_result(&path_indices, &name_path);
                    return false;
                }
                OverlayAction::ApplyFilter(criteria) => {
                    self.overlay = None;
                    self.apply_filter(Some(criteria));
                    return false;
                }
                OverlayAction::ClearFilter => {
                    self.overlay = None;
                    self.apply_filter(None);
                    return false;
                }
                OverlayAction::SwitchToExtFilter => {
                    self.overlay = Some(Overlay::FilterExtInput {
                        input: TextInputState::new(),
                    });
                    return false;
                }
            }
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            _ => {}
        }

        // View switching (available in browsing state)
        if matches!(self.state, AppState::Browsing { .. }) {
            match key.code {
                KeyCode::Char('1') => {
                    self.view_mode = ViewMode::Tree;
                    return false;
                }
                KeyCode::Char('2') => {
                    self.switch_to_nav_view(ViewMode::Treemap);
                    return false;
                }
                KeyCode::Char('3') => {
                    self.switch_to_nav_view(ViewMode::Bar);
                    return false;
                }
                _ => {}
            }
        }

        if let AppState::Browsing {
            root,
            tree_state,
            flat_rows,
            nav,
            bar_state,
            treemap_state,
            sort_config,
            active_filter,
        } = &mut self.state
        {
            // Global browsing keys
            match key.code {
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    sort_config.ascending = !sort_config.ascending;
                    root.sort_children(sort_config);
                    *flat_rows = flatten_tree_filtered(root, &tree_state.expanded, active_filter.as_ref());
                    treemap_state.invalidate();
                    bar_state.sync_selection(nav.selected_child);
                    return false;
                }
                KeyCode::Char('s') => {
                    sort_config.field = sort_config.field.next();
                    root.sort_children(sort_config);
                    *flat_rows = flatten_tree_filtered(root, &tree_state.expanded, active_filter.as_ref());
                    treemap_state.invalidate();
                    bar_state.sync_selection(nav.selected_child);
                    return false;
                }
                KeyCode::Char('i') => {
                    let path_indices = selected_path_from_state(
                        self.view_mode,
                        flat_rows,
                        tree_state,
                        nav,
                    );
                    if let Some(path_indices) = path_indices {
                        if let Some(fs_path) =
                            resolve_fs_path(&self.root_path, root, &path_indices)
                        {
                            let node_name = resolve_node(root, &path_indices)
                                .map(|n| n.name.clone())
                                .unwrap_or_default();
                            match load_metadata(&fs_path) {
                                Ok(meta) => {
                                    self.overlay = Some(Overlay::FileInfo {
                                        lines: meta.to_lines(&node_name),
                                    });
                                }
                                Err(e) => {
                                    self.overlay = Some(Overlay::Flash {
                                        message: format!("Cannot read info: {e}"),
                                    });
                                }
                            }
                        }
                    }
                    return false;
                }
                KeyCode::Char('d') => {
                    let path_indices = selected_path_from_state(
                        self.view_mode,
                        flat_rows,
                        tree_state,
                        nav,
                    );
                    if let Some(path_indices) = path_indices {
                        if let Some(fs_path) =
                            resolve_fs_path(&self.root_path, root, &path_indices)
                        {
                            if let Some(node) = resolve_node(root, &path_indices) {
                                self.overlay = Some(Overlay::DeleteConfirm {
                                    node_name: node.name.clone(),
                                    path_indices: path_indices.clone(),
                                    fs_path,
                                    is_dir: node.node_type == crate::model::NodeType::Dir,
                                    size: node.size,
                                });
                            }
                        }
                    }
                    return false;
                }
                KeyCode::Char('/') => {
                    // Pre-collect all node names once
                    let mut all_entries = Vec::new();
                    collect_all_entries(root, &mut Vec::new(), &mut Vec::new(), &mut all_entries);
                    self.overlay = Some(Overlay::Search {
                        input: TextInputState::new(),
                        all_entries,
                        results: Vec::new(),
                        selected: 0,
                        list_state: ListState::default(),
                    });
                    return false;
                }
                KeyCode::Char('f') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.overlay = Some(Overlay::FilterMenu);
                    return false;
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    // B = open bookmark list
                    let mut list_state = ListState::default();
                    if !self.bookmarks.is_empty() {
                        list_state.select(Some(0));
                    }
                    self.overlay = Some(Overlay::BookmarkList {
                        bookmarks: self.bookmarks.bookmarks.clone(),
                        selected: 0,
                        list_state,
                    });
                    return false;
                }
                KeyCode::Char('b') => {
                    // b = bookmark current view directory
                    let dir_path = match self.view_mode {
                        ViewMode::Tree => {
                            // Bookmark the directory the cursor is on (or parent if on file)
                            let path_indices = flat_rows
                                .get(tree_state.cursor)
                                .map(|r| r.path_indices.clone());
                            if let Some(pi) = path_indices {
                                if let Some(node) = resolve_node(root, &pi) {
                                    if node.node_type == crate::model::NodeType::Dir {
                                        resolve_fs_path(&self.root_path, root, &pi)
                                    } else if pi.len() > 1 {
                                        resolve_fs_path(
                                            &self.root_path,
                                            root,
                                            &pi[..pi.len() - 1],
                                        )
                                    } else {
                                        Some(self.root_path.clone())
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        ViewMode::Bar | ViewMode::Treemap => {
                            if nav.view_root_path.is_empty() {
                                Some(self.root_path.clone())
                            } else {
                                resolve_fs_path(&self.root_path, root, &nav.view_root_path)
                            }
                        }
                    };
                    if let Some(path) = dir_path {
                        let label = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.to_string_lossy().into_owned());
                        if self.bookmarks.add(path, label) {
                            let _ = self.bookmarks.save();
                            self.overlay = Some(Overlay::Flash {
                                message: "Bookmarked!".into(),
                            });
                        } else {
                            self.overlay = Some(Overlay::Flash {
                                message: "Already bookmarked".into(),
                            });
                        }
                    }
                    return false;
                }
                _ => {}
            }

            // View-specific keys
            match self.view_mode {
                ViewMode::Tree => {
                    handle_tree_key(key, root, tree_state, flat_rows, active_filter.as_ref());
                }
                ViewMode::Bar => {
                    handle_nav_key(key, nav, root);
                    bar_state.sync_selection(nav.selected_child);
                }
                ViewMode::Treemap => {
                    handle_treemap_key(key, nav, root, treemap_state);
                }
            }
        }

        false
    }

    fn switch_to_nav_view(&mut self, mode: ViewMode) {
        if let AppState::Browsing {
            treemap_state, ..
        } = &mut self.state
        {
            if mode == ViewMode::Treemap {
                treemap_state.invalidate();
            }
        }
        self.view_mode = mode;
    }

    fn update_search_results(&mut self) {
        if let Some(Overlay::Search {
            input,
            all_entries,
            results,
            selected,
            list_state,
        }) = &mut self.overlay
        {
            let query = &input.query;
            if query.len() < 2 {
                results.clear();
                *selected = 0;
                list_state.select(None);
                return;
            }

            // Filter the pre-collected entries (no tree walk)
            let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
            use fuzzy_matcher::FuzzyMatcher;

            let mut scored: Vec<SearchResult> = all_entries
                .iter()
                .filter_map(|entry| {
                    matcher.fuzzy_match(&entry.name, query).map(|score| {
                        SearchResult {
                            name: entry.name.clone(),
                            path_indices: entry.path_indices.clone(),
                            name_path: entry.name_path.clone(),
                            score,
                        }
                    })
                })
                .collect();

            scored.sort_by(|a, b| b.score.cmp(&a.score));
            scored.truncate(20);

            *results = scored;
            *selected = 0;
            if results.is_empty() {
                list_state.select(None);
            } else {
                list_state.select(Some(0));
            }
        }
    }

    fn jump_to_search_result(&mut self, path_indices: &[usize], name_path: &[String]) {
        if let AppState::Browsing {
            root,
            tree_state,
            flat_rows,
            nav,
            bar_state,
            treemap_state,
            active_filter,
            ..
        } = &mut self.state
        {
            // Expand all parent directories in tree view
            for i in 1..name_path.len() {
                let parent_name_path = name_path[..i].to_vec();
                tree_state.expanded.insert(parent_name_path);
            }
            *flat_rows = flatten_tree_filtered(root, &tree_state.expanded, active_filter.as_ref());

            // Move cursor to the target
            if let Some(pos) = flat_rows
                .iter()
                .position(|r| r.path_indices == path_indices)
            {
                tree_state.cursor = pos;
                tree_state.list_state.select(Some(pos));
            }

            // Update nav views
            if path_indices.len() > 1 {
                nav.view_root_path = path_indices[..path_indices.len() - 1].to_vec();
                nav.selected_child = *path_indices.last().unwrap_or(&0);
            } else if !path_indices.is_empty() {
                nav.view_root_path.clear();
                nav.selected_child = path_indices[0];
            }
            bar_state.sync_selection(nav.selected_child);
            treemap_state.invalidate();
        }
    }

    fn apply_filter(&mut self, criteria: Option<FilterCriteria>) {
        if let AppState::Browsing {
            root,
            tree_state,
            flat_rows,
            active_filter,
            ..
        } = &mut self.state
        {
            *active_filter = criteria;
            *flat_rows =
                flatten_tree_filtered(root, &tree_state.expanded, active_filter.as_ref());
            tree_state.clamp_cursor(flat_rows.len());
        }
    }

    fn jump_to_bookmark(&mut self, path: &std::path::Path) {
        // Check if the bookmark is under our current root
        if let Ok(rel) = path.strip_prefix(&self.root_path) {
            if let AppState::Browsing {
                root,
                tree_state,
                flat_rows,
                nav,
                bar_state,
                treemap_state,
                active_filter,
                ..
            } = &mut self.state
            {
                // Walk the tree to find path_indices for this bookmark
                let components: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();

                let mut path_indices = Vec::new();
                let mut current = root.as_ref();
                let mut found = true;

                for component in &components {
                    if let Some(idx) = current
                        .children
                        .iter()
                        .position(|c| c.name == *component)
                    {
                        path_indices.push(idx);
                        current = &current.children[idx];
                    } else {
                        found = false;
                        break;
                    }
                }

                if found && !path_indices.is_empty() {
                    // For tree view: expand all parents and move cursor
                    let mut name_path = Vec::new();
                    let mut node = root.as_ref();
                    for &idx in &path_indices {
                        node = &node.children[idx];
                        name_path.push(node.name.clone());
                        // Expand this directory if it's a directory
                        if node.node_type == crate::model::NodeType::Dir
                            && !node.children.is_empty()
                        {
                            tree_state.expanded.insert(name_path.clone());
                        }
                    }
                    *flat_rows = flatten_tree_filtered(root, &tree_state.expanded, active_filter.as_ref());
                    // Find the cursor position for the target
                    if let Some(pos) = flat_rows
                        .iter()
                        .position(|r| r.path_indices == path_indices)
                    {
                        tree_state.cursor = pos;
                        tree_state.list_state.select(Some(pos));
                    }

                    // For nav views: set view root to parent
                    if path_indices.len() > 1 {
                        nav.view_root_path = path_indices[..path_indices.len() - 1].to_vec();
                        nav.selected_child = *path_indices.last().unwrap_or(&0);
                    } else {
                        nav.view_root_path.clear();
                        nav.selected_child = path_indices[0];
                    }
                    bar_state.sync_selection(nav.selected_child);
                    treemap_state.invalidate();
                }
            }
        } else {
            self.overlay = Some(Overlay::Flash {
                message: "Bookmark outside current scan — rescan needed".into(),
            });
        }
    }

    fn execute_delete(&mut self, path_indices: &[usize], fs_path: &std::path::Path, is_dir: bool) {
        // Perform filesystem deletion
        let result = if self.use_trash {
            trash::delete(fs_path).map_err(|e| e.to_string())
        } else if is_dir {
            std::fs::remove_dir_all(fs_path).map_err(|e| e.to_string())
        } else {
            std::fs::remove_file(fs_path).map_err(|e| e.to_string())
        };

        match result {
            Ok(()) => {
                // Remove from tree
                if let AppState::Browsing {
                    root,
                    tree_state,
                    flat_rows,
                    nav,
                    bar_state,
                    treemap_state,
                    active_filter,
                    ..
                } = &mut self.state
                {
                    root.remove_node(path_indices);
                    *flat_rows = flatten_tree_filtered(root, &tree_state.expanded, active_filter.as_ref());
                    tree_state.clamp_cursor(flat_rows.len());

                    // Clamp nav selection
                    let child_count = nav.child_count(root);
                    if child_count > 0 && nav.selected_child >= child_count {
                        nav.selected_child = child_count - 1;
                    }
                    bar_state.sync_selection(nav.selected_child);
                    treemap_state.invalidate();
                }
            }
            Err(msg) => {
                self.overlay = Some(Overlay::Flash {
                    message: format!("Delete failed: {msg}"),
                });
            }
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let elapsed = self.scan_start.elapsed();
        let root_path_str = self.root_path.to_string_lossy().to_string();
        let theme = &self.theme;
        let total_scan_time = self.total_scan_time;
        let view_mode = self.view_mode;

        match &mut self.state {
            AppState::Scanning {
                files_found,
                bytes_found,
                spinner_tick,
            } => {
                let progress = ScanProgress {
                    files_found: *files_found,
                    bytes_found: *bytes_found,
                    elapsed,
                    spinner_tick: *spinner_tick,
                    scan_path: &root_path_str,
                    theme,
                };
                frame.render_widget(progress, frame.area());
            }
            AppState::Browsing {
                root,
                tree_state,
                flat_rows,
                nav,
                bar_state,
                treemap_state,
                sort_config,
                active_filter,
            } => {
                render_browsing(
                    frame,
                    root,
                    tree_state,
                    flat_rows,
                    nav,
                    bar_state,
                    treemap_state,
                    sort_config,
                    active_filter,
                    theme,
                    &root_path_str,
                    total_scan_time,
                    view_mode,
                );
            }
            AppState::Error(msg) => {
                let para = Paragraph::new(format!("Error: {msg}"))
                    .alignment(Alignment::Center)
                    .red();
                let area = frame.area();
                let vert = Layout::vertical([
                    Constraint::Fill(1),
                    Constraint::Length(3),
                    Constraint::Fill(1),
                ])
                .split(area);
                frame.render_widget(para, vert[1]);
            }
        }

        // Render overlay on top
        if let Some(ref mut overlay) = self.overlay {
            render_overlay(frame, overlay, theme);
        }
    }
}

/// Pre-collect all node names from the tree (done once when search opens).
fn collect_all_entries(
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

/// Get the path_indices of the currently selected node, given already-destructured state.
fn selected_path_from_state(
    view_mode: ViewMode,
    flat_rows: &[FlatRow],
    tree_state: &TreeViewState,
    nav: &ViewNavState,
) -> Option<Vec<usize>> {
    match view_mode {
        ViewMode::Tree => flat_rows
            .get(tree_state.cursor)
            .map(|r| r.path_indices.clone()),
        ViewMode::Bar | ViewMode::Treemap => Some(nav.selected_path()),
    }
}

fn handle_tree_key(
    key: KeyEvent,
    root: &DiskNode,
    tree_state: &mut TreeViewState,
    flat_rows: &mut Vec<FlatRow>,
    active_filter: Option<&FilterCriteria>,
) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            tree_state.move_down(flat_rows.len());
        }
        KeyCode::Char('k') | KeyCode::Up => {
            tree_state.move_up();
        }
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

fn handle_nav_key(key: KeyEvent, nav: &mut ViewNavState, root: &DiskNode) {
    let child_count = nav.child_count(root);
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => nav.move_next(child_count),
        KeyCode::Char('k') | KeyCode::Up => nav.move_prev(),
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => nav.drill_in(root),
        KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => nav.drill_out(),
        _ => {}
    }
}

fn handle_treemap_key(
    key: KeyEvent,
    nav: &mut ViewNavState,
    root: &DiskNode,
    treemap_state: &mut TreemapState,
) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            let new_sel = treemap_state.navigate(nav.selected_child, 0, 1);
            nav.selected_child = remap_treemap_index(treemap_state, new_sel);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let new_sel = treemap_state.navigate(nav.selected_child, 0, -1);
            nav.selected_child = remap_treemap_index(treemap_state, new_sel);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            let new_sel = treemap_state.navigate(nav.selected_child, 1, 0);
            nav.selected_child = remap_treemap_index(treemap_state, new_sel);
        }
        KeyCode::Left | KeyCode::Char('h') => {
            let new_sel = treemap_state.navigate(nav.selected_child, -1, 0);
            nav.selected_child = remap_treemap_index(treemap_state, new_sel);
        }
        KeyCode::Enter => {
            nav.drill_in(root);
            treemap_state.invalidate();
        }
        KeyCode::Backspace => {
            nav.drill_out();
            treemap_state.invalidate();
        }
        _ => {}
    }
}

/// Map from treemap rect index back to child_index.
fn remap_treemap_index(state: &TreemapState, rect_idx: usize) -> usize {
    state
        .cached_rects
        .get(rect_idx)
        .map(|r| r.child_index)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn render_browsing(
    frame: &mut Frame,
    root: &DiskNode,
    tree_state: &mut TreeViewState,
    flat_rows: &[FlatRow],
    nav: &ViewNavState,
    bar_state: &mut BarState,
    treemap_state: &mut TreemapState,
    sort_config: &SortConfig,
    active_filter: &Option<FilterCriteria>,
    theme: &Theme,
    root_path_str: &str,
    total_scan_time: Option<Duration>,
    view_mode: ViewMode,
) {
    let area = frame.area();

    // Main area + status bar
    let [main_area, status_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    // For tree view: left (tree) + right (info), same as before
    // For other views: left (viz) + right (info)
    let [viz_area, info_area] =
        Layout::horizontal([Constraint::Percentage(65), Constraint::Fill(1)]).areas(main_area);

    // Viz panel with border
    let viz_title = match view_mode {
        ViewMode::Tree => format!(" {root_path_str} "),
        ViewMode::Treemap => format!(" {root_path_str} — Treemap "),
        ViewMode::Bar => format!(" {root_path_str} — Bar "),
    };

    let viz_block = Block::bordered()
        .title(viz_title)
        .title_alignment(Alignment::Left)
        .border_style(theme.border_style);

    let inner_viz = viz_block.inner(viz_area);
    frame.render_widget(viz_block, viz_area);

    // Render the active view
    let selected_node_path = match view_mode {
        ViewMode::Tree => {
            let tree_view = TreeView {
                rows: flat_rows,
                root_size: root.size,
                theme,
            };
            frame.render_stateful_widget(tree_view, inner_viz, &mut tree_state.list_state);
            flat_rows
                .get(tree_state.cursor)
                .map(|r| r.path_indices.clone())
        }
        ViewMode::Bar => {
            if let Some(view_node) = nav.resolve_view_root(root) {
                let bar_view = BarView {
                    node: view_node,
                    theme,
                };
                frame.render_stateful_widget(bar_view, inner_viz, bar_state);
            }
            Some(nav.selected_path())
        }
        ViewMode::Treemap => {
            if let Some(view_node) = nav.resolve_view_root(root) {
                let treemap_view = TreemapView {
                    node: view_node,
                    theme,
                    selected: nav.selected_child,
                };
                frame.render_stateful_widget(treemap_view, inner_viz, treemap_state);
            }
            Some(nav.selected_path())
        }
    };

    // Info panel
    render_info_panel(frame, root, &selected_node_path, theme, info_area);

    // Status bar with view indicator
    render_status_bar(
        frame,
        root,
        theme,
        total_scan_time,
        view_mode,
        sort_config,
        active_filter,
        status_area,
    );
}

fn render_info_panel(
    frame: &mut Frame,
    root: &DiskNode,
    selected_path: &Option<Vec<usize>>,
    theme: &Theme,
    area: Rect,
) {
    let info_block = Block::bordered()
        .title(" Info ")
        .title_alignment(Alignment::Left)
        .border_style(theme.border_style);

    let inner_info = info_block.inner(area);
    frame.render_widget(info_block, area);

    let info_content = if let Some(path) = selected_path {
        let node = resolve_node(root, path);
        match node {
            Some(node) => build_info_lines(node, root.size, theme),
            None => vec![Line::from("  No data")],
        }
    } else {
        vec![Line::from("  Empty directory")]
    };

    frame.render_widget(Paragraph::new(info_content), inner_info);
}

#[allow(clippy::too_many_arguments)]
fn render_status_bar(
    frame: &mut Frame,
    root: &DiskNode,
    theme: &Theme,
    total_scan_time: Option<Duration>,
    view_mode: ViewMode,
    sort_config: &SortConfig,
    active_filter: &Option<FilterCriteria>,
    area: Rect,
) {
    let scan_time = total_scan_time
        .map(|d| format!("{:.1}s", d.as_secs_f64()))
        .unwrap_or_else(|| "...".into());

    // Build view indicator
    let mut view_spans: Vec<Span> = Vec::new();
    view_spans.push(Span::raw("  "));
    for (i, mode) in ViewMode::ALL.iter().enumerate() {
        if i > 0 {
            view_spans.push(Span::raw(" "));
        }
        let text = format!("{}:{}", mode.key(), mode.label());
        if *mode == view_mode {
            view_spans.push(Span::styled(
                format!("[{text}]"),
                theme.view_indicator_active,
            ));
        } else {
            view_spans.push(Span::styled(text, theme.view_indicator_inactive));
        }
    }

    // Sort indicator
    let sort_arrow = if sort_config.ascending { "▲" } else { "▼" };
    let sort_text = format!("  Sort: {} {sort_arrow}", sort_config.field.label());

    let mut spans = vec![
        Span::styled("  Total: ", theme.status_style),
        Span::raw(format_size(root.size, BINARY)),
        Span::styled("  │  Files: ", theme.status_style),
        Span::raw(root.total_files().to_string()),
        Span::styled("  │  ", theme.status_style),
        Span::raw(scan_time),
        Span::styled("  │", theme.status_style),
    ];
    spans.extend(view_spans);
    spans.push(Span::styled("  │", theme.status_style));
    spans.push(Span::styled(sort_text, theme.status_style));

    if let Some(filter) = active_filter {
        spans.push(Span::styled("  │  Filter: ", theme.status_style));
        spans.push(Span::styled(
            filter.label(),
            Style::default().fg(ratatui::style::Color::Yellow),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn build_info_lines<'a>(node: &'a DiskNode, root_size: u64, theme: &Theme) -> Vec<Line<'a>> {
    let mut lines: Vec<Line<'a>> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("  Name: ", theme.status_style),
        Span::styled(node.name.as_str(), theme.node_style(&node.node_type)),
    ]));

    let type_str = match node.node_type {
        crate::model::NodeType::File => "File",
        crate::model::NodeType::Dir => "Directory",
        crate::model::NodeType::Symlink => "Symlink",
    };
    lines.push(Line::from(vec![
        Span::styled("  Type: ", theme.status_style),
        Span::raw(type_str),
    ]));

    lines.push(Line::from(vec![
        Span::styled("  Size: ", theme.status_style),
        Span::styled(
            format_size(node.size, BINARY),
            theme.size_style(node.size, root_size),
        ),
    ]));

    if node.node_type == crate::model::NodeType::Dir {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Children: ", theme.status_style),
            Span::raw(node.children.len().to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Files (recursive): ", theme.status_style),
            Span::raw(node.total_files().to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Dirs (recursive): ", theme.status_style),
            Span::raw(node.total_dirs().to_string()),
        ]));

        if !node.children.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Largest:",
                theme.status_style,
            )));
            for child in node.children.iter().take(5) {
                let icon = match child.node_type {
                    crate::model::NodeType::Dir => "d ",
                    crate::model::NodeType::File => "  ",
                    crate::model::NodeType::Symlink => "l ",
                };
                lines.push(Line::from(format!(
                    "  {icon} {} ({})",
                    child.name,
                    format_size(child.size, BINARY)
                )));
            }
        }
    }

    lines
}
