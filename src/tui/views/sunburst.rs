use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::StatefulWidget;

use crate::model::node::{DiskNode, NodeType};
use crate::tui::theme::Theme;

use std::f64::consts::PI;

/// An arc segment in the sunburst chart.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ArcSegment {
    pub child_index: usize,
    pub ring: usize,
    pub start_angle: f64,
    pub end_angle: f64,
    pub inner_radius: f64,
    pub outer_radius: f64,
    pub color: Color,
    pub name: String,
}

pub struct SunburstState {
    pub cached_segments: Vec<ArcSegment>,
    pub cached_area: Rect,
    pub max_rings: usize,
    /// (ring, segment_index_within_ring) for selection
    pub selected_ring: usize,
    pub selected_segment: usize,
}

impl SunburstState {
    pub fn new() -> Self {
        Self {
            cached_segments: Vec::new(),
            cached_area: Rect::default(),
            max_rings: 3,
            selected_ring: 0,
            selected_segment: 0,
        }
    }

    pub fn invalidate(&mut self) {
        self.cached_segments.clear();
    }

    pub fn update_segments(&mut self, node: &DiskNode, area: Rect, theme: &Theme) {
        if self.cached_area == area && !self.cached_segments.is_empty() {
            return;
        }
        self.cached_segments = compute_segments(node, self.max_rings, theme);
        self.cached_area = area;
    }

    /// Get segments in the current ring for navigation.
    fn segments_in_ring(&self, ring: usize) -> Vec<usize> {
        self.cached_segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.ring == ring)
            .map(|(i, _)| i)
            .collect()
    }

    pub fn move_angular_next(&mut self) {
        let ring_segs = self.segments_in_ring(self.selected_ring);
        if ring_segs.is_empty() {
            return;
        }
        self.selected_segment = (self.selected_segment + 1) % ring_segs.len();
    }

    pub fn move_angular_prev(&mut self) {
        let ring_segs = self.segments_in_ring(self.selected_ring);
        if ring_segs.is_empty() {
            return;
        }
        self.selected_segment = if self.selected_segment == 0 {
            ring_segs.len() - 1
        } else {
            self.selected_segment - 1
        };
    }

    pub fn move_ring_out(&mut self) {
        if self.selected_ring < self.max_rings - 1 {
            // Check if there are segments in the next ring
            let next_segs = self.segments_in_ring(self.selected_ring + 1);
            if !next_segs.is_empty() {
                self.selected_ring += 1;
                self.selected_segment = 0;
            }
        }
    }

    pub fn move_ring_in(&mut self) {
        if self.selected_ring > 0 {
            self.selected_ring -= 1;
            self.selected_segment = 0;
        }
    }

    /// Get the child_index of the currently selected segment.
    pub fn selected_child_index(&self) -> Option<usize> {
        let ring_segs = self.segments_in_ring(self.selected_ring);
        ring_segs
            .get(self.selected_segment)
            .and_then(|&seg_idx| self.cached_segments.get(seg_idx))
            .map(|s| s.child_index)
    }
}

