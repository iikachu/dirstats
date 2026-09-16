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

pub use crate::color::{Oklch, Rgb};

/// Lightness of palette entries and of leaf faces at the default options.
pub const PALETTE_LIGHTNESS: f64 = 0.72;
/// Chroma of palette entries.
pub const PALETTE_CHROMA: f64 = 0.19;

/// Ridge height relative to WinDirStat's classic setting.
const GLOW_RIDGE: f64 = 0.4;
/// Glow shading: lightness range from fully shadowed to fully lit, in OKLCH units.
const GLOW_RANGE: f64 = 0.16;
/// Glow shading: extra lightness at the highlight peak.
const GLOW_HIGHLIGHT: f64 = 0.04;

/// How each rectangle is lit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shading {
    /// Cushion geometry (van Wijk and van de Wetering, via WinDirStat) lit
    /// softly across the whole box, with a broad highlight toward the light.
    #[default]
    Glow,
    /// Plain fills.
    Flat,
}

#[derive(Clone, Debug)]
pub struct TreemapOptions {
    pub style: Style,
    pub shading: Shading,
    /// Leave a one-pixel grid line between leaves.
    pub grid: bool,
    pub grid_color: Oklch,
    pub background: Oklch,
    /// OKLCH lightness of a leaf face before shading, 0..=1.
    pub lightness: f64,
    /// Chroma multiplier applied to leaf colours; 0 gives greys.
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
            shading: Shading::default(),
            grid: false,
            grid_color: Oklch::grey(0.0),
            background: Oklch::grey(0.0),
            lightness: PALETTE_LIGHTNESS,
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
        self.shading == Shading::Glow
            && self.ambient_light < 1.0
            && self.height > 0.0
            && self.scale_factor > 0.0
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
    /// Leaf item indices per `GRID_CELL`-pixel cell, row-major.
    grid: Vec<Vec<u32>>,
    grid_columns: usize,
}

/// Size of a hit-test grid cell in pixels (WinDirStat uses the same).
const GRID_CELL: i32 = 16;

impl Treemap {
    fn build_grid(&mut self) {
        let columns = ((self.width as i32 + GRID_CELL - 1) / GRID_CELL).max(1) as usize;
        let rows = ((self.height as i32 + GRID_CELL - 1) / GRID_CELL).max(1) as usize;
        let mut grid = vec![Vec::new(); columns * rows];
        // Only leaves are needed: the deepest item at a point is always a leaf
        // of the visible tree, and items are recorded parent-first.
        let mut is_leaf = vec![true; self.items.len()];
        let mut last_at_depth: Vec<usize> = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            last_at_depth.truncate(item.depth as usize);
            if let Some(&parent) = last_at_depth.last() {
                is_leaf[parent] = false;
            }
            last_at_depth.push(i);
        }
        for (i, item) in self.items.iter().enumerate() {
            if !is_leaf[i] || item.rect.is_empty() {
                continue;
            }
            let (c0, c1) = (item.rect.left / GRID_CELL, (item.rect.right - 1) / GRID_CELL);
            let (r0, r1) = (item.rect.top / GRID_CELL, (item.rect.bottom - 1) / GRID_CELL);
            for r in r0..=r1 {
                for c in c0..=c1 {
                    grid[r as usize * columns + c as usize].push(i as u32);
                }
            }
        }
        self.grid = grid;
        self.grid_columns = columns;
    }

    /// Deepest visible node at a pixel.
    #[must_use]
    pub fn hit_test(&self, x: i32, y: i32) -> Option<NodeId> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        let cell = &self.grid[(y / GRID_CELL) as usize * self.grid_columns + (x / GRID_CELL) as usize];
        cell.iter()
            .map(|&i| &self.items[i as usize])
            .filter(|item| item.rect.contains(x, y))
            .max_by_key(|item| item.depth)
            .map(|item| item.node)
    }

    /// The visible item for `node`, if it is on screen.
    #[must_use]
    pub fn item(&self, node: NodeId) -> Option<&VisibleItem> {
        self.items.iter().find(|item| item.node == node)
    }
}

/// Colours for leaves keyed by file extension, assigned by rank of total size.
///
/// Extensions are ranked by the bytes they account for in the tree, then
/// hues are handed out in rank order along the golden angle, so the largest
/// extensions are always far apart on the wheel and every extension gets a
/// hue of its own. This follows WinDirStat's idea of ranking extensions by
/// size; the hue assignment is ours.
#[derive(Clone, Debug)]
pub struct ExtensionColors {
    colors: foldhash::HashMap<Option<String>, Oklch>,
    /// (extension, total bytes, colour), largest first.
    ranked: Vec<(Option<String>, u64, Oklch)>,
    directory: Oklch,
}

