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

/// Layout, colour and lighting settings for [`render()`].
#[derive(Clone, Debug)]
pub struct TreemapOptions {
    /// How siblings are laid out.
    pub style: Style,
    /// How faces are lit.
    pub shading: Shading,
    /// Leave a one-pixel grid line between leaves.
    pub grid: bool,
    /// Colour of the grid lines, and of any area no leaf covers, when `grid` is on.
    pub grid_color: Oklch,
    /// Colour of any area no leaf covers when `grid` is off.
    pub background: Oklch,
    /// OKLCH lightness of a leaf face before shading, 0..=1, for a colour at
    /// [`PALETTE_LIGHTNESS`]; other colours are scaled by the same ratio.
    pub lightness: f64,
    /// Chroma multiplier applied to leaf colours; 0 gives greys.
    pub saturation: f64,
    /// Ridge height "H" (scaled down for the glow); 0 disables cushions.
    pub height: f64,
    /// Ridge height falloff per level "F", 0..=1; 0 disables cushions.
    pub scale_factor: f64,
    /// WinDirStat's ambient light "Ia", 0..=1. Glow shading does not use
    /// the value; 1 or more disables cushions.
    pub ambient_light: f64,
    /// Horizontal light direction, -4..=4 (not enforced); negative is from the left.
    pub light_x: f64,
    /// Vertical light direction, -4..=4 (not enforced); negative is from the top.
    pub light_y: f64,
}

impl Default for TreemapOptions {
    /// Rows layout and cushion parameters from WinDirStat's "Classic"
    /// preset, with glow shading at [`PALETTE_LIGHTNESS`] on black.
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
    /// Whether faces get cushion geometry: glow shading with settings that
    /// leave a ridge to light.
    fn cushion_shading(&self) -> bool {
        self.shading == Shading::Glow && self.ambient_light < 1.0 && self.height > 0.0 && self.scale_factor > 0.0
    }
}

/// A node that got a box in a [`Treemap`].
#[derive(Clone, Copy, Debug)]
pub struct VisibleItem {
    /// The node this box shows.
    pub node: NodeId,
    /// The node's box in image pixels, grid line included.
    pub rect: Rect,
    /// Levels below the rendered root, which is 0.
    pub depth: u32,
    /// No visible descendants, so hit tests land here. The box is painted as
    /// one face only if the node has no children and the box is wider and
    /// taller than the grid line; otherwise the background shows through.
    pub leaf: bool,
    /// Cushion surface `[x², y², x, y]` coefficients the face was shaded
    /// with, zero if it has no face; see [`Treemap::shade_leaves`].
    pub surface: [f64; 4],
}

/// A rendered treemap: RGBA8 pixels plus the rectangle of every visible node.
#[derive(Clone, Debug)]
pub struct Treemap {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// `width * height` RGBA8 pixels, row-major from the top left, all opaque.
    pub pixels: Vec<u8>,
    /// Parents precede their descendants.
    pub items: Vec<VisibleItem>,
    /// Leaf item indices per `GRID_CELL`-pixel cell, row-major.
    grid: Vec<Vec<u32>>,
    /// Cells per row of `grid`.
    grid_columns: usize,
    /// Node to its index in `items` (WinDirStat keeps the same map beside its item list).
    index: foldhash::HashMap<NodeId, u32>,
}

/// Size of a hit-test grid cell in pixels (WinDirStat uses the same).
const GRID_CELL: i32 = 16;

