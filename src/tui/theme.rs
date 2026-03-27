use ratatui::style::{Color, Modifier, Style};

use crate::model::NodeType;

pub struct Theme {
    pub dir_style: Style,
    pub file_style: Style,
    pub symlink_style: Style,
    pub selected_style: Style,
    pub border_style: Style,
    pub status_style: Style,
    pub size_large: Style,
    pub size_medium: Style,
    pub size_small: Style,
    pub spinner_style: Style,
    /// Rotating palette for treemap/sunburst segment distinction.
    pub segment_colors: [Color; 8],
    pub bar_empty: Style,
    pub view_indicator_active: Style,
    pub view_indicator_inactive: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            dir_style: Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            file_style: Style::default().fg(Color::White),
            symlink_style: Style::default().fg(Color::Cyan).add_modifier(Modifier::ITALIC),
            selected_style: Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            border_style: Style::default().fg(Color::Gray),
            status_style: Style::default().fg(Color::DarkGray),
            size_large: Style::default().fg(Color::Red),
            size_medium: Style::default().fg(Color::Yellow),
            size_small: Style::default().fg(Color::Green),
            spinner_style: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            segment_colors: [
                Color::Blue,
                Color::Green,
                Color::Red,
                Color::Yellow,
                Color::Magenta,
                Color::Cyan,
                Color::LightBlue,
                Color::LightGreen,
            ],
            bar_empty: Style::default().fg(Color::DarkGray),
            view_indicator_active: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            view_indicator_inactive: Style::default().fg(Color::DarkGray),
        }
    }
}

impl Theme {
    /// Pick a style based on node type.
    pub fn node_style(&self, node_type: &NodeType) -> Style {
        match node_type {
            NodeType::Dir => self.dir_style,
            NodeType::File => self.file_style,
            NodeType::Symlink => self.symlink_style,
        }
    }

    /// Pick a color from the rotating segment palette.
    pub fn segment_color(&self, index: usize) -> Color {
        self.segment_colors[index % self.segment_colors.len()]
    }

    /// Pick a size style based on proportion of parent.
    pub fn size_style(&self, size: u64, parent_size: u64) -> Style {
        if parent_size == 0 {
            return self.size_small;
        }
        let ratio = size as f64 / parent_size as f64;
        if ratio > 0.5 {
            self.size_large
        } else if ratio > 0.1 {
            self.size_medium
        } else {
            self.size_small
        }
    }
}