impl ExtensionColors {
    /// Rank every extension in `tree` and assign hues.
    #[must_use]
    pub fn rank(tree: &Tree) -> Self {
        let mut totals: foldhash::HashMap<Option<String>, u64> = foldhash::HashMap::default();
        for (id, node) in tree.nodes() {
            if node.kind != Kind::Directory {
                *totals.entry(extension_of(node)).or_default() += tree.size(id);
            }
        }
        let mut ranked: Vec<_> = totals.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        // Golden-angle steps never repeat and keep every prefix well spread.
        const GOLDEN_ANGLE: f64 = 137.507_764_05;
        let ranked: Vec<_> = ranked
            .into_iter()
            .enumerate()
            .map(|(i, (ext, total))| {
                (ext, total, Oklch::new(PALETTE_LIGHTNESS, PALETTE_CHROMA, (i as f64 * GOLDEN_ANGLE) % 360.0))
            })
            .collect();
        let colors = ranked.iter().map(|(ext, _, color)| (ext.clone(), *color)).collect();
        Self { colors, ranked, directory: Oklch::grey(PALETTE_LIGHTNESS) }
    }

    /// Extensions largest first with their total bytes and colour; `None` is "no extension".
    #[must_use]
    pub fn entries(&self) -> &[(Option<String>, u64, Oklch)] {
        &self.ranked
    }

    #[must_use]
    pub fn color(&self, tree: &Tree, id: NodeId) -> Oklch {
        let node = tree.node(id);
        if node.kind == Kind::Directory {
            return self.directory;
        }
        self.colors.get(&extension_of(node)).copied().unwrap_or(self.directory)
    }

    /// Number of distinct extensions ranked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }
}

fn extension_of(node: &dirstats_scan::Node) -> Option<String> {
    std::path::Path::new(&*node.name).extension().map(|e| e.to_string_lossy().to_lowercase())
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
    color: impl Fn(&Tree, NodeId) -> Oklch,
) -> Treemap {
    let mut canvas = Canvas::new(width, height, options);
    let bounds = Rect::new(0, 0, width as i32, height as i32);
    let mut items = Vec::new();
    let background = if options.grid { options.grid_color } else { options.background };
    canvas.fill(bounds, background);

    if tree.size(root) == 0 {
        items.push(VisibleItem { node: root, rect: bounds, depth: 0 });
        return canvas.finish(items);
    }

    let grid_width = i32::from(options.grid);
    let cushions = options.cushion_shading();
    let ridge_height = options.height * GLOW_RIDGE;
    let mut weights = Vec::new();
    let mut regions = Vec::new();
    let mut stack = vec![DrawState {
        surface: [0.0; 4],
        rect: bounds,
        node: root,
        ridge_height,
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

/// One leaf to rasterise; collected during layout, drawn afterwards.
#[derive(Clone, Copy)]
struct Job {
    rect: Rect,
    surface: [f64; 4],
    color: Oklch,
    glow: bool,
}

struct Canvas<'a> {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    options: &'a TreemapOptions,
    light: [f64; 3],
    jobs: Vec<Job>,
}

/// Rows per rasterisation band; each band is drawn independently.
const BAND_ROWS: i32 = 32;

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
            jobs: Vec::new(),
        }
    }

    fn finish(mut self, items: Vec<VisibleItem>) -> Treemap {
        self.rasterise();
        let mut map = Treemap {
            width: self.width,
            height: self.height,
            pixels: self.pixels,
            items,
            grid: Vec::new(),
            grid_columns: 1,
        };
        map.build_grid();
        map
    }

    fn fill(&mut self, rect: Rect, color: Oklch) {
        let rgb = color.to_srgb();
        let width = self.width as usize;
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let i = (y as usize * width + x as usize) * 4;
                self.pixels[i..i + 3].copy_from_slice(&rgb);
            }
        }
    }

    /// Draw every collected job, one horizontal band at a time. Bands are
    /// independent, so with the `parallel` feature they run on all cores.
    fn rasterise(&mut self) {
        let (width, height) = (self.width as i32, self.height as i32);
        if width == 0 || height == 0 || self.jobs.is_empty() {
            return;
        }
        let bands = ((height + BAND_ROWS - 1) / BAND_ROWS) as usize;
        let mut by_band: Vec<Vec<usize>> = vec![Vec::new(); bands];
        for (i, job) in self.jobs.iter().enumerate() {
            let (b0, b1) = (job.rect.top / BAND_ROWS, (job.rect.bottom - 1) / BAND_ROWS);
            for b in b0..=b1 {
                by_band[b as usize].push(i);
            }
        }
        let light = self.light;
        let jobs = &self.jobs;
        let band_bytes = BAND_ROWS as usize * width as usize * 4;
        let draw_band = |(band, pixels): (usize, &mut [u8])| {
            let top = band as i32 * BAND_ROWS;
            let bottom = (top + BAND_ROWS).min(height);
            for &i in &by_band[band] {
                shade_job(&jobs[i], light, width, top, bottom, pixels);
            }
        };
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            self.pixels.par_chunks_mut(band_bytes).enumerate().for_each(draw_band);
        }
        #[cfg(not(feature = "parallel"))]
        self.pixels.chunks_mut(band_bytes).enumerate().for_each(draw_band);
    }

    fn draw_leaf(&mut self, rect: Rect, surface: &[f64; 4], color: Oklch) {
        if rect.is_empty() {
            return;
        }
        let options = self.options;
        // The palette supplies hue and chroma; the options set the face lightness.
        let color = color.scale_chroma(options.saturation).with_lightness(options.lightness * color.l / PALETTE_LIGHTNESS);
        let glow = options.shading == Shading::Glow && options.cushion_shading();
        self.jobs.push(Job { rect, surface: *surface, color, glow });
    }
}