impl Treemap {
    /// Derive the lookups from `items`: clear `leaf` on every item with a
    /// child item, fill `index`, and bucket the leaves into `grid`.
    fn build_grid(&mut self) {
        let columns = ((self.width as i32 + GRID_CELL - 1) / GRID_CELL).max(1) as usize;
        let rows = ((self.height as i32 + GRID_CELL - 1) / GRID_CELL).max(1) as usize;
        let mut grid = vec![Vec::new(); columns * rows];
        // Only leaves are needed: the deepest item at a point is always a leaf
        // of the visible tree, and items are recorded parent-first.
        let mut last_at_depth: Vec<usize> = Vec::new();
        for i in 0..self.items.len() {
            last_at_depth.truncate(self.items[i].depth as usize);
            if let Some(&parent) = last_at_depth.last() {
                self.items[parent].leaf = false;
            }
            last_at_depth.push(i);
        }
        self.index = self.items.iter().enumerate().map(|(i, item)| (item.node, i as u32)).collect();
        for (i, item) in self.items.iter().enumerate() {
            if !item.leaf || item.rect.is_empty() {
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

    /// Deepest visible node at a pixel; `None` outside the image.
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
        self.item_index(node).map(|i| &self.items[i])
    }

    /// Index into [`Treemap::items`] for `node`, if it is on screen.
    #[must_use]
    pub fn item_index(&self, node: NodeId) -> Option<usize> {
        self.index.get(&node).map(|&i| i as usize)
    }

    /// Shade the leaves `leaves` (item index and colour) into an RGBA buffer
    /// covering `bounds`, transparent elsewhere, using the same cushion
    /// geometry as the base render so an overlay keeps the glow. Cost is
    /// proportional to the area of the leaves, not the map. `bounds` is in
    /// image pixels; the buffer is its width × height, row-major. Colours get
    /// the same `saturation` and `lightness` treatment as in [`render()`].
    ///
    /// # Panics
    ///
    /// If an index is out of range for [`Treemap::items`].
    #[must_use]
    pub fn shade_leaves(&self, options: &TreemapOptions, bounds: Rect, leaves: impl IntoIterator<Item = (usize, Oklch)>) -> Vec<u8> {
        let (width, height) = (bounds.width().max(0), bounds.height().max(0));
        let mut pixels = vec![0; width as usize * height as usize * 4];
        if bounds.is_empty() {
            return pixels;
        }
        let light = light_of(options);
        for (index, color) in leaves {
            let item = &self.items[index];
            let job = leaf_job(leaf_face(item.rect, options), &item.surface, color, options);
            shade_job(&job, light, bounds.left, width, bounds.top, bounds.bottom, &mut pixels);
        }
        pixels
    }

    /// Items under `node` on screen, `node` first, in drawing order.
    /// Items are stored parent-first, so a subtree is one contiguous run.
    #[must_use]
    pub fn subtree(&self, node: NodeId) -> &[VisibleItem] {
        let Some(start) = self.item_index(node) else { return &[] };
        let depth = self.items[start].depth;
        let end = self.items[start + 1..].iter().position(|item| item.depth <= depth).map_or(self.items.len(), |n| start + 1 + n);
        &self.items[start..end]
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
            .map(|(i, (ext, total))| (ext, total, Oklch::new(PALETTE_LIGHTNESS, PALETTE_CHROMA, (i as f64 * GOLDEN_ANGLE) % 360.0)))
            .collect();
        let colors = ranked.iter().map(|(ext, _, color)| (ext.clone(), *color)).collect();
        Self { colors, ranked, directory: Oklch::grey(PALETTE_LIGHTNESS) }
    }

    /// Lower-cased extension of a node's name, as used for ranking; `None` for no extension.
    #[must_use]
    pub fn extension(node: &dirstats_scan::Node) -> Option<String> {
        extension_of(node)
    }

    /// Extensions largest first with their total bytes and colour; `None` is "no extension".
    #[must_use]
    pub fn entries(&self) -> &[(Option<String>, u64, Oklch)] {
        &self.ranked
    }

    /// Colour for node `id`: its extension's hue, or neutral grey for
    /// directories and extensions this ranking has not seen.
    #[must_use]
    pub fn color(&self, tree: &Tree, id: NodeId) -> Oklch {
        let node = tree.node(id);
        if node.kind == Kind::Directory {
            return self.directory;
        }
        self.colors.get(&extension_of(node)).copied().unwrap_or(self.directory)
    }

    /// Colour of the extension at `rank` in [`Self::entries`]; neutral grey
    /// if `rank` is out of range.
    #[must_use]
    pub fn color_at(&self, rank: usize) -> Oklch {
        self.ranked.get(rank).map_or(self.directory, |(_, _, c)| *c)
    }

    /// Per-node breakdown of bytes by extension, for drawing size bars as
    /// stacked colour segments.
    ///
    /// One bottom-up pass over the tree: every node's full tally is merged
    /// into its parent's, so totals stay exact, and only the copy kept for
    /// the node is cut to its `keep` largest extensions. The work is linear
    /// in the number of nodes times the extension count and the result is a
    /// few entries per node. Nodes are stored
    /// parent-first, so walking indices in reverse visits children before
    /// their parents.
    #[must_use]
    pub fn mix(&self, tree: &Tree, keep: usize) -> ExtensionMix {
        let rank_of: foldhash::HashMap<&Option<String>, u32> =
            self.ranked.iter().enumerate().map(|(i, (ext, _, _))| (ext, i as u32)).collect();
        let n = tree.len();
        let mut tallies: Vec<Option<foldhash::HashMap<u32, u64>>> = (0..n).map(|_| None).collect();
        let mut segments: Vec<Vec<(u32, u64)>> = (0..n).map(|_| Vec::new()).collect();
        let ids: Vec<NodeId> = tree.nodes().map(|(id, _)| id).collect();
        for &id in ids.iter().rev() {
            let index = id.index();
            let node = tree.node(id);
            let mut tally = tallies[index].take().unwrap_or_default();
            if node.kind != Kind::Directory {
                let size = tree.size(id);
                if size > 0 {
                    let rank = rank_of.get(&extension_of(node)).copied().unwrap_or(u32::MAX);
                    *tally.entry(rank).or_default() += size;
                }
            }
            if !tally.is_empty() {
                let mut top: Vec<(u32, u64)> = tally.iter().map(|(&r, &b)| (r, b)).collect();
                top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                top.truncate(keep);
                segments[index] = top;
            }
            if let Some(parent) = node.parent {
                debug_assert!(parent.index() < index, "nodes are stored parent-first");
                match &mut tallies[parent.index()] {
                    None => tallies[parent.index()] = Some(tally),
                    Some(into) => {
                        for (rank, bytes) in tally {
                            *into.entry(rank).or_default() += bytes;
                        }
                    }
                }
            }
        }
        ExtensionMix { segments }
    }

    /// Number of distinct extensions ranked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// True if the tree had no files, symlinks or other non-directories.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }
}

/// The largest extensions below each node with their bytes, computed by
/// [`ExtensionColors::mix`]. Ranks index [`ExtensionColors::entries`].
#[derive(Clone, Debug)]
pub struct ExtensionMix {
    segments: Vec<Vec<(u32, u64)>>,
}

impl ExtensionMix {
    /// `(rank, bytes)` pairs for `id`, largest first. Bytes not covered by
    /// the returned pairs belong to smaller extensions.
    #[must_use]
    pub fn segments(&self, id: NodeId) -> &[(u32, u64)] {
        self.segments.get(id.index()).map_or(&[], Vec::as_slice)
    }
}

fn extension_of(node: &dirstats_scan::Node) -> Option<String> {
    std::path::Path::new(&*node.name).extension().map(|e| e.to_string_lossy().to_lowercase())
}

/// One box waiting to be laid out during [`render()`].
struct DrawState {
    surface: [f64; 4],
    rect: Rect,
    node: NodeId,
    ridge_height: f64,
    /// The rendered root gets no ridge of its own.
    as_root: bool,
    depth: u32,
}

/// Render the subtree at `root` into a `width` × `height` image.
///
/// `color` gives each leaf's base colour; `options` adjust and shade it. Nodes
/// whose box comes out empty are not recorded. An empty `root` is one leaf
/// item painted with the background.
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
        items.push(VisibleItem { node: root, rect: bounds, depth: 0, leaf: true, surface: [0.0; 4] });
        return canvas.finish(items);
    }