/// Compute all arc segments for the sunburst chart.
fn compute_segments(node: &DiskNode, max_rings: usize, theme: &Theme) -> Vec<ArcSegment> {
    let mut segments = Vec::new();
    if node.size == 0 || node.children.is_empty() {
        return segments;
    }

    let ring_width = 1.0 / (max_rings as f64 + 0.5); // leave inner gap

    #[allow(clippy::too_many_arguments)]
    fn recurse(
        children: &[DiskNode],
        parent_size: u64,
        start_angle: f64,
        angle_span: f64,
        ring: usize,
        max_rings: usize,
        ring_width: f64,
        theme: &Theme,
        segments: &mut Vec<ArcSegment>,
    ) {
        if ring >= max_rings || children.is_empty() || parent_size == 0 {
            return;
        }

        let inner_r = (ring as f64 + 0.5) * ring_width;
        let outer_r = (ring as f64 + 1.5) * ring_width;
        let mut current_angle = start_angle;

        for (i, child) in children.iter().enumerate() {
            if child.size == 0 {
                continue;
            }
            let child_span = angle_span * (child.size as f64 / parent_size as f64);
            if child_span < 0.01 {
                current_angle += child_span;
                continue; // too small to render
            }

            let color = theme.segment_color(i);
            segments.push(ArcSegment {
                child_index: i,
                ring,
                start_angle: current_angle,
                end_angle: current_angle + child_span,
                inner_radius: inner_r,
                outer_radius: outer_r,
                color,
                name: child.name.clone(),
            });

            // Recurse into children
            if child.node_type == NodeType::Dir {
                recurse(
                    &child.children,
                    child.size,
                    current_angle,
                    child_span,
                    ring + 1,
                    max_rings,
                    ring_width,
                    theme,
                    segments,
                );
            }

            current_angle += child_span;
        }
    }

    recurse(
        &node.children,
        node.size,
        0.0,
        2.0 * PI,
        0,
        max_rings,
        ring_width,
        theme,
        &mut segments,
    );

    segments
}

pub struct SunburstView<'a> {
    pub node: &'a DiskNode,
    pub theme: &'a Theme,
}

impl StatefulWidget for SunburstView<'_> {
    type State = SunburstState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if self.node.children.is_empty() || self.node.size == 0 {
            buf.set_string(
                area.x + 1,
                area.y + area.height / 2,
                "(empty directory)",
                self.theme.status_style,
            );
            return;
        }

        state.update_segments(self.node, area, self.theme);

        let w = area.width as usize;
        let h = area.height as usize;
        if w == 0 || h == 0 {
            return;
        }

        // Braille resolution: each cell is 2 dots wide, 4 dots tall
        let dot_w = w * 2;
        let dot_h = h * 4;

        // Center in dot-space
        let cx = dot_w as f64 / 2.0;
        let cy = dot_h as f64 / 2.0;

        // Max radius in dot-space (accounting for terminal cell aspect ~1:2)
        // One cell is 2 dots wide, 4 dots tall, but visually 4 dots tall ≈ 2 dots wide
        // So we scale x by 1 and y by 0.5 for visual aspect
        let max_r = (cx.min(cy * 0.5)) * 0.9; // 90% to leave margin

        // Per-cell data: (braille_dots, dominant_color, is_selected)
        let mut cell_dots = vec![0u8; w * h];
        let mut cell_colors: Vec<Option<Color>> = vec![None; w * h];
        let mut cell_selected = vec![false; w * h];

        // Get selected segment info
        let ring_segs = state.segments_in_ring(state.selected_ring);
        let selected_seg_idx = ring_segs.get(state.selected_segment).copied();

        // Rasterize: iterate over all dots
        for dot_y in 0..dot_h {
            for dot_x in 0..dot_w {
                // Convert to visual coordinates (correct aspect ratio)
                let vx = dot_x as f64 - cx;
                let vy = (dot_y as f64 - cy) * 0.5; // compress Y for visual aspect

                let radius = (vx * vx + vy * vy).sqrt();
                let norm_r = radius / max_r;

                if norm_r > 1.0 {
                    continue;
                }

                let angle = vy.atan2(vx);
                let angle = if angle < 0.0 { angle + 2.0 * PI } else { angle };

                // Find matching segment
                for (seg_idx, seg) in state.cached_segments.iter().enumerate() {
                    if norm_r >= seg.inner_radius
                        && norm_r < seg.outer_radius
                        && angle >= seg.start_angle
                        && angle < seg.end_angle
                    {
                        // Set the braille dot
                        let cell_x = dot_x / 2;
                        let cell_y = dot_y / 4;
                        let local_x = dot_x % 2;
                        let local_y = dot_y % 4;

                        let cell_idx = cell_y * w + cell_x;
                        if cell_idx < cell_dots.len() {
                            let bit = braille_bit(local_x, local_y);
                            cell_dots[cell_idx] |= bit;
                            cell_colors[cell_idx] = Some(seg.color);
                            if selected_seg_idx == Some(seg_idx) {
                                cell_selected[cell_idx] = true;
                            }
                        }
                        break;
                    }
                }
            }
        }

        // Render braille chars to buffer
        for cy_cell in 0..h {
            for cx_cell in 0..w {
                let idx = cy_cell * w + cx_cell;
                let dots = cell_dots[idx];
                if dots == 0 {
                    continue;
                }

                let ch = char::from_u32(0x2800 + dots as u32).unwrap_or(' ');
                let color = cell_colors[idx].unwrap_or(Color::White);
                let style = if cell_selected[idx] {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                };

                let x = area.x + cx_cell as u16;
                let y = area.y + cy_cell as u16;
                if x < area.x + area.width && y < area.y + area.height {
                    buf.set_string(x, y, ch.to_string(), style);
                }
            }
        }

        // Draw center label (node name)
        let label = if self.node.name.len() > w.saturating_sub(4) {
            &self.node.name[..w.saturating_sub(4).max(1)]
        } else {
            &self.node.name
        };
        let label_x = area.x + (w as u16).saturating_sub(label.len() as u16) / 2;
        let label_y = area.y + h as u16 / 2;
        buf.set_string(
            label_x,
            label_y,
            label,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    }
}

