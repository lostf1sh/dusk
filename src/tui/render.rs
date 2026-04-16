use std::time::Duration;

use humansize::{format_size, BINARY};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::model::node::{DiskNode, SortConfig};
use crate::tui::filter::FilterCriteria;
use crate::tui::overlay::render_overlay;
use crate::tui::theme::Theme;
use crate::tui::views::bar::BarView;
use crate::tui::views::tree::{filter_visible_child_indices, resolve_node, TreeView};
use crate::tui::views::treemap::{visible_treemap_child_indices, TreemapView};
use crate::tui::widgets::progress::ScanProgress;

use super::browse::{sync_bar_list_selection, sync_treemap_selection};
use super::{App, AppState, BrowsingState, ViewMode};

impl App {
    pub(super) fn render(&mut self, frame: &mut Frame) {
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
            AppState::Browsing(browsing) => {
                render_browsing(
                    frame,
                    browsing,
                    theme,
                    &root_path_str,
                    total_scan_time,
                    view_mode,
                    self.scan_warnings.skipped_entries,
                );
            }
            AppState::Error(message) => {
                let paragraph = Paragraph::new(format!("Error: {message}"))
                    .alignment(Alignment::Center)
                    .red();
                let area = frame.area();
                let vertical = Layout::vertical([
                    Constraint::Fill(1),
                    Constraint::Length(3),
                    Constraint::Fill(1),
                ])
                .split(area);
                frame.render_widget(paragraph, vertical[1]);
            }
        }

        if let Some(ref mut overlay) = self.overlay {
            render_overlay(frame, overlay, theme);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_browsing(
    frame: &mut Frame,
    browsing: &mut BrowsingState,
    theme: &Theme,
    root_path_str: &str,
    total_scan_time: Option<Duration>,
    view_mode: ViewMode,
    skipped_entries: u64,
) {
    let area = frame.area();
    let [main_area, status_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    let [viz_area, info_area] =
        Layout::horizontal([Constraint::Percentage(65), Constraint::Fill(1)]).areas(main_area);

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

    match view_mode {
        ViewMode::Bar => {
            browsing
                .nav
                .ensure_valid_selection(&browsing.root, browsing.active_filter.as_ref());
            sync_bar_list_selection(
                &browsing.nav,
                &browsing.root,
                &mut browsing.bar_state,
                browsing.active_filter.as_ref(),
            );
        }
        ViewMode::Treemap => {
            sync_treemap_selection(
                &mut browsing.nav,
                &browsing.root,
                browsing.active_filter.as_ref(),
            );
        }
        ViewMode::Tree => {}
    }

    let selected_node_path = match view_mode {
        ViewMode::Tree => {
            let tree_view = TreeView {
                rows: &browsing.flat_rows,
                root_size: browsing.root.size,
                theme,
            };
            frame.render_stateful_widget(tree_view, inner_viz, &mut browsing.tree_state.list_state);
            browsing
                .flat_rows
                .get(browsing.tree_state.cursor)
                .map(|row| row.path_indices.clone())
        }
        ViewMode::Bar => {
            if let Some(view_node) = browsing.nav.resolve_view_root(&browsing.root) {
                let visible =
                    filter_visible_child_indices(view_node, browsing.active_filter.as_ref());
                let bar_view = BarView {
                    node: view_node,
                    theme,
                    visible_indices: &visible,
                };
                frame.render_stateful_widget(bar_view, inner_viz, &mut browsing.bar_state);
            }
            browsing.nav.path_indices(&browsing.root)
        }
        ViewMode::Treemap => {
            if let Some(view_node) = browsing.nav.resolve_view_root(&browsing.root) {
                let visible =
                    visible_treemap_child_indices(view_node, browsing.active_filter.as_ref());
                let selected_child_index = view_node
                    .children
                    .iter()
                    .position(|child| child.name == browsing.nav.selected_name)
                    .unwrap_or(0);
                let treemap_view = TreemapView {
                    node: view_node,
                    theme,
                    selected_child_index,
                    visible_indices: &visible,
                };
                frame.render_stateful_widget(treemap_view, inner_viz, &mut browsing.treemap_state);
            }
            browsing.nav.path_indices(&browsing.root)
        }
    };

    render_info_panel(frame, &browsing.root, &selected_node_path, theme, info_area);
    render_status_bar(
        frame,
        &browsing.root,
        theme,
        total_scan_time,
        view_mode,
        &browsing.sort_config,
        &browsing.active_filter,
        status_area,
        skipped_entries,
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
        match resolve_node(root, path) {
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
    skipped_entries: u64,
) {
    let scan_time = total_scan_time
        .map(|duration| format!("{:.1}s", duration.as_secs_f64()))
        .unwrap_or_else(|| "...".into());

    let mut view_spans = vec![Span::raw("  ")];
    for (index, mode) in ViewMode::ALL.iter().enumerate() {
        if index > 0 {
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

    if skipped_entries > 0 {
        spans.push(Span::styled("  │  Skipped: ", theme.status_style));
        spans.push(Span::styled(
            skipped_entries.to_string(),
            Style::default().fg(ratatui::style::Color::Yellow),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn build_info_lines<'a>(node: &'a DiskNode, root_size: u64, theme: &Theme) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

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
            lines.push(Line::from(Span::styled("  Largest:", theme.status_style)));
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
