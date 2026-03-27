use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::StatefulWidget;

use crate::model::node::{DiskNode, NodeType};
use crate::tui::theme::Theme;

use std::f64::consts::PI;

/// Angular gap between adjacent segments (radians).
const SEGMENT_GAP: f64 = 0.02;
/// Radial gap between rings (normalized, 0..1 space).
const RING_GAP: f64 = 0.015;
/// Radius of the center hole (normalized).
const CENTER_RADIUS: f64 = 0.18;

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
    pub size: u64,
    /// Index of parent segment's child_index (for color inheritance).
    pub parent_color_index: usize,
}

pub struct SunburstState {
    pub cached_segments: Vec<ArcSegment>,
    pub cached_area: Rect,
    pub max_rings: usize,
    pub selected_ring: usize,
    pub selected_segment: usize,
}

impl SunburstState {
    pub fn new() -> Self {
        Self {
            cached_segments: Vec::new(),
            cached_area: Rect::default(),
            max_rings: 4,
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

    let ring_width = (1.0 - CENTER_RADIUS) / max_rings as f64;

    #[allow(clippy::too_many_arguments)]
    fn recurse(
        children: &[DiskNode],
        parent_size: u64,
        start_angle: f64,
        angle_span: f64,
        ring: usize,
        max_rings: usize,
        ring_width: f64,
        parent_color_index: usize,
        theme: &Theme,
        segments: &mut Vec<ArcSegment>,
    ) {
        if ring >= max_rings || children.is_empty() || parent_size == 0 {
            return;
        }

        let inner_r = CENTER_RADIUS + ring as f64 * ring_width + RING_GAP;
        let outer_r = CENTER_RADIUS + (ring + 1) as f64 * ring_width - RING_GAP;

        if outer_r <= inner_r {
            return;
        }

        // Compute gap distribution — each segment gets a small angular gap
        let visible_children: Vec<(usize, &DiskNode)> = children
            .iter()
            .enumerate()
            .filter(|(_, c)| c.size > 0)
            .collect();

        if visible_children.is_empty() {
            return;
        }

        let total_gap = SEGMENT_GAP * visible_children.len() as f64;
        let available_span = (angle_span - total_gap).max(0.0);

        let mut current_angle = start_angle + SEGMENT_GAP / 2.0;

        for (i, child) in &visible_children {
            let child_span = available_span * (child.size as f64 / parent_size as f64);
            if child_span < 0.008 {
                current_angle += child_span + SEGMENT_GAP;
                continue;
            }

            // For ring 0, use child's own index for color; deeper rings inherit parent color
            let color_idx = if ring == 0 { *i } else { parent_color_index };
            let color = theme.sunburst_color(color_idx, ring);

            segments.push(ArcSegment {
                child_index: *i,
                ring,
                start_angle: current_angle,
                end_angle: current_angle + child_span,
                inner_radius: inner_r,
                outer_radius: outer_r,
                color,
                name: child.name.clone(),
                size: child.size,
                parent_color_index: color_idx,
            });

            if child.node_type == NodeType::Dir {
                recurse(
                    &child.children,
                    child.size,
                    current_angle,
                    child_span,
                    ring + 1,
                    max_rings,
                    ring_width,
                    color_idx,
                    theme,
                    segments,
                );
            }

            current_angle += child_span + SEGMENT_GAP;
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
        0,
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

        let cx = dot_w as f64 / 2.0;
        let cy = dot_h as f64 / 2.0;

        // Max radius — correct for terminal aspect ratio (~1:2 cell)
        let max_r = (cx.min(cy * 0.5)) * 0.95;

        // Per-cell buffers
        let cell_count = w * h;
        let mut cell_dots = vec![0u8; cell_count];
        let mut cell_colors: Vec<Option<Color>> = vec![None; cell_count];
        let mut cell_selected = vec![false; cell_count];

        // Selected segment identification
        let ring_segs = state.segments_in_ring(state.selected_ring);
        let selected_seg_idx = ring_segs.get(state.selected_segment).copied();

        // Rasterize all dots
        for dot_y in 0..dot_h {
            for dot_x in 0..dot_w {
                let vx = dot_x as f64 - cx;
                let vy = (dot_y as f64 - cy) * 0.5;

                let radius = (vx * vx + vy * vy).sqrt();
                let norm_r = radius / max_r;

                if !(CENTER_RADIUS * 0.85..=1.0).contains(&norm_r) {
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
                        let cell_x = dot_x / 2;
                        let cell_y = dot_y / 4;
                        let local_x = dot_x % 2;
                        let local_y = dot_y % 4;

                        let cell_idx = cell_y * w + cell_x;
                        if cell_idx < cell_count {
                            cell_dots[cell_idx] |= braille_bit(local_x, local_y);
                            // Last-write-wins for color (inner segments overwrite)
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
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
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

        // Draw center: clear area and show label
        let center_cell_x = (cx / 2.0).round() as u16;
        let center_cell_y = (cy / 4.0).round() as u16;
        let center_r_cells = (max_r * CENTER_RADIUS * 0.7 / 2.0).round() as u16;

        // Clear center circle for clean label
        let clear_start_x = center_cell_x.saturating_sub(center_r_cells);
        let clear_end_x = (center_cell_x + center_r_cells).min(w as u16);
        let clear_start_y = center_cell_y.saturating_sub(center_r_cells / 2);
        let clear_end_y = (center_cell_y + center_r_cells / 2).min(h as u16);

        for cy_c in clear_start_y..clear_end_y {
            for cx_c in clear_start_x..clear_end_x {
                let x = area.x + cx_c;
                let y = area.y + cy_c;
                if x < area.x + area.width && y < area.y + area.height {
                    buf.set_string(x, y, " ", Style::default());
                }
            }
        }

        // Center label: name + size
        let name = &self.node.name;
        let size_str = humansize::format_size(self.node.size, humansize::BINARY);
        let max_label_w = (center_r_cells * 2).min(w as u16).saturating_sub(2) as usize;

        let label = if name.len() > max_label_w {
            &name[..max_label_w]
        } else {
            name
        };

        let label_x = area.x + center_cell_x.saturating_sub(label.len() as u16 / 2);
        let label_y = area.y + center_cell_y.saturating_sub(1);
        buf.set_string(
            label_x,
            label_y,
            label,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );

        // Size below name
        let size_label = if size_str.len() > max_label_w {
            &size_str[..max_label_w]
        } else {
            &size_str
        };
        let size_x = area.x + center_cell_x.saturating_sub(size_label.len() as u16 / 2);
        let size_y = area.y + center_cell_y;
        buf.set_string(
            size_x,
            size_y,
            size_label,
            Style::default().fg(Color::DarkGray),
        );

        // Draw labels on large segments (ring 0 only, if arc span is wide enough)
        for seg in &state.cached_segments {
            if seg.ring != 0 {
                continue;
            }
            let arc_span = seg.end_angle - seg.start_angle;
            if arc_span < 0.4 {
                continue; // too small for label
            }

            let mid_angle = (seg.start_angle + seg.end_angle) / 2.0;
            let mid_r = (seg.inner_radius + seg.outer_radius) / 2.0;

            // Convert to cell coords
            let dot_px = cx + mid_r * max_r * mid_angle.cos();
            let dot_py = cy + mid_r * max_r * mid_angle.sin() * 2.0;
            let cell_lx = (dot_px / 2.0).round() as u16;
            let cell_ly = (dot_py / 4.0).round() as u16;

            let max_label = ((arc_span * mid_r * max_r / 2.0) as usize).clamp(2, 12);
            let seg_label = if seg.name.len() > max_label {
                format!("{}..", &seg.name[..max_label.saturating_sub(2)])
            } else {
                seg.name.clone()
            };

            let lx = area.x + cell_lx.saturating_sub(seg_label.len() as u16 / 2);
            let ly = area.y + cell_ly;
            if ly < area.y + area.height && lx < area.x + area.width {
                let is_sel = selected_seg_idx
                    .map(|si| state.cached_segments.get(si).map(|s| s.child_index) == Some(seg.child_index))
                    .unwrap_or(false);
                let label_style = if is_sel {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM)
                };
                buf.set_string(lx, ly, &seg_label, label_style);
            }
        }
    }
}

/// Convert braille dot position (x: 0-1, y: 0-3) to bit mask.
fn braille_bit(x: usize, y: usize) -> u8 {
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

        let segments = compute_segments(&node, 4, &theme);
        assert_eq!(segments.len(), 2); // two segments in ring 0

        // All segments should have gaps (not start at exactly 0)
        assert!(segments[0].start_angle > 0.0);

        // Segments should not overlap
        assert!(segments[0].end_angle <= segments[1].start_angle);
    }

    #[test]
    fn test_depth_segments() {
        let theme = Theme::default();
        let mut node = DiskNode::new("root".into(), 100, NodeType::Dir, 0);
        let mut sub = DiskNode::new("sub".into(), 80, NodeType::Dir, 1);
        sub.children.push(DiskNode::new("file".into(), 80, NodeType::File, 2));
        node.children.push(sub);
        node.children.push(DiskNode::new("other".into(), 20, NodeType::File, 1));

        let segments = compute_segments(&node, 4, &theme);
        // Ring 0: sub + other = 2 segments
        // Ring 1: file = 1 segment
        assert_eq!(segments.len(), 3);

        let rings: Vec<usize> = segments.iter().map(|s| s.ring).collect();
        assert_eq!(rings.iter().filter(|&&r| r == 0).count(), 2);
        assert_eq!(rings.iter().filter(|&&r| r == 1).count(), 1);
    }

    #[test]
    fn test_sunburst_state_navigation() {
        let theme = Theme::default();
        let mut node = DiskNode::new("root".into(), 100, NodeType::Dir, 0);
        node.children.push(DiskNode::new("a".into(), 60, NodeType::File, 1));
        node.children.push(DiskNode::new("b".into(), 40, NodeType::File, 1));

        let mut state = SunburstState::new();
        state.cached_segments = compute_segments(&node, 4, &theme);

        assert_eq!(state.selected_ring, 0);
        assert_eq!(state.selected_segment, 0);

        state.move_angular_next();
        assert_eq!(state.selected_segment, 1);

        state.move_angular_next();
        assert_eq!(state.selected_segment, 0);

        state.move_angular_prev();
        assert_eq!(state.selected_segment, 1);
    }
}
