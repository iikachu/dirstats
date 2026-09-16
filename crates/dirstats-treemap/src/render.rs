// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// Ported from WinDirStat windirstat/Controls/TreeMap.cpp and TreeMap.h
// (CTreeMap::DrawTreeMap, DrawCushion, AddRidge, CColorSpace)
// by WinDirStat Team (https://windirstat.net), GPL-2.0-or-later.
// Original treemap code by Bernhard Seifert. Cushion shading follows
// van Wijk & van de Wetering, "Cushion Treemaps" (1999).

//! Cushion treemap rendering into an RGBA buffer.

use crate::layout::{self, Rect, Style};
use dirstats_scan::{Kind, NodeId, Tree};
use std::hash::{Hash, Hasher};

pub type Rgb = [u8; 3];

/// Brightness the palette colors are normalized to.
const PALETTE_BRIGHTNESS: f64 = 0.6;

#[derive(Clone, Debug)]
pub struct TreemapOptions {
    pub style: Style,
    /// Leave a one-pixel grid line between leaves.
    pub grid: bool,
    pub grid_color: Rgb,
    pub background: Rgb,
    /// 0..=1
    pub brightness: f64,
    /// 0..=1
    pub saturation: f64,
    /// Ridge height "H"; 0 disables cushions.
    pub height: f64,
    /// Ridge height falloff per level "F", 0..=1.
    pub scale_factor: f64,
    /// Ambient light "Ia", 0..=1; 1 disables cushions.
    pub ambient_light: f64,
    /// Light direction, -4..=4; negative is left.
    pub light_x: f64,
    /// Light direction, -4..=4; negative is top.
    pub light_y: f64,
}

impl Default for TreemapOptions {
    /// WinDirStat's "Classic" preset.
    fn default() -> Self {
        Self {
            style: Style::Rows,
            grid: false,
            grid_color: [0, 0, 0],
            background: [0, 0, 0],
            brightness: 0.88,
            saturation: 1.0,
            height: 0.38,
            scale_factor: 0.91,
            ambient_light: 0.13,
            light_x: -1.0,
            light_y: -1.0,
        }
    }
}

