// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Scan a directory and write a cushion treemap PNG.
//!
//! cargo run --release -p dirstats-treemap --example treemap -- <dir> [out.png] [rows|squarified]

use dirstats_scan::{ScanOptions, scan};
use dirstats_treemap::render::{color_by_extension, default_palette, render};
use dirstats_treemap::{Style, TreemapOptions};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| ".".into());
    let out = args.next().unwrap_or_else(|| "treemap.png".into());
    let style = match args.next().as_deref() {
        Some("squarified") => Style::Squarified,
        _ => Style::Rows,
    };

    let started = Instant::now();
    let tree = scan(&dir, &ScanOptions::default())?;
    let root = tree.root();
    println!(
        "{} entries, {} files, {:.1} MiB in {:.2?}",
        tree.len(),
        tree.node(root).file_count,
        tree.size(root) as f64 / 1_048_576.0,
        started.elapsed()
    );
    for &child in tree.children(root).iter().take(10) {
        println!("{:>12}  {}", tree.size(child), tree.path(child).display());
    }

    let (width, height) = (1600, 1000);
    let palette = default_palette();
    let options = TreemapOptions { style, ..Default::default() };
    let started = Instant::now();
    let map = render(&tree, root, width, height, &options, color_by_extension(&palette));
    println!("rendered {} rectangles in {:.2?}", map.items.len(), started.elapsed());

    let file = std::io::BufWriter::new(std::fs::File::create(&out)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&map.pixels)?;
    println!("wrote {out}");
    Ok(())
}
