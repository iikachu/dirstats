// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Treemap drawn with terminal cells: one coloured cell per unit of area,
//! laid out by `dirstats_core::treemap::layout`.

use dirstats_core::treemap::layout::{self, Rect as MapRect, Style};
use dirstats_core::treemap::{ExtensionColors, Oklch};
use dirstats_core::{NodeId, Tree};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

/// Stop subdividing below this many cells; smaller boxes read as noise.
const MIN_CELLS: i32 = 4;
/// Nodes this many levels below the root are filled as one box.
const MAX_DEPTH: u32 = 6;

/// Draw the subtree at `root` into `area` of `buffer`, one terminal cell per
/// layout unit. A `selected` child of `root` is drawn as one box with a
/// blinking outline.
pub fn render(buffer: &mut Buffer, tree: &Tree, colors: &ExtensionColors, root: NodeId, selected: Option<NodeId>, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let colour = |t: &Tree, id: NodeId| colors.color(t, id);
    let bounds = MapRect::new(0, 0, i32::from(area.width), i32::from(area.height));
    // Laid out in plain cells: terminal cells are roughly twice as tall as
    // wide, so squarified boxes come out about twice as tall as they are wide.
    let mut scratch = Vec::new();
    draw_node(buffer, tree, root, bounds, 0, selected, &colour, area, &mut scratch);
}

/// Lay out `node`'s children in `rect` and recurse, or fill `rect` with the
/// node's colour once it is a leaf, too deep or too small. `scratch` is a
/// reusable layout buffer.
#[allow(clippy::too_many_arguments)]
fn draw_node(
    buffer: &mut Buffer,
    tree: &Tree,
    node: NodeId,
    rect: MapRect,
    depth: u32,
    selected: Option<NodeId>,
    colour: &dyn Fn(&Tree, NodeId) -> Oklch,
    area: Rect,
    scratch: &mut Vec<MapRect>,
) {
    if rect.is_empty() {
        return;
    }
    let children = tree.children(node);
    let too_small = rect.width() * rect.height() < MIN_CELLS;
    if children.is_empty() || depth >= MAX_DEPTH || too_small {
        fill(buffer, area, rect, colour(tree, node), depth, selected == Some(node));
        return;
    }

    let weights: Vec<u64> = children.iter().map(|&c| tree.size(c)).collect();
    let total: u64 = weights.iter().sum();
    if total == 0 {
        fill(buffer, area, rect, colour(tree, node), depth, selected == Some(node));
        return;
    }
    let mut rects = std::mem::take(scratch);
    rects.clear();
    layout::arrange(Style::Squarified, rect, total, &weights, &mut rects);
    let child_rects: Vec<MapRect> = rects.clone();
    *scratch = rects;
    for (&child, child_rect) in children.iter().zip(child_rects) {
        // The selected top-level entry is drawn flat so it stands out.
        if depth == 0 && selected == Some(child) {
            fill(buffer, area, child_rect, colour(tree, child), depth, true);
        } else {
            draw_node(buffer, tree, child, child_rect, depth + 1, selected, colour, area, scratch);
        }
    }
}

/// Paint `rect` (relative to `area`) in `colour`, darkened by `depth`, with a
/// seam on its right and bottom edges; cells outside `area` are skipped.
fn fill(buffer: &mut Buffer, area: Rect, rect: MapRect, colour: Oklch, depth: u32, selected: bool) {
    // Deeper nodes get darker so nesting is visible without borders.
    let face = colour.lighten(-0.05 * f64::from(depth));
    let [r, g, b] = face.to_srgb();
    let bg = Color::Rgb(r, g, b);
    let [sr, sg, sb] = face.lighten(-0.2).to_srgb();
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            let cx = area.x + x as u16;
            let cy = area.y + y as u16;
            if cx >= area.right() || cy >= area.bottom() {
                continue;
            }
            let cell = &mut buffer[(cx, cy)];
            cell.set_bg(bg);
            let on_edge = x == rect.left || x + 1 == rect.right || y == rect.top || y + 1 == rect.bottom;
            // Right and bottom edges get a faint seam so adjacent boxes separate.
            let seam = x + 1 == rect.right || y + 1 == rect.bottom;
            if selected && on_edge {
                // The selected box gets a blinking border; terminals that
                // don't blink still show a bright outline.
                let symbol = match (x == rect.left || x + 1 == rect.right, y == rect.top || y + 1 == rect.bottom) {
                    (true, true) => "+",
                    (true, false) => "│",
                    (false, true) => "─",
                    (false, false) => " ",
                };
                cell.set_fg(Color::White).set_symbol(symbol).modifier.insert(Modifier::SLOW_BLINK | Modifier::BOLD);
            } else if seam {
                cell.set_fg(Color::Rgb(sr, sg, sb)).set_symbol("▏");
            } else {
                cell.set_symbol(" ");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_box_has_blinking_border() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), vec![0u8; 3000]).unwrap();
        std::fs::write(dir.path().join("b.bin"), vec![0u8; 1000]).unwrap();
        let options = dirstats_core::ScanOptions { size_metric: dirstats_core::SizeMetric::Apparent, ..Default::default() };
        let tree = dirstats_core::scan::scan(dir.path(), &options).unwrap();
        let selected = tree.children(tree.root())[0];
        let area = Rect::new(0, 0, 40, 20);
        let mut buffer = Buffer::empty(area);
        let colors = ExtensionColors::rank(&tree);
        render(&mut buffer, &tree, &colors, tree.root(), Some(selected), area);

        let blinking = buffer.content().iter().filter(|c| c.modifier.contains(Modifier::SLOW_BLINK)).count();
        let white_fill = buffer.content().iter().filter(|c| c.symbol() == "▒").count();
        assert!(blinking > 0, "selected box should have a blinking border");
        assert_eq!(white_fill, 0, "selection must not fill the box");
        // Border cells are far fewer than the box's area.
        assert!(blinking < 40 * 20 / 2);
    }
}