impl TreemapOptions {
    fn cushion_shading(&self) -> bool {
        self.ambient_light < 1.0 && self.height > 0.0 && self.scale_factor > 0.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VisibleItem {
    pub node: NodeId,
    pub rect: Rect,
    pub depth: u32,
}

/// A rendered treemap: RGBA8 pixels plus the rectangle of every visible node.
#[derive(Clone, Debug)]
pub struct Treemap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// Parents precede their descendants.
    pub items: Vec<VisibleItem>,
}

impl Treemap {
    /// Deepest visible node at a pixel.
    // TODO: WinDirStat builds a 16px grid index for this; linear is fine for a draft.
    #[must_use]
    pub fn hit_test(&self, x: i32, y: i32) -> Option<NodeId> {
        self.items
            .iter()
            .filter(|item| item.rect.contains(x, y))
            .max_by_key(|item| item.depth)
            .map(|item| item.node)
    }
}

/// WinDirStat's default cushion palette, normalized to a common brightness.
#[must_use]
pub fn default_palette() -> Vec<Rgb> {
    const COLORS: [Rgb; 18] = [
        [0, 0, 255],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [0, 255, 255],
        [255, 0, 255],
        [255, 170, 0],
        [0, 85, 255],
        [255, 0, 85],
        [85, 255, 0],
        [170, 0, 255],
        [0, 255, 85],
        [255, 0, 170],
        [0, 170, 255],
        [255, 85, 0],
        [0, 255, 170],
        [85, 0, 255],
        [255, 255, 255],
    ];
    COLORS.iter().map(|&c| make_bright_color(c, PALETTE_BRIGHTNESS)).collect()
}

/// Color leaves by file extension.
// TODO: WinDirStat ranks extensions by total size so the biggest types get the
// most distinct colors; hashing is a stand-in.
pub fn color_by_extension(palette: &[Rgb]) -> impl Fn(&Tree, NodeId) -> Rgb + '_ {
    move |tree, id| {
        let node = tree.node(id);
        if node.kind == Kind::Directory {
            return make_bright_color([128, 128, 128], PALETTE_BRIGHTNESS);
        }
        let extension = std::path::Path::new(&*node.name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        extension.hash(&mut hasher);
        palette[(hasher.finish() % palette.len() as u64) as usize]
    }
}

struct DrawState {
    surface: [f64; 4],
    rect: Rect,
    node: NodeId,
    ridge_height: f64,
    as_root: bool,
    depth: u32,
}

/// Render the subtree at `root` into a `width` × `height` image.
pub fn render(
    tree: &Tree,
    root: NodeId,
    width: u32,
    height: u32,
    options: &TreemapOptions,
    color: impl Fn(&Tree, NodeId) -> Rgb,
) -> Treemap {
    let mut canvas = Canvas::new(width, height, options);
    let bounds = Rect::new(0, 0, width as i32, height as i32);
    let mut items = Vec::new();
    let background = if options.grid { options.grid_color } else { options.background };
    canvas.fill(bounds, background, PALETTE_BRIGHTNESS);

    if tree.size(root) == 0 {
        items.push(VisibleItem { node: root, rect: bounds, depth: 0 });
        return canvas.finish(items);
    }

    let grid_width = i32::from(options.grid);
    let cushions = options.cushion_shading();
    let mut weights = Vec::new();
    let mut regions = Vec::new();
    let mut stack = vec![DrawState {
        surface: [0.0; 4],
        rect: bounds,
        node: root,
        ridge_height: options.height,
        as_root: true,
        depth: 0,
    }];

    while let Some(mut state) = stack.pop() {
        items.push(VisibleItem { node: state.node, rect: state.rect, depth: state.depth });
        if state.rect.width() <= grid_width || state.rect.height() <= grid_width {
            continue;
        }
        if cushions && !state.as_root {
            add_ridge(state.rect, &mut state.surface, state.ridge_height);
        }

        let children = tree.children(state.node);
        if children.is_empty() {
            let mut rect = state.rect;
            if options.grid {
                rect.top += 1;
                rect.left += 1;
            }
            canvas.draw_leaf(rect, &state.surface, color(tree, state.node));
            continue;
        }

        weights.clear();
        weights.extend(children.iter().map(|&child| tree.size(child)));
        let parent_weight = weights.iter().sum();
        layout::arrange(options.style, state.rect, parent_weight, &weights, &mut regions);
        for (&child, &rect) in children.iter().zip(&regions) {
            if rect.is_empty() {
                continue;
            }
            stack.push(DrawState {
                surface: state.surface,
                rect,
                node: child,
                ridge_height: state.ridge_height * options.scale_factor,
                as_root: false,
                depth: state.depth + 1,
            });
        }
    }

    canvas.finish(items)
}

struct Canvas<'a> {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    options: &'a TreemapOptions,
    light: [f64; 3],
}

impl<'a> Canvas<'a> {
    fn new(width: u32, height: u32, options: &'a TreemapOptions) -> Self {
        let (lx, ly, lz) = (options.light_x, options.light_y, 10.0);
        let len = (lx * lx + ly * ly + lz * lz).sqrt();
        Self {
            width,
            height,
            pixels: vec![255; width as usize * height as usize * 4],
            options,
            light: [lx / len, ly / len, lz / len],
        }
    }

    fn finish(self, items: Vec<VisibleItem>) -> Treemap {
        Treemap { width: self.width, height: self.height, pixels: self.pixels, items }
    }

    fn put(&mut self, x: i32, y: i32, rgb: Rgb) {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[i..i + 3].copy_from_slice(&rgb);
    }

