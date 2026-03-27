use humansize::{format_size, BINARY};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, StatefulWidget};

use crate::model::node::DiskNode;
use crate::tui::theme::Theme;

pub struct BarState {
    pub list_state: ListState,
}

impl BarState {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self { list_state }
    }

    pub fn sync_selection(&mut self, selected: usize) {
        self.list_state.select(Some(selected));
    }
}

pub struct BarView<'a> {
    pub node: &'a DiskNode,
    pub theme: &'a Theme,
}

impl StatefulWidget for BarView<'_> {
    type State = BarState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let parent_size = self.node.size;
        if parent_size == 0 || self.node.children.is_empty() {
            let empty = List::new(vec![ListItem::new("  (empty)")]);
            StatefulWidget::render(empty, area, buf, &mut state.list_state);
            return;
        }

        let bar_width = (area.width as usize * 35 / 100).clamp(8, 50);

        let items: Vec<ListItem> = self
            .node
            .children
            .iter()
            .enumerate()
            .map(|(i, child)| {
                let pct = child.size as f64 / parent_size as f64 * 100.0;
                let fill_count = (bar_width as f64 * child.size as f64 / parent_size as f64)
                    .round() as usize;
                let empty_count = bar_width.saturating_sub(fill_count);

                let fill_str: String = "█".repeat(fill_count);
                let empty_str: String = "░".repeat(empty_count);

                let bar_color = self.theme.segment_color(i);
                let size_str = format_size(child.size, BINARY);

                let name = if child.node_type == crate::model::NodeType::Dir {
                    format!("{}/", child.name)
                } else {
                    child.name.clone()
                };

                // Calculate spacing
                let pct_str = format!("{pct:5.1}%");
                let used = bar_width + 1 + pct_str.len() + 2 + name.len() + 2 + size_str.len();
                let available = area.width as usize;
                let padding = if used < available {
                    available - used
                } else {
                    1
                };

                let name_style = self.theme.node_style(&child.node_type);
                let size_style = self.theme.size_style(child.size, parent_size);

                let line = Line::from(vec![
                    Span::styled(fill_str, ratatui::style::Style::default().fg(bar_color)),
                    Span::styled(empty_str, self.theme.bar_empty),
                    Span::raw(" "),
                    Span::styled(pct_str, size_style),
                    Span::raw("  "),
                    Span::styled(name, name_style),
                    Span::raw(" ".repeat(padding)),
                    Span::raw(size_str),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(self.theme.selected_style)
            .highlight_symbol("│");

        StatefulWidget::render(list, area, buf, &mut state.list_state);
    }
}
