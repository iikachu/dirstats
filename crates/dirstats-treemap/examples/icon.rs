// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Write the app icon files a package needs into a directory:
//!
//! - `dirstats-<size>.png` at 16 to 1024 pixels (Linux hicolor sizes and more),
//! - `dirstats.ico` for Windows, with PNG images from 16 to 256,
//! - `dirstats.icns` for macOS, on Apple's icon grid, 16 to 1024 including @2x.
//!
//! ```text
//! cargo run -p dirstats-treemap --example icon -- OUT_DIR
//! ```

use dirstats_treemap::icon::{Shape, icon};
use std::path::PathBuf;

const PNG_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512, 1024];
const ICO_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256];
/// ICNS element types that hold a PNG, with their pixel size (Apple's
/// `iconutil` writes the same set).
const ICNS_TYPES: &[(&[u8; 4], u32)] = &[
    (b"icp4", 16),
    (b"icp5", 32),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic13", 256),
    (b"ic08", 256),
    (b"ic14", 512),
    (b"ic09", 512),
    (b"ic10", 1024),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args_os().nth(1).ok_or("usage: icon OUT_DIR")?);
    std::fs::create_dir_all(&out)?;

    for &size in PNG_SIZES {
        write(&out.join(format!("dirstats-{size}.png")), &png(size, Shape::Square)?)?;
    }

    // ICONDIR, one ICONDIRENTRY per image, then the PNGs; 0 means 256.
    let images = ICO_SIZES.iter().map(|&s| png(s, Shape::Square)).collect::<Result<Vec<_>, _>>()?;
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (&size, image) in ICO_SIZES.iter().zip(&images) {
        let side = if size >= 256 { 0 } else { size as u8 };
        ico.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    images.iter().for_each(|image| ico.extend_from_slice(image));
    write(&out.join("dirstats.ico"), &ico)?;

    // "icns", total length, then (type, length including this header, PNG) per element.
    let mut body = Vec::new();
    for &(kind, size) in ICNS_TYPES {
        let image = png(size, Shape::Macos)?;
        body.extend_from_slice(kind);
        body.extend_from_slice(&(image.len() as u32 + 8).to_be_bytes());
        body.extend_from_slice(&image);
    }
    let mut icns = b"icns".to_vec();
    icns.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    icns.extend_from_slice(&body);
    write(&out.join("dirstats.icns"), &icns)?;
    Ok(())
}

fn png(size: u32, shape: Shape) -> Result<Vec<u8>, png::EncodingError> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&icon(size, shape))?;
    Ok(bytes)
}

fn write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)?;
    println!("wrote {}", path.display());
    Ok(())
}
