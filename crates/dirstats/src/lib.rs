// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! dirstats as a library: command-line options and the entry points each
//! front end is started from. The `dirstats` binary is a thin wrapper.

// The treemap icon as an inline SVG; regenerate with
// `cargo run -p dirstats-treemap --example icon -- OUT_DIR` (dirstats-logo.url).
#![doc(
    html_logo_url = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Cdefs%3E%3CradialGradient id='c' cx='.3' cy='.3' r='.9'%3E%3Cstop offset='0' stop-color='%23fff' stop-opacity='.3'/%3E%3Cstop offset='.65' stop-color='%23fff' stop-opacity='0'/%3E%3Cstop offset='1' stop-color='%23000' stop-opacity='.12'/%3E%3C/radialGradient%3E%3CclipPath id='k'%3E%3Crect x='6.8' y='6.8' width='86.4' height='86.4' rx='10.8'/%3E%3C/clipPath%3E%3C/defs%3E%3Crect x='3' y='3' width='94' height='94' rx='13.5' fill='%231f1f1f'/%3E%3Cg clip-path='url(%23k)'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3' fill='%2300d6ce'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7' fill='%2300d6ce'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7' fill='%2300d6ce'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1' fill='%23ffa062'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8' fill='%23ffa062'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2' fill='%23ffa062'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8' fill='%23ffa062'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3' fill='%23ffa062'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4' fill='%2300d6ce'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2' fill='%23a6b5ff'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3' fill='%23ff95b7'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3' fill='%2379d652'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5' fill='%2379d652'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5' fill='%2379d652'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9' fill='%2379d652'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9' fill='%2379d652'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4' fill='%2379d652'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5' fill='%2379d652'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9' fill='%2379d652'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8' fill='%2379d652'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8' fill='%23ff95b7'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7' fill='%23ff95b7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1' fill='%23ff95b7'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8' fill='%23ff95b7'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8' fill='%23ff95b7'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8' fill='%23ff95b7'/%3E%3Cg fill='url(%23c)' stroke='%234d4d4d' stroke-width='.35' stroke-opacity='.6'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8'/%3E%3C/g%3E%3C/g%3E%3C/svg%3E",
    html_favicon_url = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Cdefs%3E%3CradialGradient id='c' cx='.3' cy='.3' r='.9'%3E%3Cstop offset='0' stop-color='%23fff' stop-opacity='.3'/%3E%3Cstop offset='.65' stop-color='%23fff' stop-opacity='0'/%3E%3Cstop offset='1' stop-color='%23000' stop-opacity='.12'/%3E%3C/radialGradient%3E%3CclipPath id='k'%3E%3Crect x='6.8' y='6.8' width='86.4' height='86.4' rx='10.8'/%3E%3C/clipPath%3E%3C/defs%3E%3Crect x='3' y='3' width='94' height='94' rx='13.5' fill='%231f1f1f'/%3E%3Cg clip-path='url(%23k)'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3' fill='%2300d6ce'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7' fill='%2300d6ce'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7' fill='%2300d6ce'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1' fill='%23ffa062'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8' fill='%23ffa062'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2' fill='%23ffa062'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8' fill='%23ffa062'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3' fill='%23ffa062'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4' fill='%2300d6ce'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2' fill='%23a6b5ff'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3' fill='%23ff95b7'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3' fill='%2379d652'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5' fill='%2379d652'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5' fill='%2379d652'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9' fill='%2379d652'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9' fill='%2379d652'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4' fill='%2379d652'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5' fill='%2379d652'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9' fill='%2379d652'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8' fill='%2379d652'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8' fill='%23ff95b7'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7' fill='%23ff95b7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1' fill='%23ff95b7'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8' fill='%23ff95b7'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8' fill='%23ff95b7'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8' fill='%23ff95b7'/%3E%3Cg fill='url(%23c)' stroke='%234d4d4d' stroke-width='.35' stroke-opacity='.6'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8'/%3E%3C/g%3E%3C/g%3E%3C/svg%3E"
)]

