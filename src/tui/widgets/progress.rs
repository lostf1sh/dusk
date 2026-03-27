use std::time::Duration;

use humansize::{format_size, BINARY};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};

use crate::tui::theme::Theme;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct ScanProgress<'a> {
    pub files_found: u64,
    pub bytes_found: u64,
    pub elapsed: Duration,
    pub spinner_tick: usize,
    pub scan_path: &'a str,
    pub theme: &'a Theme,
}

impl Widget for ScanProgress<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let spinner = SPINNER_FRAMES[self.spinner_tick % SPINNER_FRAMES.len()];

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("{spinner} "), self.theme.spinner_style),
                Span::styled("Scanning ", self.theme.spinner_style),
                Span::raw(self.scan_path),
            ]),
            Line::from(""),
            Line::from(format!("Files found:  {}", format_number(self.files_found))),
            Line::from(format!(
                "Size found:   {}",
                format_size(self.bytes_found, BINARY)
            )),
            Line::from(format!("Elapsed:      {:.1}s", self.elapsed.as_secs_f64())),
        ];

        // Center the content box
        let block_height = 8u16;
        let block_width = 44u16;

        let vert = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(block_height),
            Constraint::Fill(1),
        ])
        .split(area);

        let horiz = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(block_width),
            Constraint::Fill(1),
        ])
        .split(vert[1]);

        let block = Block::bordered()
            .title(" dusk ")
            .title_alignment(Alignment::Center)
            .border_style(self.theme.border_style);

        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center)
            .render(horiz[1], buf);
    }
}

fn format_number(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}
