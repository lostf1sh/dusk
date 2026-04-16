use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::StatefulWidget;

use crate::model::node::{DiskNode, NodeType};
use crate::tui::filter::FilterCriteria;
use crate::tui::text::truncate_to_width;
use crate::tui::theme::Theme;
use crate::tui::views::tree::filter_visible_child_indices;

/// A rectangle in the treemap layout.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TreemapRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub child_index: usize,
    pub name: String,
    pub size: u64,
    pub node_type: NodeType,
}

pub struct TreemapState {
    pub cached_rects: Vec<TreemapRect>,
    pub cached_area: Rect,
    cached_visible: Vec<usize>,
}

impl Default for TreemapState {
    fn default() -> Self {
        Self::new()
    }
}

impl TreemapState {
    pub fn new() -> Self {
        Self {
            cached_rects: Vec::new(),
            cached_area: Rect::default(),
            cached_visible: Vec::new(),
        }
    }

    /// Recompute layout if area, visible children, or cache state changed.
    pub fn update_layout(&mut self, node: &DiskNode, area: Rect, visible_indices: &[usize]) {
        if self.cached_area == area
            && self.cached_visible == visible_indices
            && !self.cached_rects.is_empty()
        {
            return;
        }
        self.cached_rects = squarify_layout_filtered(node, area, visible_indices);
        self.cached_area = area;
        self.cached_visible = visible_indices.to_vec();
    }

    pub fn invalidate(&mut self) {
        self.cached_rects.clear();
        self.cached_visible.clear();
    }

    /// Spatial navigation from the rect for `selected_child_index` (index into `node.children`).
    /// Returns a child index in `node.children`, not a rect list index.
    pub fn navigate(&self, selected_child_index: usize, dx: i16, dy: i16) -> usize {
        if self.cached_rects.is_empty() {
            return selected_child_index;
        }
        let start_rect_idx = self
            .cached_rects
            .iter()
            .position(|r| r.child_index == selected_child_index)
            .unwrap_or(0);
        let sel = match self.cached_rects.get(start_rect_idx) {
            Some(r) => r,
            None => return selected_child_index,
        };
        let cx = sel.x as i16 + sel.width as i16 / 2;
        let cy = sel.y as i16 + sel.height as i16 / 2;

        let target_x = cx + dx * 10;
        let target_y = cy + dy * 5;

        let mut best = start_rect_idx;
        let mut best_dist = i32::MAX;

        for (i, r) in self.cached_rects.iter().enumerate() {
            if i == start_rect_idx {
                continue;
            }
            let rx = r.x as i16 + r.width as i16 / 2;
            let ry = r.y as i16 + r.height as i16 / 2;

            // Check direction constraint
            let in_direction = match (dx, dy) {
                (1, 0) => rx > cx,
                (-1, 0) => rx < cx,
                (0, 1) => ry > cy,
                (0, -1) => ry < cy,
                _ => true,
            };
            if !in_direction {
                continue;
            }

            let dist = (rx as i32 - target_x as i32).pow(2) + (ry as i32 - target_y as i32).pow(2);
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        self.cached_rects
            .get(best)
            .map(|r| r.child_index)
            .unwrap_or(selected_child_index)
    }
}

pub struct TreemapView<'a> {
    pub node: &'a DiskNode,
    pub theme: &'a Theme,
    /// Child index in `node.children` (not filtered list index).
    pub selected_child_index: usize,
    /// Same subset as bar/tree filter (indices into `node.children`).
    pub visible_indices: &'a [usize],
}

impl StatefulWidget for TreemapView<'_> {
    type State = TreemapState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if self.node.children.is_empty() {
            buf.set_string(
                area.x + 1,
                area.y + area.height / 2,
                "(empty directory)",
                self.theme.status_style,
            );
            return;
        }

        state.update_layout(self.node, area, self.visible_indices);

        for rect in &state.cached_rects {
            if rect.width == 0 || rect.height == 0 {
                continue;
            }

            let is_selected = rect.child_index == self.selected_child_index;
            let color = self.theme.segment_color(rect.child_index);

            let base_style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };

            let fill_char = if is_selected { '█' } else { '▓' };

            // Fill the rectangle
            for dy in 0..rect.height {
                for dx in 0..rect.width {
                    let x = rect.x + dx;
                    let y = rect.y + dy;
                    if x < area.x + area.width && y < area.y + area.height {
                        buf.set_string(x, y, fill_char.to_string(), base_style);
                    }
                }
            }

            // Draw name label if enough space
            if rect.width >= 3 && rect.height >= 1 {
                let label = truncate_to_width(&rect.name, rect.width as usize - 1);
                let label_style = if is_selected {
                    Style::default()
                        .fg(Color::White)
                        .bg(color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White).bg(Color::Reset)
                };
                buf.set_string(rect.x, rect.y, &label, label_style);
            }

            // Draw size label on second row if space
            if rect.width >= 5 && rect.height >= 2 {
                let size_label = humansize::format_size(rect.size, humansize::BINARY);
                let size_label = truncate_to_width(&size_label, rect.width as usize - 1);
                let size_style = if is_selected {
                    Style::default().fg(Color::LightYellow).bg(color)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                buf.set_string(rect.x, rect.y + 1, &size_label, size_style);
            }
        }
    }
}

