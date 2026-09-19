// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Draw the window icon at build time, so the icon code never ships in the
//! app: `src/lib.rs` embeds the raw pixels written here.

// Shared with the `icon` example, which uses the rest of it.
#[path = "build/icon.rs"]
#[allow(dead_code)]
mod icon;

use icon::{Shape, icon};

/// Side of the embedded icon in pixels; `src/lib.rs` must agree.
const SIZE: u32 = 256;

fn main() {
    // The target, not the host this script runs on: macOS gets the margin
    // its Dock icons have.
    let shape = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => Shape::Macos,
        _ => Shape::Square,
    };
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(out.join("icon.rgba"), icon(SIZE, shape)).expect("OUT_DIR is writable");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build/icon.rs");
}