pub use dirstats_core as core;
pub use dirstats_core::{scan, treemap};
#[cfg(feature = "tui")]
pub use dirstats_tui as tui;
#[cfg(feature = "gui")]
pub use dirstats_gui as gui;

pub mod session;

use clap::{Parser, ValueEnum};
use dirstats_core::{ScanOptions, SizeMetric};
use std::path::{Component, Path, PathBuf};

/// `--metric`: which [`SizeMetric`] the scan sorts and weighs by.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Metric {
    /// Bytes allocated on disk.
    #[default]
    Allocated,
    /// Logical file length.
    Apparent,
}

impl From<Metric> for SizeMetric {
    fn from(metric: Metric) -> Self {
        match metric {
            Metric::Allocated => SizeMetric::Allocated,
            Metric::Apparent => SizeMetric::Apparent,
        }
    }
}

/// `--layout`: the treemap layout for `--png`.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum LayoutStyle {
    /// KDirStat-style rows.
    Rows,
    /// Tiles kept close to square.
    #[default]
    Squarified,
}

/// `--shading`: the treemap shading for `--png`.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ShadingStyle {
    /// Softly lit cushions with a broad highlight.
    #[default]
    Glow,
    /// Plain fills.
    Flat,
}

/// Command-line options.
#[derive(Debug, Parser)]
#[command(name = "dirstats", version, about)]
pub struct Cli {
    /// Directory to scan. Defaults to the current directory; the graphical
    /// interface instead offers the home folder, disks and root to pick from.
    pub path: Option<PathBuf>,
    /// Which size to sort and draw by.
    #[arg(short, long, value_enum, default_value_t)]
    pub metric: Metric,
    /// Worker threads for directory reads (default: all cores).
    #[arg(short = 'j', long)]
    pub threads: Option<usize>,
    /// Descend into other mounted filesystems.
    #[arg(short = 'x', long)]
    pub cross_filesystems: bool,
    /// Count every hard link's data again instead of once.
    #[arg(long)]
    pub count_hard_links: bool,
    /// Print the largest entries instead of opening an interface.
    #[arg(long)]
    pub summary: bool,
    /// Use the terminal interface instead of opening a window. Chosen on
    /// its own when there is no graphical session (a Linux console, SSH).
    #[cfg(feature = "tui")]
    #[arg(long, conflicts_with = "gui")]
    pub tui: bool,
    /// Open a window even when there seems to be no graphical session.
    #[cfg(all(feature = "gui", feature = "tui"))]
    #[arg(long)]
    pub gui: bool,
    /// Write a cushion treemap PNG to this file and exit.
    #[cfg(feature = "png")]
    #[arg(long, value_name = "FILE")]
    pub png: Option<PathBuf>,
    /// Treemap layout used for --png.
    #[cfg(feature = "png")]
    #[arg(long, value_enum, default_value_t)]
    pub layout: LayoutStyle,
    /// Shading used for --png.
    #[cfg(feature = "png")]
    #[arg(long, value_enum, default_value_t)]
    pub shading: ShadingStyle,
}

impl Cli {
    /// Scan options from the flags; `--threads 0` counts as 1.
    #[must_use]
    pub fn scan_options(&self) -> ScanOptions {
        let mut options = ScanOptions {
            same_filesystem: !self.cross_filesystems,
            count_hard_links_once: !self.count_hard_links,
            size_metric: self.metric.into(),
            ..ScanOptions::default()
        };
        if let Some(threads) = self.threads {
            options.threads = threads.max(1);
        }
        options
    }

    /// The directory to scan as an absolute, normalized path; the current
    /// directory when none was given.
    ///
    /// The scan root's name becomes the tree's root name and is shown in
    /// titles, breadcrumbs and messages, so a relative argument such as
    /// `.`, `../x` or `x/` is resolved against the working directory and
    /// cleaned of `.`, `..` and trailing separators first. Symlinks are
    /// left alone so the user sees the path they typed, not its target.
    pub fn scan_root(&self) -> std::io::Result<PathBuf> {
        Ok(normalize(&std::path::absolute(self.path.as_deref().unwrap_or(Path::new(".")))?))
    }
}

