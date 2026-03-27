use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, StatefulWidget, Wrap};
use ratatui::Frame;

use crate::config::bookmarks::Bookmark;
use crate::tui::filter::{FilterCriteria, DATE_PRESETS, SIZE_PRESETS};
use crate::tui::theme::Theme;
use crate::tui::widgets::text_input::{TextInput, TextInputState};

/// Active overlay on top of the main view.
pub enum Overlay {
    /// Delete confirmation dialog.
    DeleteConfirm {
        node_name: String,
        path_indices: Vec<usize>,
        fs_path: PathBuf,
        is_dir: bool,
        size: u64,
    },
    /// File info popup.
    FileInfo { lines: Vec<String> },
    /// Bookmark list.
    BookmarkList {
        bookmarks: Vec<Bookmark>,
        selected: usize,
        list_state: ListState,
    },
    /// Fuzzy search overlay.
    Search {
        input: TextInputState,
        /// All node names collected once when overlay opens.
        all_entries: Vec<SearchEntry>,
        results: Vec<SearchResult>,
        selected: usize,
        list_state: ListState,
    },
    /// Filter mode submenu.
    FilterMenu,
    /// Extension filter input.
    FilterExtInput { input: TextInputState },
    /// Status flash message (auto-dismiss on any key).
    Flash { message: String },
}

/// Pre-collected node entry for search (built once when overlay opens).
#[derive(Debug, Clone)]
pub struct SearchEntry {
    pub name: String,
    pub path_indices: Vec<usize>,
    pub name_path: Vec<String>,
}

/// A single fuzzy search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub name: String,
    pub path_indices: Vec<usize>,
    pub name_path: Vec<String>,
    pub score: i64,
}

/// Result of handling a key in an overlay.
pub enum OverlayAction {
    /// Overlay consumed the key, keep it open.
    Consumed,
    /// Close the overlay, no further action.
    Close,
    /// Confirm delete action.
    ConfirmDelete {
        path_indices: Vec<usize>,
        fs_path: PathBuf,
        is_dir: bool,
    },
    /// Jump to a bookmarked path.
    BookmarkJump(PathBuf),
    /// Remove a bookmark at index.
    BookmarkRemove(usize),
    /// Search query changed — needs result refresh.
    SearchUpdate,
    /// Jump to search result.
    SearchJump {
        path_indices: Vec<usize>,
        name_path: Vec<String>,
    },
    /// Apply a filter.
    ApplyFilter(FilterCriteria),
    /// Clear active filter.
    ClearFilter,
    /// Switch to extension filter input.
    SwitchToExtFilter,
}