    let grid_width = i32::from(options.grid);
    let cushions = options.cushion_shading();
    let ridge_height = options.height * GLOW_RIDGE;
    let mut weights = Vec::new();
    let mut regions = Vec::new();
    let mut stack = vec![DrawState { surface: [0.0; 4], rect: bounds, node: root, ridge_height, as_root: true, depth: 0 }];

    while let Some(mut state) = stack.pop() {
        items.push(VisibleItem { node: state.node, rect: state.rect, depth: state.depth, leaf: true, surface: [0.0; 4] });
        if state.rect.width() <= grid_width || state.rect.height() <= grid_width {
            continue;
        }
        if cushions && !state.as_root {
            add_ridge(state.rect, &mut state.surface, state.ridge_height);
        }

        let children = tree.children(state.node);
        if children.is_empty() {
            items.last_mut().expect("pushed above").surface = state.surface;
            canvas.draw_leaf(leaf_face(state.rect, options), &state.surface, color(tree, state.node));
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

/// The image being rendered and the leaf jobs still to rasterise into it.
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

/// Unit light direction for `options`.
fn light_of(options: &TreemapOptions) -> [f64; 3] {
    let (lx, ly, lz) = (options.light_x, options.light_y, 10.0);
    let len = (lx * lx + ly * ly + lz * lz).sqrt();
    [lx / len, ly / len, lz / len]
}

/// The part of a leaf's box that is painted: inset by the grid line if any.
fn leaf_face(mut rect: Rect, options: &TreemapOptions) -> Rect {
    if options.grid {
        rect.top += 1;
        rect.left += 1;
    }
    rect
}

/// A leaf's shading job. The colour supplies hue and chroma (scaled by `saturation`); its
/// lightness is rescaled so a colour at [`PALETTE_LIGHTNESS`] lands on `options.lightness`.
fn leaf_job(rect: Rect, surface: &[f64; 4], color: Oklch, options: &TreemapOptions) -> Job {
    let color = color.scale_chroma(options.saturation).with_lightness(options.lightness * color.l / PALETTE_LIGHTNESS);
    let glow = options.shading == Shading::Glow && options.cushion_shading();
    Job { rect, surface: *surface, color, glow }
}

impl<'a> Canvas<'a> {
    fn new(width: u32, height: u32, options: &'a TreemapOptions) -> Self {
        Self { width, height, pixels: vec![255; width as usize * height as usize * 4], options, light: light_of(options), jobs: Vec::new() }
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
            index: foldhash::HashMap::default(),
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
                shade_job(&jobs[i], light, 0, width, top, bottom, pixels);
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
        self.jobs.push(leaf_job(rect, surface, color, self.options));
    }
}

/// Shade one job's pixels within the window `left..left + width` by
/// `top..bottom`; `pixels` is that window's RGBA buffer. Written pixels are opaque.
fn shade_job(job: &Job, light: [f64; 3], left: i32, width: i32, top: i32, bottom: i32, pixels: &mut [u8]) {
    let rect = job.rect;
    let (y0, y1) = (rect.top.max(top), rect.bottom.min(bottom));
    let (x0, x1) = (rect.left.max(left), rect.right.min(left + width));
    if y0 >= y1 || x0 >= x1 {
        return;
    }
    let put = |pixels: &mut [u8], x: i32, y: i32, rgb: Rgb| {
        let i = ((y - top) as usize * width as usize + (x - left) as usize) * 4;
        pixels[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    };
    if !job.glow {
        let rgb = job.color.to_srgb();
        for y in y0..y1 {
            for x in x0..x1 {
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
        let mut nx = -(2.0 * surface[0] * (f64::from(x0) + 0.5) + surface[2]);
        for x in x0..x1 {
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

/// Add one cushion ridge of height `h` over `rect` to `surface` (WinDirStat's `AddRidge`).
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
        let options = dirstats_scan::ScanOptions { size_metric: dirstats_scan::SizeMetric::Apparent, ..Default::default() };
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
        let options = dirstats_scan::ScanOptions { size_metric: dirstats_scan::SizeMetric::Apparent, ..Default::default() };
        let tree = dirstats_scan::scan(dir.path(), &options).unwrap();
        let colors = ExtensionColors::rank(&tree);
        for style in [Style::Rows, Style::Squarified] {
            let map = render(&tree, tree.root(), 64, 48, &TreemapOptions { style, ..Default::default() }, |t, id| colors.color(t, id));
            assert_eq!(map.pixels.len(), 64 * 48 * 4);
            assert_eq!(map.items.len(), 3);
            let hit = map.hit_test(1, 1).unwrap();
            assert_ne!(hit, tree.root());
            assert_eq!(map.item_index(tree.root()), Some(0));
            assert_eq!(map.item(hit).map(|i| i.node), Some(hit));
            assert!(map.item(hit).unwrap().leaf && !map.items[0].leaf);
            assert_eq!(map.subtree(tree.root()).len(), 3, "root's run covers every item");
            assert_eq!(map.subtree(hit).len(), 1, "a leaf's run is itself");

            // An overlay re-shades only the given leaf, opaque there and clear elsewhere.
            let index = map.item_index(hit).unwrap();
            let leaf = map.items[index].rect;
            let bounds = layout::Rect::new(0, 0, 64, 48);
            let overlay = map.shade_leaves(&TreemapOptions { style, ..Default::default() }, bounds, [(index, Oklch::new(0.7, 0.2, 90.0))]);
            let alpha = |x: i32, y: i32| overlay[((y * 64 + x) * 4 + 3) as usize];
            assert_eq!(alpha(leaf.left, leaf.top), 255);
            assert_eq!(alpha(leaf.right - 1, leaf.bottom - 1), 255);
            let other = map.items.iter().find(|i| i.leaf && i.node != hit).unwrap().rect;
            assert_eq!(alpha(other.left, other.top), 0);
            let base = &map.pixels[((leaf.top * 64 + leaf.left) * 4) as usize..][..3];
            assert_ne!(&overlay[((leaf.top * 64 + leaf.left) * 4) as usize..][..3], base, "overlay uses the given colour");
        }
    }
}

#[cfg(test)]
mod mix_tests {
    use super::*;

    #[test]
    fn mix_aggregates_bytes_per_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("a.rs"), vec![0u8; 30_000]).unwrap();
        std::fs::write(dir.path().join("sub/b.rs"), vec![0u8; 20_000]).unwrap();
        std::fs::write(dir.path().join("sub/c.png"), vec![0u8; 10_000]).unwrap();
        let options = dirstats_scan::ScanOptions { size_metric: dirstats_scan::SizeMetric::Apparent, ..Default::default() };
        let tree = dirstats_scan::scan(dir.path(), &options).unwrap();
        let colors = ExtensionColors::rank(&tree);
        let mix = colors.mix(&tree, 8);
        let root = tree.root();
        let rs = colors.entries().iter().position(|(e, _, _)| e.as_deref() == Some("rs")).unwrap() as u32;
        let png = colors.entries().iter().position(|(e, _, _)| e.as_deref() == Some("png")).unwrap() as u32;
        assert_eq!(mix.segments(root), &[(rs, 50_000), (png, 10_000)]);
        let sub = tree.children(root).iter().copied().find(|&id| tree.node(id).name.as_ref() == "sub").unwrap();
        assert_eq!(mix.segments(sub), &[(rs, 20_000), (png, 10_000)]);
        let a = tree.children(root).iter().copied().find(|&id| tree.node(id).name.as_ref() == "a.rs").unwrap();
        assert_eq!(mix.segments(a), &[(rs, 30_000)]);
        assert_eq!(colors.mix(&tree, 1).segments(root), &[(rs, 50_000)]);
    }
}