    fn fill(&mut self, rect: Rect, color: Rgb, brightness: f64) {
        let factor = brightness / PALETTE_BRIGHTNESS;
        let rgb = normalize_color(
            (f64::from(color[0]) * factor) as i32,
            (f64::from(color[1]) * factor) as i32,
            (f64::from(color[2]) * factor) as i32,
        );
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                self.put(x, y, rgb);
            }
        }
    }

    fn draw_leaf(&mut self, rect: Rect, surface: &[f64; 4], color: Rgb) {
        if rect.is_empty() {
            return;
        }
        let options = self.options;
        let mut color = color;
        if options.saturation < 1.0 {
            let saturation = options.saturation.max(0.0);
            let gray = color.iter().map(|&c| f64::from(c)).sum::<f64>() / 3.0;
            color = color.map(|c| (gray + (f64::from(c) - gray) * saturation) as u8);
        }
        if options.cushion_shading() {
            self.draw_cushion(rect, surface, color, options.brightness);
        } else {
            self.fill(rect, color, options.brightness);
        }
    }

    fn draw_cushion(&mut self, rect: Rect, surface: &[f64; 4], color: Rgb, brightness: f64) {
        let ambient = self.options.ambient_light;
        let diffuse = 1.0 - ambient;
        let factor = brightness / PALETTE_BRIGHTNESS;
        let [lx, ly, lz] = self.light;
        let [r, g, b] = color.map(f64::from);
        let nx_step = -2.0 * surface[0];

        for y in rect.top..rect.bottom {
            let ny = -(2.0 * surface[1] * (f64::from(y) + 0.5) + surface[3]);
            let ny_ly_lz = ny * ly + lz;
            let ny2_1 = ny * ny + 1.0;
            let mut nx = -(2.0 * surface[0] * (f64::from(rect.left) + 0.5) + surface[2]);
            for x in rect.left..rect.right {
                let cosa = ((nx * lx + ny_ly_lz) / (nx * nx + ny2_1).sqrt()).min(1.0);
                let pixel = ((diffuse * cosa).max(0.0) + ambient) * factor;
                let rgb = normalize_color((r * pixel) as i32, (g * pixel) as i32, (b * pixel) as i32);
                self.put(x, y, rgb);
                nx += nx_step;
            }
        }
    }
}

fn add_ridge(rect: Rect, surface: &mut [f64; 4], h: f64) {
    let h4 = 4.0 * h;
    let wf = h4 / f64::from(rect.width());
    surface[2] += wf * f64::from(rect.right + rect.left);
    surface[0] -= wf;
    let hf = h4 / f64::from(rect.height());
    surface[3] += hf * f64::from(rect.bottom + rect.top);
    surface[1] -= hf;
}

/// Give a color a defined average brightness (0..=1).
#[must_use]
pub fn make_bright_color(color: Rgb, brightness: f64) -> Rgb {
    let [r, g, b] = color.map(|c| f64::from(c) / 255.0);
    let sum = r + g + b;
    if sum == 0.0 {
        let v = (brightness * 255.0) as u8;
        return [v, v, v];
    }
    let f = 3.0 * brightness / sum;
    normalize_color((r * f * 255.0) as i32, (g * f * 255.0) as i32, (b * f * 255.0) as i32)
}

/// Push channel overflow above 255 into the other two channels.
fn normalize_color(mut red: i32, mut green: i32, mut blue: i32) -> Rgb {
    fn distribute(first: &mut i32, second: &mut i32, third: &mut i32) {
        let h = (*first - 255) / 2;
        *first = 255;
        *second += h;
        *third += h;
        if *second > 255 {
            *third += *second - 255;
            *second = 255;
        } else if *third > 255 {
            *second += *third - 255;
            *third = 255;
        }
    }
    if red > 255 {
        distribute(&mut red, &mut green, &mut blue);
    } else if green > 255 {
        distribute(&mut green, &mut red, &mut blue);
    } else if blue > 255 {
        distribute(&mut blue, &mut red, &mut green);
    }
    [red, green, blue].map(|c| c.clamp(0, 255) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_overflow() {
        assert_eq!(normalize_color(355, 100, 0), [255, 150, 50]);
        assert_eq!(normalize_color(10, 20, 30), [10, 20, 30]);
    }

    #[test]
    fn bright_colors_share_brightness() {
        for color in default_palette().into_iter().take(17) {
            let sum: u32 = color.iter().map(|&c| u32::from(c)).sum();
            assert!((sum as f64 / 3.0 / 255.0 - PALETTE_BRIGHTNESS).abs() < 0.02, "{color:?}");
        }
    }

    #[test]
    fn renders_scanned_tree() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), vec![0u8; 30_000]).unwrap();
        std::fs::write(dir.path().join("b.bin"), vec![0u8; 10_000]).unwrap();
        let options = dirstats_scan::ScanOptions {
            size_metric: dirstats_scan::SizeMetric::Apparent,
            ..Default::default()
        };
        let tree = dirstats_scan::scan(dir.path(), &options).unwrap();
        let palette = default_palette();
        for style in [Style::Rows, Style::Squarified] {
            let map = render(
                &tree,
                tree.root(),
                64,
                48,
                &TreemapOptions { style, ..Default::default() },
                color_by_extension(&palette),
            );
            assert_eq!(map.pixels.len(), 64 * 48 * 4);
            assert_eq!(map.items.len(), 3);
            let hit = map.hit_test(1, 1).unwrap();
            assert_ne!(hit, tree.root());
        }
    }
}