/// Handle a key event for the current overlay. Returns a mutable reference issue workaround.
pub fn handle_overlay_key(overlay: &mut Overlay, key: KeyEvent) -> OverlayAction {
    match overlay {
        Overlay::DeleteConfirm {
            path_indices,
            fs_path,
            is_dir,
            ..
        } => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => OverlayAction::ConfirmDelete {
                path_indices: path_indices.clone(),
                fs_path: fs_path.clone(),
                is_dir: *is_dir,
            },
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => OverlayAction::Close,
            _ => OverlayAction::Consumed,
        },
        Overlay::FileInfo { .. } => match key.code {
            KeyCode::Esc | KeyCode::Char('i') | KeyCode::Char('q') => OverlayAction::Close,
            _ => OverlayAction::Consumed,
        },
        Overlay::BookmarkList {
            bookmarks,
            selected,
            list_state,
        } => match key.code {
            KeyCode::Esc | KeyCode::Char('B') | KeyCode::Char('q') => OverlayAction::Close,
            KeyCode::Char('j') | KeyCode::Down => {
                if !bookmarks.is_empty() {
                    *selected = (*selected + 1).min(bookmarks.len() - 1);
                    list_state.select(Some(*selected));
                }
                OverlayAction::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *selected = selected.saturating_sub(1);
                list_state.select(Some(*selected));
                OverlayAction::Consumed
            }
            KeyCode::Enter => {
                if let Some(bm) = bookmarks.get(*selected) {
                    OverlayAction::BookmarkJump(bm.path.clone())
                } else {
                    OverlayAction::Close
                }
            }
            KeyCode::Char('d') => {
                if !bookmarks.is_empty() {
                    OverlayAction::BookmarkRemove(*selected)
                } else {
                    OverlayAction::Consumed
                }
            }
            _ => OverlayAction::Consumed,
        },
        Overlay::Search {
            input,
            results,
            selected,
            list_state,
            ..
        } => match key.code {
            KeyCode::Esc => OverlayAction::Close,
            KeyCode::Enter => {
                if results.is_empty() {
                    // No results yet — run the search
                    if !input.query.is_empty() {
                        OverlayAction::SearchUpdate
                    } else {
                        OverlayAction::Close
                    }
                } else {
                    // Results shown — jump to selected
                    if let Some(result) = results.get(*selected) {
                        OverlayAction::SearchJump {
                            path_indices: result.path_indices.clone(),
                            name_path: result.name_path.clone(),
                        }
                    } else {
                        OverlayAction::Close
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') if !results.is_empty() => {
                *selected = (*selected + 1).min(results.len() - 1);
                list_state.select(Some(*selected));
                OverlayAction::Consumed
            }
            KeyCode::Up | KeyCode::Char('k') if !results.is_empty() => {
                *selected = selected.saturating_sub(1);
                list_state.select(Some(*selected));
                OverlayAction::Consumed
            }
            KeyCode::Backspace => {
                input.backspace();
                // Clear results so user can re-search
                results.clear();
                *selected = 0;
                list_state.select(None);
                OverlayAction::Consumed
            }
            KeyCode::Char(ch) if results.is_empty() => {
                // Only allow typing when no results shown
                input.insert(ch);
                OverlayAction::Consumed
            }
            KeyCode::Char(_) if !results.is_empty() => {
                // Any char while results shown — clear results, go back to input
                results.clear();
                *selected = 0;
                list_state.select(None);
                OverlayAction::Consumed
            }
            _ => OverlayAction::Consumed,
        },
        Overlay::FilterMenu => match key.code {
            KeyCode::Esc | KeyCode::Char('f') | KeyCode::Char('q') => OverlayAction::Close,
            KeyCode::Char('e') => {
                // Signal to caller to switch to ext input overlay
                // We use a special action for this
                OverlayAction::SwitchToExtFilter
            }
            KeyCode::Char('c') => OverlayAction::ClearFilter,
            KeyCode::Char('1') => OverlayAction::ApplyFilter(FilterCriteria::SizeRange {
                min: Some(SIZE_PRESETS[0].0),
                max: None,
            }),
            KeyCode::Char('2') => OverlayAction::ApplyFilter(FilterCriteria::SizeRange {
                min: Some(SIZE_PRESETS[1].0),
                max: None,
            }),
            KeyCode::Char('3') => OverlayAction::ApplyFilter(FilterCriteria::SizeRange {
                min: Some(SIZE_PRESETS[2].0),
                max: None,
            }),
            KeyCode::Char('4') => OverlayAction::ApplyFilter(FilterCriteria::SizeRange {
                min: Some(SIZE_PRESETS[3].0),
                max: None,
            }),
            KeyCode::Char('d') => {
                OverlayAction::ApplyFilter(FilterCriteria::ModifiedWithin(DATE_PRESETS[0].0))
            }
            KeyCode::Char('w') => {
                OverlayAction::ApplyFilter(FilterCriteria::ModifiedWithin(DATE_PRESETS[1].0))
            }
            KeyCode::Char('m') => {
                OverlayAction::ApplyFilter(FilterCriteria::ModifiedWithin(DATE_PRESETS[2].0))
            }
            KeyCode::Char('y') => {
                OverlayAction::ApplyFilter(FilterCriteria::ModifiedWithin(DATE_PRESETS[3].0))
            }
            _ => OverlayAction::Consumed,
        },
        Overlay::FilterExtInput { input } => match key.code {
            KeyCode::Esc => OverlayAction::Close,
            KeyCode::Enter => {
                if input.query.is_empty() {
                    OverlayAction::ClearFilter
                } else {
                    OverlayAction::ApplyFilter(FilterCriteria::Extension(input.query.clone()))
                }
            }
            KeyCode::Backspace => {
                input.backspace();
                OverlayAction::Consumed
            }
            KeyCode::Char(ch) => {
                input.insert(ch);
                OverlayAction::Consumed
            }
            KeyCode::Left => {
                input.move_left();
                OverlayAction::Consumed
            }
            KeyCode::Right => {
                input.move_right();
                OverlayAction::Consumed
            }
            _ => OverlayAction::Consumed,
        },
        Overlay::Flash { .. } => OverlayAction::Close,
    }
}

/// Render the overlay on top of existing content.
pub fn render_overlay(frame: &mut Frame, overlay: &mut Overlay, theme: &Theme) {
    match overlay {
        Overlay::DeleteConfirm {
            node_name,
            is_dir,
            size,
            ..
        } => {
            let area = centered_rect(50, 7, frame.area());
            let size_str = humansize::format_size(*size, humansize::BINARY);
            let type_str = if *is_dir { "directory" } else { "file" };

            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::raw("  Delete "),
                    Span::styled(type_str, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!(" \"{node_name}\" ({size_str})?")),
                ]),
                Line::from(""),
                Line::from(Span::styled("  [y] Yes   [n] No", theme.status_style)),
            ];

            let block = Block::bordered()
                .title(" Confirm Delete ")
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(Color::Red));

            frame.render_widget(Clear, area);
            frame.render_widget(Paragraph::new(lines).block(block), area);
        }
        Overlay::FileInfo { lines } => {
            let height = (lines.len() + 2).min(24) as u16;
            let area = centered_rect(60, height, frame.area());

            let text_lines: Vec<Line> = lines.iter().map(|l| Line::from(l.as_str())).collect();

            let block = Block::bordered()
                .title(" File Info ")
                .title_alignment(Alignment::Center)
                .border_style(theme.border_style);

            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(text_lines)
                    .block(block)
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Overlay::BookmarkList {
            bookmarks,
            list_state,
            ..
        } => {
            let height = (bookmarks.len() + 4).clamp(5, 20) as u16;
            let area = centered_rect(60, height, frame.area());

            if bookmarks.is_empty() {
                let block = Block::bordered()
                    .title(" Bookmarks ")
                    .title_alignment(Alignment::Center)
                    .border_style(theme.border_style);
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Paragraph::new("  No bookmarks yet. Press 'b' to add one.").block(block),
                    area,
                );
            } else {
                let items: Vec<ListItem> = bookmarks
                    .iter()
                    .map(|bm| {
                        let line = Line::from(vec![
                            Span::styled("  ", Style::default()),
                            Span::styled(
                                &bm.label,
                                Style::default()
                                    .fg(Color::Blue)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw("  "),
                            Span::styled(bm.path.to_string_lossy().to_string(), theme.status_style),
                        ]);
                        ListItem::new(line)
                    })
                    .collect();

                let block = Block::bordered()
                    .title(" Bookmarks — Enter:jump  d:remove  Esc:close ")
                    .title_alignment(Alignment::Center)
                    .border_style(theme.border_style);

                let list = List::new(items)
                    .block(block)
                    .highlight_style(theme.selected_style)
                    .highlight_symbol("│");

                frame.render_widget(Clear, area);
                StatefulWidget::render(list, area, frame.buffer_mut(), list_state);
            }
        }
        Overlay::Search {
            input,
            results,
            list_state,
            ..
        } => {
            let height = (results.len() + 4).clamp(5, 20) as u16;
            let area = centered_rect(60, height, frame.area());

            let title = if results.is_empty() {
                " Search — type and press Enter "
            } else {
                " Search — j/k:navigate  Enter:jump  Esc:close "
            };
            let block = Block::bordered()
                .title(title)
                .title_alignment(Alignment::Center)
                .border_style(theme.border_style);

            let inner = block.inner(area);
            frame.render_widget(Clear, area);
            frame.render_widget(block, area);

            if inner.height > 0 {
                // Text input on first row
                let [input_area, results_area] =
                    Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);

                let text_input = TextInput {
                    query: &input.query,
                    cursor: input.cursor,
                    label: " / ",
                };
                frame.render_widget(text_input, input_area);

                // Results list
                if !results.is_empty() {
                    let items: Vec<ListItem> = results
                        .iter()
                        .map(|r| {
                            let path_str = r.name_path.join("/");
                            ListItem::new(Line::from(vec![
                                Span::styled(
                                    format!("  {}", r.name),
                                    Style::default()
                                        .fg(Color::White)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::raw("  "),
                                Span::styled(path_str, theme.status_style),
                            ]))
                        })
                        .collect();

                    let list = List::new(items)
                        .highlight_style(theme.selected_style)
                        .highlight_symbol("│");

                    StatefulWidget::render(list, results_area, frame.buffer_mut(), list_state);
                }
            }
        }
        Overlay::FilterMenu => {
            let area = centered_rect(50, 14, frame.area());

            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Extension:",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from("   e  Enter extension"),
                Line::from(""),
                Line::from(Span::styled(
                    "  Size:",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(format!("   1  {}", SIZE_PRESETS[0].1)),
                Line::from(format!("   2  {}", SIZE_PRESETS[1].1)),
                Line::from(format!("   3  {}", SIZE_PRESETS[2].1)),
                Line::from(format!("   4  {}", SIZE_PRESETS[3].1)),
                Line::from(""),
                Line::from(Span::styled(
                    "  Date:",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(format!(
                    "   d/w/m/y  {}/{}/{}/{}",
                    DATE_PRESETS[0].1, DATE_PRESETS[1].1, DATE_PRESETS[2].1, DATE_PRESETS[3].1
                )),
                Line::from(""),
                Line::from(Span::styled("   c  Clear filter", theme.status_style)),
            ];

            let block = Block::bordered()
                .title(" Filter (Esc to close) ")
                .title_alignment(Alignment::Center)
                .border_style(theme.border_style);

            frame.render_widget(Clear, area);
            frame.render_widget(Paragraph::new(lines).block(block), area);
        }
        Overlay::FilterExtInput { input } => {
            let area = centered_rect(40, 3, frame.area());

            let block = Block::bordered()
                .title(" Extension Filter (Enter to apply) ")
                .title_alignment(Alignment::Center)
                .border_style(theme.border_style);

            let inner = block.inner(area);
            frame.render_widget(Clear, area);
            frame.render_widget(block, area);

            if inner.height > 0 {
                let text_input = TextInput {
                    query: &input.query,
                    cursor: input.cursor,
                    label: " ext: ",
                };
                frame.render_widget(text_input, inner);
            }
        }
        Overlay::Flash { message } => {
            let area = centered_rect(40, 3, frame.area());
            let block = Block::bordered().border_style(theme.border_style);

            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(Line::from(format!("  {message}"))).block(block),
                area,
            );
        }
    }
}

/// Create a centered rectangle with given width percentage and fixed height.
fn centered_rect(width_pct: u16, height: u16, area: Rect) -> Rect {
    let [vertical] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [horizontal] = Layout::horizontal([Constraint::Percentage(width_pct)])
        .flex(Flex::Center)
        .areas(vertical);
    horizontal
}
