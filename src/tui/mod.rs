pub mod theme;
pub mod views;
pub mod widgets;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use humansize::{format_size, BINARY};
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::model::node::DiskNode;
use crate::scanner::walker::ScanUpdate;
use crate::tui::theme::Theme;
use crate::tui::views::tree::{flatten_tree, resolve_node, FlatRow, TreeView, TreeViewState};
use crate::tui::widgets::progress::ScanProgress;

enum AppState {
    Scanning {
        files_found: u64,
        bytes_found: u64,
        spinner_tick: usize,
    },
    Browsing {
        root: DiskNode,
        tree_state: TreeViewState,
        flat_rows: Vec<FlatRow>,
    },
    Error(String),
}

pub struct App {
    state: AppState,
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
                        root,
                        tree_state,
                        flat_rows,
                    };
                }
                Ok(ScanUpdate::Error(msg)) => {
                    self.state = AppState::Error(msg);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Scanner thread died without sending Complete
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

        if let AppState::Browsing {
            root,
            tree_state,
            flat_rows,
        } = &mut self.state
        {
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

        false
    }

    fn render(&mut self, frame: &mut Frame) {
        // Extract immutable fields before matching on state
        let elapsed = self.scan_start.elapsed();
        let root_path_str = self.root_path.to_string_lossy().to_string();
        let theme = &self.theme;
        let total_scan_time = self.total_scan_time;

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
            } => {
                render_browsing(frame, root, tree_state, flat_rows, theme, &root_path_str, total_scan_time);
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

fn render_browsing(
    frame: &mut Frame,
    root: &DiskNode,
    tree_state: &mut TreeViewState,
    flat_rows: &[FlatRow],
    theme: &Theme,
    root_path_str: &str,
    total_scan_time: Option<Duration>,
) {
    let area = frame.area();

    // Main area + status bar
    let [main_area, status_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    // Left panel (tree) + right panel (info)
    let [tree_area, info_area] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Fill(1)]).areas(main_area);

    // Tree panel
    let tree_block = Block::bordered()
        .title(format!(" {root_path_str} "))
        .title_alignment(Alignment::Left)
        .border_style(theme.border_style);

    let inner_tree = tree_block.inner(tree_area);
    frame.render_widget(tree_block, tree_area);

    let tree_view = TreeView {
        rows: flat_rows,
        root_size: root.size,
        theme,
    };
    frame.render_stateful_widget(tree_view, inner_tree, &mut tree_state.list_state);

    // Info panel
    let info_block = Block::bordered()
        .title(" Info ")
        .title_alignment(Alignment::Left)
        .border_style(theme.border_style);

    let inner_info = info_block.inner(info_area);
    frame.render_widget(info_block, info_area);

    let info_content = if let Some(row) = flat_rows.get(tree_state.cursor) {
        let node = resolve_node(root, &row.path_indices);
        match node {
            Some(node) => build_info_lines(node, root.size, theme),
            None => vec![Line::from("  No data")],
        }
    } else {
        vec![Line::from("  Empty directory")]
    };

    frame.render_widget(Paragraph::new(info_content), inner_info);

    // Status bar
    let scan_time = total_scan_time
        .map(|d| format!("{:.1}s", d.as_secs_f64()))
        .unwrap_or_else(|| "...".into());

    let status = Line::from(vec![
        Span::styled("  Total: ", theme.status_style),
        Span::raw(format_size(root.size, BINARY)),
        Span::styled("  │  Files: ", theme.status_style),
        Span::raw(root.total_files().to_string()),
        Span::styled("  │  Dirs: ", theme.status_style),
        Span::raw(root.total_dirs().to_string()),
        Span::styled("  │  Scanned in ", theme.status_style),
        Span::raw(scan_time),
    ]);

    frame.render_widget(Paragraph::new(status), status_area);
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

        // Top 5 largest children
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
