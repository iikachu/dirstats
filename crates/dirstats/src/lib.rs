// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! dirstats as a library: command-line options and the entry points each
//! front end is started from. The `dirstats` binary is a thin wrapper.

pub use dirstats_app as app;
pub use dirstats_scan as scan;
pub use dirstats_treemap as treemap;
#[cfg(feature = "tui")]
pub use dirstats_tui as tui;
#[cfg(feature = "gui")]
pub use dirstats_gui as gui;

use clap::{Parser, ValueEnum};
use dirstats_scan::{ScanOptions, SizeMetric};
use std::path::{Component, Path, PathBuf};

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

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum LayoutStyle {
    Rows,
    #[default]
    Squarified,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ShadingStyle {
    /// Softly lit cushions with a broad highlight.
    #[default]
    Glow,
    /// Plain fills.
    Flat,
}

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
    /// Use the terminal interface instead of opening a window.
    #[cfg(feature = "tui")]
    #[arg(long)]
    pub tui: bool,
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

/// Scan synchronously and print the largest entries under the root.
pub fn print_summary(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let started = std::time::Instant::now();
    let tree = dirstats_ntfs::scan(cli.scan_root()?, &cli.scan_options())?;
    let root = tree.root();
    println!(
        "{} entries, {} files, {} in {:.2?}",
        tree.len(),
        tree.node(root).file_count,
        dirstats_app::format::size(tree.size(root)),
        started.elapsed()
    );
    for &child in tree.children(root).iter().take(20) {
        println!("{:>10}  {}", dirstats_app::format::size(tree.size(child)), tree.path(child).display());
    }
    Ok(())
}

/// Scan synchronously and write a cushion treemap PNG.
#[cfg(feature = "png")]
pub fn write_png(cli: &Cli, out: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use dirstats_treemap::render::{ExtensionColors, render};
    use dirstats_treemap::{Shading, Style, TreemapOptions};

    let tree = dirstats_ntfs::scan(cli.scan_root()?, &cli.scan_options())?;
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

/// Start the graphical interface.
#[cfg(feature = "gui")]
pub fn run_gui(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = dirstats_app::App::new(cli.scan_options());
    if cli.path.is_some() {
        app.start_scan(cli.scan_root()?);
    }
    dirstats_gui::run(app).map_err(|e| e.to_string())?;
    Ok(())
}

/// Start the interactive terminal interface.
#[cfg(feature = "tui")]
pub fn run_tui(cli: &Cli) -> std::io::Result<()> {
    let mut app = dirstats_app::App::new(cli.scan_options());
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
