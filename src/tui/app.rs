use std::sync::mpsc::TryRecvError;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::ListState;

use crate::config::bookmarks::BookmarkStore;
use crate::model::metadata::load_metadata;
use crate::scanner::walker::ScanUpdate;
use crate::tui::overlay::{handle_overlay_key, Overlay, OverlayAction, SearchResult};
use crate::tui::views::tree::{
    flatten_tree_filtered, resolve_fs_path, resolve_fs_path_by_name_path, resolve_node,
};
use crate::tui::widgets::text_input::TextInputState;

use super::browse::{
    collect_all_entries, handle_nav_key, handle_tree_key, handle_treemap_key,
    selected_path_from_state, sync_bar_list_selection,
};
use super::{App, AppState, BrowsingState, ViewMode};

impl App {
    pub fn new(
        root_path: std::path::PathBuf,
        scan_rx: std::sync::mpsc::Receiver<ScanUpdate>,
        use_trash: bool,
    ) -> Self {
        let (bookmarks, startup_notices) = match BookmarkStore::load() {
            Ok(bookmarks) => (bookmarks, Vec::new()),
            Err(error) => (
                BookmarkStore::default(),
                vec![format!("Bookmarks unavailable: {error}")],
            ),
        };

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
            scan_start: std::time::Instant::now(),
            total_scan_time: None,
            theme: Default::default(),
            use_trash,
            bookmarks,
            scan_warnings: Default::default(),
            startup_notices,
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
                        files_found: current_files,
                        bytes_found: current_bytes,
                        ..
                    } = &mut self.state
                    {
                        *current_files = files_found;
                        *current_bytes = bytes_found;
                    }
                }
                Ok(ScanUpdate::Complete { root, warnings }) => {
                    self.total_scan_time = Some(self.scan_start.elapsed());
                    self.scan_warnings = warnings;
                    if let Some(summary) = self.scan_warnings.summary() {
                        self.startup_notices.push(summary);
                    }
                    self.state = AppState::Browsing(BrowsingState::from_root(root));
                    self.flush_notices();
                }
                Ok(ScanUpdate::Error(message)) => {
                    self.state = AppState::Error(message);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if matches!(self.state, AppState::Scanning { .. }) {
                        self.state = AppState::Error("Scanner terminated unexpectedly".to_string());
                    }
                    break;
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
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
                    if let Err(error) = self.bookmarks.save() {
                        self.overlay = Some(Overlay::Flash {
                            message: format!("Bookmark removed, but save failed: {error}"),
                        });
                        return false;
                    }

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

        if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
            return true;
        }

        if matches!(self.state, AppState::Browsing(_)) {
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

        if let AppState::Browsing(browsing) = &mut self.state {
            match key.code {
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    browsing.sort_config.ascending = !browsing.sort_config.ascending;
                    browsing.root.sort_children(&browsing.sort_config);
                    browsing.flat_rows = flatten_tree_filtered(
                        &browsing.root,
                        &browsing.tree_state.expanded,
                        browsing.active_filter.as_ref(),
                    );
                    browsing.treemap_state.invalidate();
                    sync_bar_list_selection(
                        &browsing.nav,
                        &browsing.root,
                        &mut browsing.bar_state,
                        browsing.active_filter.as_ref(),
                    );
                    return false;
                }
                KeyCode::Char('s') => {
                    browsing.sort_config.field = browsing.sort_config.field.next();
                    browsing.root.sort_children(&browsing.sort_config);
                    browsing.flat_rows = flatten_tree_filtered(
                        &browsing.root,
                        &browsing.tree_state.expanded,
                        browsing.active_filter.as_ref(),
                    );
                    browsing.treemap_state.invalidate();
                    sync_bar_list_selection(
                        &browsing.nav,
                        &browsing.root,
                        &mut browsing.bar_state,
                        browsing.active_filter.as_ref(),
                    );
                    return false;
                }
                KeyCode::Char('i') => {
                    let path_indices = selected_path_from_state(self.view_mode, browsing);
                    if let Some(path_indices) = path_indices {
                        if let Some(fs_path) =
                            resolve_fs_path(&self.root_path, &browsing.root, &path_indices)
                        {
                            let node_name = resolve_node(&browsing.root, &path_indices)
                                .map(|node| node.name.clone())
                                .unwrap_or_default();
                            match load_metadata(&fs_path) {
                                Ok(metadata) => {
                                    self.overlay = Some(Overlay::FileInfo {
                                        lines: metadata.to_lines(&node_name),
                                    });
                                }
                                Err(error) => {
                                    self.overlay = Some(Overlay::Flash {
                                        message: format!("Cannot read info: {error}"),
                                    });
                                }
                            }
                        }
                    }
                    return false;
                }
                KeyCode::Char('d') => {
                    let path_indices = selected_path_from_state(self.view_mode, browsing);
                    if let Some(path_indices) = path_indices {
                        if let Some(fs_path) =
                            resolve_fs_path(&self.root_path, &browsing.root, &path_indices)
                        {
                            if let Some(node) = resolve_node(&browsing.root, &path_indices) {
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
                    let mut all_entries = Vec::new();
                    collect_all_entries(
                        &browsing.root,
                        &mut Vec::new(),
                        &mut Vec::new(),
                        &mut all_entries,
                    );
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
                    let dir_path = match self.view_mode {
                        ViewMode::Tree => {
                            let path_indices = browsing
                                .flat_rows
                                .get(browsing.tree_state.cursor)
                                .map(|row| row.path_indices.clone());
                            if let Some(path_indices) = path_indices {
                                if let Some(node) = resolve_node(&browsing.root, &path_indices) {
                                    if node.node_type == crate::model::NodeType::Dir {
                                        resolve_fs_path(
                                            &self.root_path,
                                            &browsing.root,
                                            &path_indices,
                                        )
                                    } else if path_indices.len() > 1 {
                                        resolve_fs_path(
                                            &self.root_path,
                                            &browsing.root,
                                            &path_indices[..path_indices.len() - 1],
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
                        ViewMode::Bar | ViewMode::Treemap => resolve_fs_path_by_name_path(
                            &self.root_path,
                            &browsing.root,
                            &browsing.nav.view_dir_name_path,
                        ),
                    };

                    if let Some(path) = dir_path {
                        let label = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.to_string_lossy().into_owned());
                        if self.bookmarks.add(path, label) {
                            let message = match self.bookmarks.save() {
                                Ok(()) => "Bookmarked!".into(),
                                Err(error) => {
                                    format!("Bookmarked in memory, but save failed: {error}")
                                }
                            };
                            self.overlay = Some(Overlay::Flash { message });
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

            match self.view_mode {
                ViewMode::Tree => handle_tree_key(
                    key,
                    &browsing.root,
                    &mut browsing.tree_state,
                    &mut browsing.flat_rows,
                    browsing.active_filter.as_ref(),
                ),
                ViewMode::Bar => {
                    handle_nav_key(
                        key,
                        &mut browsing.nav,
                        &browsing.root,
                        browsing.active_filter.as_ref(),
                    );
                    sync_bar_list_selection(
                        &browsing.nav,
                        &browsing.root,
                        &mut browsing.bar_state,
                        browsing.active_filter.as_ref(),
                    );
                }
                ViewMode::Treemap => handle_treemap_key(
                    key,
                    &mut browsing.nav,
                    &browsing.root,
                    &mut browsing.treemap_state,
                    browsing.active_filter.as_ref(),
                ),
            }
        }

        false
    }

    fn switch_to_nav_view(&mut self, mode: ViewMode) {
        if let AppState::Browsing(browsing) = &mut self.state {
            if mode == ViewMode::Treemap {
                browsing.treemap_state.invalidate();
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

            use fuzzy_matcher::FuzzyMatcher;

            let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
            let mut scored: Vec<SearchResult> = all_entries
                .iter()
                .filter_map(|entry| {
                    matcher
                        .fuzzy_match(&entry.name, query)
                        .map(|score| SearchResult {
                            name: entry.name.clone(),
                            path_indices: entry.path_indices.clone(),
                            name_path: entry.name_path.clone(),
                            score,
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
        if let AppState::Browsing(browsing) = &mut self.state {
            for i in 1..name_path.len() {
                browsing.tree_state.expanded.insert(name_path[..i].to_vec());
            }
            browsing.flat_rows = flatten_tree_filtered(
                &browsing.root,
                &browsing.tree_state.expanded,
                browsing.active_filter.as_ref(),
            );

            if let Some(position) = browsing
                .flat_rows
                .iter()
                .position(|row| row.path_indices == path_indices)
            {
                browsing.tree_state.cursor = position;
                browsing.tree_state.list_state.select(Some(position));
            }

            if name_path.len() > 1 {
                browsing.nav.view_dir_name_path = name_path[..name_path.len() - 1].to_vec();
                browsing.nav.selected_name = name_path.last().cloned().unwrap_or_default();
            } else if let Some(name) = name_path.first() {
                browsing.nav.view_dir_name_path.clear();
                browsing.nav.selected_name = name.clone();
            }
            browsing
                .nav
                .ensure_valid_selection(&browsing.root, browsing.active_filter.as_ref());
            sync_bar_list_selection(
                &browsing.nav,
                &browsing.root,
                &mut browsing.bar_state,
                browsing.active_filter.as_ref(),
            );
            browsing.treemap_state.invalidate();
        }
    }

    fn apply_filter(&mut self, criteria: Option<crate::tui::filter::FilterCriteria>) {
        if let AppState::Browsing(browsing) = &mut self.state {
            browsing.active_filter = criteria;
            browsing.flat_rows = flatten_tree_filtered(
                &browsing.root,
                &browsing.tree_state.expanded,
                browsing.active_filter.as_ref(),
            );
            browsing.tree_state.clamp_cursor(browsing.flat_rows.len());
            browsing
                .nav
                .ensure_valid_selection(&browsing.root, browsing.active_filter.as_ref());
            browsing.treemap_state.invalidate();
        }
    }

    pub(super) fn flush_notices(&mut self) {
        if self.startup_notices.is_empty() {
            return;
        }

        let message = self.startup_notices.join(" | ");
        self.startup_notices.clear();
        self.overlay = Some(Overlay::Flash { message });
    }

    pub(super) fn jump_to_bookmark(&mut self, path: &std::path::Path) {
        if let Ok(rel) = path.strip_prefix(&self.root_path) {
            if let AppState::Browsing(browsing) = &mut self.state {
                if rel.as_os_str().is_empty() {
                    browsing.tree_state.cursor = 0;
                    browsing
                        .tree_state
                        .list_state
                        .select((!browsing.flat_rows.is_empty()).then_some(0));
                    browsing.nav.view_dir_name_path.clear();
                    browsing
                        .nav
                        .ensure_valid_selection(&browsing.root, browsing.active_filter.as_ref());
                    sync_bar_list_selection(
                        &browsing.nav,
                        &browsing.root,
                        &mut browsing.bar_state,
                        browsing.active_filter.as_ref(),
                    );
                    browsing.treemap_state.invalidate();
                    return;
                }

                let components: Vec<String> = rel
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy().into_owned())
                    .collect();

                let mut path_indices = Vec::new();
                let mut current = browsing.root.as_ref();
                let mut found = true;

                for component in &components {
                    if let Some(index) = current
                        .children
                        .iter()
                        .position(|child| child.name == *component)
                    {
                        path_indices.push(index);
                        current = &current.children[index];
                    } else {
                        found = false;
                        break;
                    }
                }

                if found && !path_indices.is_empty() {
                    let mut name_path = Vec::new();
                    let mut node = browsing.root.as_ref();
                    for &index in &path_indices {
                        node = &node.children[index];
                        name_path.push(node.name.clone());
                        if node.node_type == crate::model::NodeType::Dir
                            && !node.children.is_empty()
                        {
                            browsing.tree_state.expanded.insert(name_path.clone());
                        }
                    }
                    browsing.flat_rows = flatten_tree_filtered(
                        &browsing.root,
                        &browsing.tree_state.expanded,
                        browsing.active_filter.as_ref(),
                    );
                    if let Some(position) = browsing
                        .flat_rows
                        .iter()
                        .position(|row| row.path_indices == path_indices)
                    {
                        browsing.tree_state.cursor = position;
                        browsing.tree_state.list_state.select(Some(position));
                    }

                    if components.len() > 1 {
                        browsing.nav.view_dir_name_path =
                            components[..components.len() - 1].to_vec();
                        browsing.nav.selected_name = components[components.len() - 1].clone();
                    } else {
                        browsing.nav.view_dir_name_path.clear();
                        browsing.nav.selected_name = components[0].clone();
                    }
                    browsing
                        .nav
                        .ensure_valid_selection(&browsing.root, browsing.active_filter.as_ref());
                    sync_bar_list_selection(
                        &browsing.nav,
                        &browsing.root,
                        &mut browsing.bar_state,
                        browsing.active_filter.as_ref(),
                    );
                    browsing.treemap_state.invalidate();
                }
            }
        } else {
            self.overlay = Some(Overlay::Flash {
                message: "Bookmark outside current scan — rescan needed".into(),
            });
        }
    }

    fn execute_delete(&mut self, path_indices: &[usize], fs_path: &std::path::Path, is_dir: bool) {
        let result = if self.use_trash {
            trash::delete(fs_path).map_err(|error| error.to_string())
        } else if is_dir {
            std::fs::remove_dir_all(fs_path).map_err(|error| error.to_string())
        } else {
            std::fs::remove_file(fs_path).map_err(|error| error.to_string())
        };

        match result {
            Ok(()) => {
                if let AppState::Browsing(browsing) = &mut self.state {
                    browsing.root.remove_node(path_indices);
                    browsing.flat_rows = flatten_tree_filtered(
                        &browsing.root,
                        &browsing.tree_state.expanded,
                        browsing.active_filter.as_ref(),
                    );
                    browsing.tree_state.clamp_cursor(browsing.flat_rows.len());
                    browsing
                        .nav
                        .ensure_valid_selection(&browsing.root, browsing.active_filter.as_ref());
                    sync_bar_list_selection(
                        &browsing.nav,
                        &browsing.root,
                        &mut browsing.bar_state,
                        browsing.active_filter.as_ref(),
                    );
                    browsing.treemap_state.invalidate();
                }
            }
            Err(message) => {
                self.overlay = Some(Overlay::Flash {
                    message: format!("Delete failed: {message}"),
                });
            }
        }
    }
}
