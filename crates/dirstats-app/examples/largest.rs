// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Using dirstats-app as a library, without an `App`: scan a folder, list
//! its largest entries, and optionally move them to the trash.
//!
//! ```text
//! cargo run -p dirstats-app --features trash --example largest -- DIR [COUNT] [--trash]
//! ```
//!
//! Without `--trash` nothing is changed. With it, each listed entry goes
//! to the trash unless `check_removable` refuses it.

use dirstats_app::{ScanOptions, format, scanner, trash};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let move_them = args.iter().position(|a| a == "--trash").map(|i| args.remove(i)).is_some();
    let Some(dir) = args.first().map(PathBuf::from) else {
        return Err("usage: largest DIR [COUNT] [--trash]".into());
    };
    let count: usize = args.get(1).map(|n| n.parse()).transpose()?.unwrap_or(5);

    // Blocking scan; `App::start_scan` runs the same thing on a worker thread.
    let tree = scanner::scan(&dir, &ScanOptions::default())?;
    println!("{} in {}", format::size(tree.size(tree.root())), dir.display());
    // Children come sorted by size, largest first.
    for &id in tree.children(tree.root()).iter().take(count) {
        let path = tree.path(id);
        print!("{:>10}  {}", format::size(tree.size(id)), path.display());
        if move_them {
            match trash::move_to_trash(&path) {
                Ok(_) => print!("  -> moved to {}", trash::NAME),
                Err(err) => print!("  -> kept: {err}"),
            }
        }
        println!();
    }
    Ok(())
}