/// Remove `.` and resolve `..` components lexically in an absolute path.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never pop past the root (or drive prefix on Windows).
                if !matches!(out.components().next_back(), None | Some(Component::RootDir | Component::Prefix(_))) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Scan synchronously and print a totals line and the 20 largest entries
/// under the root.
pub fn print_summary(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let started = std::time::Instant::now();
    let tree = dirstats_core::scanner::scan(cli.scan_root()?, &cli.scan_options())?;
    let root = tree.root();
    println!(
        "{} entries, {} files, {} in {:.2?}",
        tree.len(),
        tree.node(root).file_count,
        dirstats_core::format::size(tree.size(root)),
        started.elapsed()
    );
    for &child in tree.children(root).iter().take(20) {
        println!("{:>10}  {}", dirstats_core::format::size(tree.size(child)), tree.path(child).display());
    }
    Ok(())
}

/// Scan synchronously and write a 1600 × 1000 cushion treemap PNG to `out`.
#[cfg(feature = "png")]
pub fn write_png(cli: &Cli, out: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use dirstats_core::treemap::render::{ExtensionColors, render};
    use dirstats_core::treemap::{Shading, Style, TreemapOptions};

    let tree = dirstats_core::scanner::scan(cli.scan_root()?, &cli.scan_options())?;
    let style = match cli.layout {
        LayoutStyle::Rows => Style::Rows,
        LayoutStyle::Squarified => Style::Squarified,
    };
    let (width, height) = (1600, 1000);
    let colors = ExtensionColors::rank(&tree);
    let shading = match cli.shading {
        ShadingStyle::Glow => Shading::Glow,
        ShadingStyle::Flat => Shading::Flat,
    };
    let options = TreemapOptions { style, shading, ..Default::default() };
    let map = render(&tree, tree.root(), width, height, &options, |t, id| colors.color(t, id));

    let file = std::io::BufWriter::new(std::fs::File::create(out)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&map.pixels)?;
    println!("wrote {}", out.display());
    Ok(())
}

/// Start the graphical interface and return when its window closes. A scan
/// starts at once only when a path was given; otherwise the window offers
/// places to pick from.
#[cfg(feature = "gui")]
pub fn run_gui(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = dirstats_core::App::new(cli.scan_options());
    if cli.path.is_some() {
        app.start_scan(cli.scan_root()?);
    }
    dirstats_gui::run(app).map_err(|e| e.to_string())?;
    Ok(())
}

/// Start the interactive terminal interface, scanning the path given or
/// the current directory, and return when it quits.
#[cfg(feature = "tui")]
pub fn run_tui(cli: &Cli) -> std::io::Result<()> {
    let mut app = dirstats_core::App::new(cli.scan_options());
    app.start_scan(cli.scan_root()?);
    dirstats_tui::run(&mut app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_cleans_dot_and_dotdot() {
        let cwd = std::env::current_dir().unwrap();
        let cli = |p: &str| Cli::parse_from(["dirstats", p]);
        assert_eq!(cli(".").scan_root().unwrap(), cwd);
        assert_eq!(cli("./sub/").scan_root().unwrap(), cwd.join("sub"));
        assert_eq!(cli("..").scan_root().unwrap(), cwd.parent().unwrap());
        assert_eq!(cli("a/../b").scan_root().unwrap(), cwd.join("b"));
        assert_eq!(normalize(Path::new("/../x")), PathBuf::from("/x"));
        assert!(cli("/tmp").scan_root().unwrap().is_absolute());
        assert_eq!(Cli::parse_from(["dirstats"]).scan_root().unwrap(), cwd, "no path means the current directory");
    }
}