/// Child indices that are both filter-visible and drawable in the treemap.
pub fn visible_treemap_child_indices(
    node: &DiskNode,
    filter: Option<&FilterCriteria>,
) -> Vec<usize> {
    filter_visible_child_indices(node, filter)
        .into_iter()
        .filter(|&i| node.children.get(i).is_some_and(|child| child.size > 0))
        .collect()
}

/// Layout only the given child indices (e.g. after applying the same filter as the tree view).
pub fn squarify_layout_filtered(
    node: &DiskNode,
    area: Rect,
    visible_indices: &[usize],
) -> Vec<TreemapRect> {
    if node.children.is_empty() || area.width == 0 || area.height == 0 {
        return Vec::new();
    }

    let items: Vec<(usize, u64)> = visible_indices
        .iter()
        .filter_map(|&i| {
            let c = node.children.get(i)?;
            (c.size > 0).then_some((i, c.size))
        })
        .collect();

    let total_size: u64 = items.iter().map(|(_, s)| *s).sum();
    if total_size == 0 {
        return Vec::new();
    }

    let mut rects = Vec::with_capacity(items.len());
    squarify_recurse(
        &items,
        total_size,
        area.x as f64,
        area.y as f64,
        area.width as f64,
        area.height as f64,
        node,
        &mut rects,
    );
    rects
}

#[allow(clippy::too_many_arguments)]
fn squarify_recurse(
    items: &[(usize, u64)],
    total_size: u64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    node: &DiskNode,
    rects: &mut Vec<TreemapRect>,
) {
    if items.is_empty() || w < 1.0 || h < 1.0 || total_size == 0 {
        return;
    }

    if items.len() == 1 {
        let (idx, _) = items[0];
        let child = &node.children[idx];
        rects.push(TreemapRect {
            x: x.round() as u16,
            y: y.round() as u16,
            width: w.round().max(1.0) as u16,
            height: h.round().max(1.0) as u16,
            child_index: idx,
            name: child.name.clone(),
            size: child.size,
            node_type: child.node_type.clone(),
        });
        return;
    }

    // Aspect ratio correction: terminal cells are ~1:2
    let visual_w = w;
    let visual_h = h * 2.0;
    let layout_vertical = visual_w >= visual_h; // lay out row along shorter visual side

    let total_area = w * h;

    // Find the best split point using the squarified algorithm
    let mut best_split = 1;
    let mut best_ratio = f64::MAX;

    let mut row_size: u64 = 0;
    for i in 0..items.len() {
        row_size += items[i].1;
        let ratio = worst_aspect_ratio(
            &items[..=i],
            row_size,
            total_size,
            total_area,
            w,
            h,
            layout_vertical,
        );
        if ratio <= best_ratio {
            best_ratio = ratio;
            best_split = i + 1;
        } else {
            break; // ratio getting worse, stop
        }
    }

    let row_items = &items[..best_split];
    let remaining_items = &items[best_split..];

    let row_total: u64 = row_items.iter().map(|(_, s)| *s).sum();
    let row_fraction = row_total as f64 / total_size as f64;

    if layout_vertical {
        // Row occupies a vertical strip on the left
        let row_w = (w * row_fraction).max(1.0);
        let mut cy = y;
        for &(idx, size) in row_items {
            let item_fraction = size as f64 / row_total as f64;
            let item_h = (h * item_fraction).max(1.0);
            let child = &node.children[idx];
            rects.push(TreemapRect {
                x: x.round() as u16,
                y: cy.round() as u16,
                width: row_w.round().max(1.0) as u16,
                height: item_h.round().max(1.0) as u16,
                child_index: idx,
                name: child.name.clone(),
                size: child.size,
                node_type: child.node_type.clone(),
            });
            cy += item_h;
        }
        let remaining_size = total_size - row_total;
        squarify_recurse(
            remaining_items,
            remaining_size,
            x + row_w,
            y,
            w - row_w,
            h,
            node,
            rects,
        );
    } else {
        // Row occupies a horizontal strip on top
        let row_h = (h * row_fraction).max(1.0);
        let mut cx = x;
        for &(idx, size) in row_items {
            let item_fraction = size as f64 / row_total as f64;
            let item_w = (w * item_fraction).max(1.0);
            let child = &node.children[idx];
            rects.push(TreemapRect {
                x: cx.round() as u16,
                y: y.round() as u16,
                width: item_w.round().max(1.0) as u16,
                height: row_h.round().max(1.0) as u16,
                child_index: idx,
                name: child.name.clone(),
                size: child.size,
                node_type: child.node_type.clone(),
            });
            cx += item_w;
        }
        let remaining_size = total_size - row_total;
        squarify_recurse(
            remaining_items,
            remaining_size,
            x,
            y + row_h,
            w,
            h - row_h,
            node,
            rects,
        );
    }
}

