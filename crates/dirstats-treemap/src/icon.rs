// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! The dirstats app icon: a cushion treemap of a small made-up tree, drawn
//! by [`render()`](crate::render()) with the same palette and shading as the app, set in
//! a dark rounded frame.
//!
//! The icon is computed, not stored, so every size is drawn for that size
//! and the GUI can make its window icon at start-up. The `icon` example
//! writes the PNG, ICO and ICNS files a package needs.

use crate::layout::Style;
use crate::render::{ExtensionColors, Oklch, Shading, TreemapOptions, render};
use dirstats_scan::{Kind, Node, NodeId, SizeMetric, Tree, TreeBuilder};

/// The outline the treemap is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    /// A rounded square that nearly fills the image, for Windows and Linux
    /// icons and the window icon.
    #[default]
    Square,
    /// Apple's app icon grid: an 824-unit rounded square centred in 1024,
    /// leaving the margin macOS expects around every app icon.
    Macos,
}

impl Shape {
    /// (margin, corner radius) as fractions of the image and of the body.
    fn metrics(self) -> (f64, f64) {
        match self {
            Shape::Square => (1.0 / 32.0, 0.2),
            Shape::Macos => (100.0 / 1024.0, 185.0 / 824.0),
        }
    }
}

/// Subpixels per pixel along each axis; edges and corners are anti-aliased
/// by averaging them.
const SUPERSAMPLE: u32 = 4;
/// Frame colour around the treemap: the app's dark panel.
const FRAME: Oklch = Oklch::grey(0.24);
/// Colour of the lines between tiles: lighter than the frame, so they part
/// the tiles without drawing a heavy outline.
const GRID: Oklch = Oklch::grey(0.42);
/// Subpixels per treemap pixel. The grid line is one treemap pixel, so at 2
/// of the 4 subpixels it comes out half a pixel wide.
const MAP_SCALE: u32 = 2;
/// Frame width as a fraction of the body.
const FRAME_WIDTH: f64 = 0.04;
/// Below this many pixels the tree is cut to its top levels, so the tiles
/// stay big enough to tell apart.
const SMALL: u32 = 48;

/// Render the icon at `size` × `size` as RGBA8, row-major from the top left,
/// with straight (not premultiplied) alpha; outside the shape is transparent.
#[must_use]
pub fn icon(size: u32, shape: Shape) -> Vec<u8> {
    let tree = tree(if size < SMALL { 1 } else { u32::MAX });
    let colors = ExtensionColors::rank(&tree);
    let big = size * SUPERSAMPLE;
    let (margin, radius) = shape.metrics();
    let body = RoundedSquare::new(f64::from(big), margin, radius);
    let frame = (body.side * FRAME_WIDTH).round();
    let inner = body.inset(frame);

    // The treemap is drawn coarser than the subpixel grid so its grid lines
    // come out a thin, even half pixel, then sampled per subpixel. Tiny icons
    // skip the grid, which would eat the tiles.
    let map_side = (inner.side / f64::from(MAP_SCALE)).ceil() as u32;
    let options = TreemapOptions {
        style: Style::Squarified,
        shading: Shading::Glow,
        grid: size >= SMALL,
        grid_color: GRID,
        ..TreemapOptions::default()
    };
    let map = render(&tree, tree.root(), map_side, map_side, &options, |t, id| colors.color(t, id));
    let map_origin = inner.min.round() as i64;
    let frame_rgb = FRAME.to_srgb();

    // Per output pixel: colour summed over the covered subpixels, and how many
    // were covered, which becomes the alpha.
    let mut sums = vec![[0u32; 4]; size as usize * size as usize];
    for y in 0..big {
        let row = (y / SUPERSAMPLE) as usize * size as usize;
        let fy = f64::from(y) + 0.5;
        for x in 0..big {
            let fx = f64::from(x) + 0.5;
            if !body.contains(fx, fy) {
                continue;
            }
            let rgb = if inner.contains(fx, fy) {
                let (mx, my) = ((i64::from(x) - map_origin) / i64::from(MAP_SCALE), (i64::from(y) - map_origin) / i64::from(MAP_SCALE));
                let (mx, my) = (mx.clamp(0, i64::from(map_side) - 1), my.clamp(0, i64::from(map_side) - 1));
                let i = (my as usize * map_side as usize + mx as usize) * 4;
                [map.pixels[i], map.pixels[i + 1], map.pixels[i + 2]]
            } else {
                frame_rgb
            };
            let sum = &mut sums[row + (x / SUPERSAMPLE) as usize];
            for c in 0..3 {
                sum[c] += u32::from(rgb[c]);
            }
            sum[3] += 1;
        }
    }

    let samples = SUPERSAMPLE * SUPERSAMPLE;
    let mut out = vec![0u8; sums.len() * 4];
    for (pixel, sum) in out.as_chunks_mut::<4>().0.iter_mut().zip(&sums) {
        let covered = sum[3];
        if covered == 0 {
            continue;
        }
        for c in 0..3 {
            pixel[c] = ((sum[c] + covered / 2) / covered) as u8;
        }
        pixel[3] = ((covered * 255 + samples / 2) / samples) as u8;
    }
    out
}

/// A square with rounded corners, in subpixels. Corners are a superellipse
/// rather than a circle, close to the continuous corners of platform icons.
#[derive(Clone, Copy, Debug)]
struct RoundedSquare {
    min: f64,
    side: f64,
    radius: f64,
}