/// Convert braille dot position (x: 0-1, y: 0-3) to bit mask.
fn braille_bit(x: usize, y: usize) -> u8 {
    // Braille dot mapping:
    // (0,0)=0x01 (1,0)=0x08
    // (0,1)=0x02 (1,1)=0x10
    // (0,2)=0x04 (1,2)=0x20
    // (0,3)=0x40 (1,3)=0x80
    match (x, y) {
        (0, 0) => 0x01,
        (0, 1) => 0x02,
        (0, 2) => 0x04,
        (0, 3) => 0x40,
        (1, 0) => 0x08,
        (1, 1) => 0x10,
        (1, 2) => 0x20,
        (1, 3) => 0x80,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_braille_bits() {
        assert_eq!(braille_bit(0, 0), 0x01);
        assert_eq!(braille_bit(1, 3), 0x80);
        // Full cell = all 8 dots
        let full: u8 = braille_bit(0, 0)
            | braille_bit(0, 1)
            | braille_bit(0, 2)
            | braille_bit(0, 3)
            | braille_bit(1, 0)
            | braille_bit(1, 1)
            | braille_bit(1, 2)
            | braille_bit(1, 3);
        assert_eq!(full, 0xFF);
        assert_eq!(char::from_u32(0x2800 + full as u32), Some('⣿'));
    }

    #[test]
    fn test_compute_segments() {
        let theme = Theme::default();
        let mut node = DiskNode::new("root".into(), 100, NodeType::Dir, 0);
        node.children.push(DiskNode::new("a".into(), 60, NodeType::File, 1));
        node.children.push(DiskNode::new("b".into(), 40, NodeType::File, 1));

        let segments = compute_segments(&node, 3, &theme);
        assert_eq!(segments.len(), 2); // two segments in ring 0

        // Angles should cover full circle
        let total_angle: f64 = segments.iter().map(|s| s.end_angle - s.start_angle).sum();
        assert!((total_angle - 2.0 * PI).abs() < 0.1);
    }

    #[test]
    fn test_sunburst_state_navigation() {
        let theme = Theme::default();
        let mut node = DiskNode::new("root".into(), 100, NodeType::Dir, 0);
        node.children.push(DiskNode::new("a".into(), 60, NodeType::File, 1));
        node.children.push(DiskNode::new("b".into(), 40, NodeType::File, 1));

        let mut state = SunburstState::new();
        state.cached_segments = compute_segments(&node, 3, &theme);

        assert_eq!(state.selected_ring, 0);
        assert_eq!(state.selected_segment, 0);

        state.move_angular_next();
        assert_eq!(state.selected_segment, 1);

        state.move_angular_next(); // wraps
        assert_eq!(state.selected_segment, 0);

        state.move_angular_prev(); // wraps back
        assert_eq!(state.selected_segment, 1);
    }
}
