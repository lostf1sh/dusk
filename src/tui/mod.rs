pub mod theme;
pub mod views;
pub mod widgets;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use humansize::{format_size, BINARY};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::model::node::DiskNode;
use crate::scanner::walker::ScanUpdate;
use crate::tui::theme::Theme;
use crate::tui::views::bar::{BarState, BarView};
use crate::tui::views::nav::ViewNavState;
use crate::tui::views::tree::{flatten_tree, resolve_node, FlatRow, TreeView, TreeViewState};
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
    },
    Error(String),
}

pub struct App {
    state: AppState,
    view_mode: ViewMode,
    scan_rx: Receiver<ScanUpdate>,
    root_path: PathBuf,
    scan_start: Instant,
    total_scan_time: Option<Duration>,
    theme: Theme,
}

impl App {
    pub fn new(root_path: PathBuf, scan_rx: Receiver<ScanUpdate>) -> Self {
        Self {
            state: AppState::Scanning {
                files_found: 0,
                bytes_found: 0,
                spinner_tick: 0,
            },
            view_mode: ViewMode::Tree,
            scan_rx,
            root_path,
            scan_start: Instant::now(),
            total_scan_time: None,
            theme: Theme::default(),
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
        } = &mut self.state
        {
            match self.view_mode {
                ViewMode::Tree => {
                    handle_tree_key(key, root, tree_state, flat_rows);
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
            } => {
                render_browsing(
                    frame,
                    root,
                    tree_state,
                    flat_rows,
                    nav,
                    bar_state,
                    treemap_state,
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
    }
}

fn handle_tree_key(
    key: KeyEvent,
    root: &DiskNode,
    tree_state: &mut TreeViewState,
    flat_rows: &mut Vec<FlatRow>,
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
            *flat_rows = flatten_tree(root, &tree_state.expanded);
        }
        KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => {
            tree_state.drill_out(flat_rows);
            *flat_rows = flatten_tree(root, &tree_state.expanded);
        }
        KeyCode::Char(' ') => {
            tree_state.toggle_expand(flat_rows);
            *flat_rows = flatten_tree(root, &tree_state.expanded);
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

fn render_status_bar(
    frame: &mut Frame,
    root: &DiskNode,
    theme: &Theme,
    total_scan_time: Option<Duration>,
    view_mode: ViewMode,
    area: Rect,
) {
    let scan_time = total_scan_time
        .map(|d| format!("{:.1}s", d.as_secs_f64()))
        .unwrap_or_else(|| "...".into());

    // Build view indicator: [1:Tree] 2:Map 3:Sun 4:Bar
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