impl RoundedSquare {
    fn new(image: f64, margin: f64, radius: f64) -> Self {
        let min = (image * margin).round();
        let side = image - 2.0 * min;
        Self { min, side, radius: side * radius }
    }

    /// The same shape `by` subpixels smaller on every side, corners kept concentric.
    fn inset(self, by: f64) -> Self {
        Self { min: self.min + by, side: self.side - 2.0 * by, radius: (self.radius - by).max(0.0) }
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        let max = self.min + self.side;
        if x < self.min || y < self.min || x >= max || y >= max {
            return false;
        }
        let r = self.radius;
        let dx = (self.min + r - x).max(x - (max - r)).max(0.0);
        let dy = (self.min + r - y).max(y - (max - r)).max(0.0);
        if r == 0.0 || dx == 0.0 || dy == 0.0 {
            return true;
        }
        (dx / r).powi(4) + (dy / r).powi(4) <= 1.0
    }
}

/// A made-up disk: one entry per line of [`DISK`] is a directory or file
/// with a size in arbitrary units.
enum Entry {
    Dir(&'static str, &'static [Entry]),
    File(&'static str, u64),
}

use Entry::{Dir, File};

/// Sizes are picked so the squarified layout comes out balanced: a few big
/// blocks for small icons, finer tiles inside them for large ones. The
/// extensions rank in the order the palette hands out its first hues.
const DISK: &[Entry] = &[
    Dir("movies", &[File("a.mov", 170), File("b.mov", 90), File("c.mov", 60), File("poster.jpg", 26), Dir("clips", &[File("d.mov", 30), File("e.mov", 22), File("f.mov", 14)])]),
    Dir("photos", &[
        File("a.jpg", 60), File("b.jpg", 44), File("c.jpg", 36), File("d.jpg", 28), File("e.mov", 20),
        Dir("2024", &[File("e.jpg", 22), File("f.jpg", 18), File("g.jpg", 14), File("h.jpg", 10)]),
    ]),
    Dir("code", &[
        Dir("target", &[File("a.rlib", 56), File("b.rlib", 34), File("c.rlib", 20), File("d.rlib", 12)]),
        File("e.rlib", 24), File("notes.pdf", 12),
        Dir("deps", &[File("f.rlib", 16), File("g.rlib", 10), File("h.rlib", 7)]),
    ]),
    Dir("music", &[File("a.flac", 52), File("b.flac", 38), File("c.flac", 26), File("d.flac", 18), File("e.flac", 11)]),
    Dir("docs", &[File("a.pdf", 34), File("b.pdf", 20), File("c.pdf", 13), File("d.pdf", 8), File("e.pdf", 5)]),
];

/// The icon's tree. Directories deeper than `depth` below the root become
/// one file of their largest extension, so a small icon draws fewer tiles.
#[must_use]
pub fn tree(depth: u32) -> Tree {
    let mut builder = TreeBuilder::new();
    let root = builder.push(node("disk", None, Kind::Directory, 0));
    add(&mut builder, root, DISK, depth);
    builder.finish(SizeMetric::Allocated)
}

fn add(builder: &mut TreeBuilder, parent: NodeId, entries: &[Entry], depth: u32) {
    for entry in entries {
        match entry {
            File(name, size) => {
                builder.push(node(name, Some(parent), Kind::File, *size));
            }
            Dir(name, children) if depth > 1 => {
                let dir = builder.push(node(name, Some(parent), Kind::Directory, 0));
                add(builder, dir, children, depth - 1);
            }
            Dir(_, children) => {
                let (name, _) = largest_file(children).expect("directories in DISK are not empty");
                builder.push(node(name, Some(parent), Kind::File, total(children)));
            }
        }
    }
}

fn total(entries: &[Entry]) -> u64 {
    entries.iter().map(|e| match e {
        File(_, size) => *size,
        Dir(_, children) => total(children),
    }).sum()
}

fn largest_file(entries: &[Entry]) -> Option<(&'static str, u64)> {
    entries.iter().filter_map(|e| match e {
        File(name, size) => Some((*name, *size)),
        Dir(_, children) => largest_file(children),
    }).max_by_key(|&(_, size)| size)
}

fn node(name: &str, parent: Option<NodeId>, kind: Kind, size: u64) -> Node {
    Node {
        name: std::ffi::OsStr::new(name).into(),
        parent,
        kind,
        apparent_size: size,
        allocated_size: size,
        file_count: u64::from(kind != Kind::Directory),
        dir_count: 0,
        modified: None,
        duplicate_link: false,
        error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_is_opaque_inside_and_clear_at_the_corners() {
        for shape in [Shape::Square, Shape::Macos] {
            for size in [16, 32, 256] {
                let rgba = icon(size, shape);
                assert_eq!(rgba.len(), (size * size * 4) as usize);
                let alpha = |x: u32, y: u32| rgba[((y * size + x) * 4 + 3) as usize];
                assert_eq!(alpha(0, 0), 0, "{shape:?} {size}: corner");
                assert_eq!(alpha(size - 1, size - 1), 0, "{shape:?} {size}: corner");
                assert_eq!(alpha(size / 2, size / 2), 255, "{shape:?} {size}: centre");
            }
        }
    }

    #[test]
    fn small_icons_collapse_directories() {
        let full = tree(u32::MAX);
        let small = tree(1);
        assert_eq!(full.size(full.root()), small.size(small.root()), "same total either way");
        assert!(small.children(small.root()).iter().all(|&c| small.node(c).kind == Kind::File));
        assert_eq!(small.children(small.root()).len(), DISK.len());
    }
}