fn worst_aspect_ratio(
    row: &[(usize, u64)],
    row_total: u64,
    total_size: u64,
    _total_area: f64,
    w: f64,
    h: f64,
    layout_vertical: bool,
) -> f64 {
    if row_total == 0 || total_size == 0 {
        return f64::MAX;
    }

    let row_fraction = row_total as f64 / total_size as f64;

    let mut worst = 0.0f64;
    for &(_, size) in row {
        let item_fraction = size as f64 / row_total as f64;

        let (item_w, item_h) = if layout_vertical {
            let rw = w * row_fraction;
            let rh = h * item_fraction;
            (rw, rh)
        } else {
            let rw = w * item_fraction;
            let rh = h * row_fraction;
            (rw, rh)
        };

        // Visual aspect ratio (correct for terminal cell ~1:2)
        let visual_w = item_w;
        let visual_h = item_h * 2.0;
        let ratio = if visual_w > visual_h {
            visual_w / visual_h
        } else {
            visual_h / visual_w
        };
        worst = worst.max(ratio);
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::DiskNode;

    fn sample_node() -> DiskNode {
        let mut root = DiskNode::new("root".into(), 100, NodeType::Dir, 0);
        root.children
            .push(DiskNode::new("big".into(), 60, NodeType::Dir, 1));
        root.children
            .push(DiskNode::new("med".into(), 30, NodeType::File, 1));
        root.children
            .push(DiskNode::new("sml".into(), 10, NodeType::File, 1));
        root
    }

    #[test]
    fn test_squarify_produces_rects() {
        let node = sample_node();
        let area = Rect::new(0, 0, 40, 20);
        let indices: Vec<usize> = (0..node.children.len()).collect();
        let rects = squarify_layout_filtered(&node, area, &indices);

        assert_eq!(rects.len(), 3);
        // Largest child should get the most area
        let big_rect = rects.iter().find(|r| r.name == "big").unwrap();
        let sml_rect = rects.iter().find(|r| r.name == "sml").unwrap();
        let big_area = big_rect.width as u32 * big_rect.height as u32;
        let sml_area = sml_rect.width as u32 * sml_rect.height as u32;
        assert!(big_area > sml_area);
    }

    #[test]
    fn test_squarify_empty() {
        let node = DiskNode::new("empty".into(), 0, NodeType::Dir, 0);
        let rects = squarify_layout_filtered(&node, Rect::new(0, 0, 40, 20), &[]);
        assert!(rects.is_empty());
    }

    #[test]
    fn test_spatial_navigation() {
        let node = sample_node();
        let area = Rect::new(0, 0, 40, 20);
        let mut state = TreemapState::new();
        let visible: Vec<usize> = (0..node.children.len()).collect();
        state.update_layout(&node, area, &visible);

        // Should be able to navigate between rects (navigate returns child index)
        let next = state.navigate(0, 1, 0); // move right from first child
        assert!(next < node.children.len());
    }

    #[test]
    fn test_visible_treemap_child_indices_skip_zero_sized() {
        let mut node = DiskNode::new("root".into(), 10, NodeType::Dir, 0);
        node.children
            .push(DiskNode::new("zero".into(), 0, NodeType::File, 1));
        node.children
            .push(DiskNode::new("ten".into(), 10, NodeType::File, 1));

        assert_eq!(visible_treemap_child_indices(&node, None), vec![1]);
    }

    #[test]
    fn test_unicode_labels_do_not_panic_when_truncated() {
        let mut node = DiskNode::new("root".into(), 10, NodeType::Dir, 0);
        node.children.push(DiskNode::new(
            "日本語ファイル名".into(),
            10,
            NodeType::File,
            1,
        ));

        let rects = squarify_layout_filtered(&node, Rect::new(0, 0, 4, 2), &[0]);
        assert_eq!(rects.len(), 1);

        let label = truncate_to_width(&rects[0].name, 3);
        assert!(!label.is_empty());
    }
}