/// Shade one job's pixels within the band `top..bottom`; `pixels` is that band.
fn shade_job(job: &Job, light: [f64; 3], width: i32, top: i32, bottom: i32, pixels: &mut [u8]) {
    let rect = job.rect;
    let (y0, y1) = (rect.top.max(top), rect.bottom.min(bottom));
    if y0 >= y1 {
        return;
    }
    let put = |pixels: &mut [u8], x: i32, y: i32, rgb: Rgb| {
        let i = ((y - top) as usize * width as usize + x as usize) * 4;
        pixels[i..i + 3].copy_from_slice(&rgb);
    };
    if !job.glow {
        let rgb = job.color.to_srgb();
        for y in y0..y1 {
            for x in rect.left..rect.right {
                put(pixels, x, y, rgb);
            }
        }
        return;
    }
    let [lx, ly, lz] = light;
    let surface = job.surface;
    let nx_step = -2.0 * surface[0];
    for y in y0..y1 {
        let ny = -(2.0 * surface[1] * (f64::from(y) + 0.5) + surface[3]);
        let ny_ly_lz = ny * ly + lz;
        let ny2_1 = ny * ny + 1.0;
        let mut nx = -(2.0 * surface[0] * (f64::from(rect.left) + 0.5) + surface[2]);
        for x in rect.left..rect.right {
            let cosa = ((nx * lx + ny_ly_lz) / (nx * nx + ny2_1).sqrt()).clamp(0.0, 1.0);
            // Lightness moves additively around the face, so hue and chroma
            // stay put. The diffuse term is eased so light spreads over the
            // whole box; a broad, weak highlight adds the glow.
            let lit = cosa * cosa * (3.0 - 2.0 * cosa) - 0.5;
            let highlight = cosa * cosa * GLOW_HIGHLIGHT;
            put(pixels, x, y, job.color.lighten(GLOW_RANGE * lit + highlight).to_srgb());
            nx += nx_step;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranked_extensions_get_distinct_hues() {
        let dir = tempfile::tempdir().unwrap();
        for (name, size) in [("a.rs", 5000), ("b.rs", 4000), ("c.png", 3000), ("d.txt", 100), ("e", 50)] {
            std::fs::write(dir.path().join(name), vec![0u8; size]).unwrap();
        }
        let options = dirstats_scan::ScanOptions {
            size_metric: dirstats_scan::SizeMetric::Apparent,
            ..Default::default()
        };
        let tree = dirstats_scan::scan(dir.path(), &options).unwrap();
        let colors = ExtensionColors::rank(&tree);
        assert_eq!(colors.len(), 4, "rs, png, txt and no extension");
        let by_name = |n: &str| {
            let id = tree.children(tree.root()).iter().copied().find(|&c| &*tree.node(c).name == std::ffi::OsStr::new(n)).unwrap();
            colors.color(&tree, id)
        };
        assert_eq!(by_name("a.rs"), by_name("b.rs"));
        assert_eq!(by_name("a.rs").h, 0.0, "largest extension gets the first hue");
        let hues = [by_name("a.rs").h, by_name("c.png").h, by_name("d.txt").h];
        for (i, a) in hues.iter().enumerate() {
            for b in &hues[i + 1..] {
                let gap = (a - b).abs();
                assert!(gap.min(360.0 - gap) > 60.0, "{hues:?}");
            }
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
        let colors = ExtensionColors::rank(&tree);
        for style in [Style::Rows, Style::Squarified] {
            let map = render(
                &tree,
                tree.root(),
                64,
                48,
                &TreemapOptions { style, ..Default::default() },
                |t, id| colors.color(t, id),
            );
            assert_eq!(map.pixels.len(), 64 * 48 * 4);
            assert_eq!(map.items.len(), 3);
            let hit = map.hit_test(1, 1).unwrap();
            assert_ne!(hit, tree.root());
        }
    }
}
