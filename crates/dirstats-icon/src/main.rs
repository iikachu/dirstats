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
//! - `dirstats.icns` for macOS, on Apple's icon grid, 16 to 1024 including @2x,
//! - `dirstats-hero.svg`, the README banner: the macOS icon on a gradient
//!   with the name and tagline. It is checked in as `assets/dirstats-hero.svg`,
//! - `dirstats-logo.svg`, the square icon as a small vector SVG: the logo
//!   and favicon of the API docs, checked in as `assets/dirstats-logo.svg`.
//!
//! ```text
//! cargo run -p dirstats-icon --features cli -- OUT_DIR
//! ```
//!
//! After changing the icon, copy `dirstats-hero.svg` and `dirstats-logo.svg`
//! over the ones in `assets/`; the GUI picks up the change on its next build.

use dirstats_icon::{Shape, icon};
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
    let out = PathBuf::from(std::env::args_os().nth(1).ok_or("usage: dirstats-icon OUT_DIR")?);
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

    write(&out.join("dirstats-hero.svg"), hero(&png(HERO_ICON, Shape::Macos)?).as_bytes())?;
    write(&out.join("dirstats-logo.svg"), dirstats_icon::svg().as_bytes())?;
    Ok(())
}

/// Pixel size of the icon embedded in the banner: shown at 280 points, so
/// sharp on a 2× screen without making the SVG large.
const HERO_ICON: u32 = 512;

/// The README banner. It carries its own background, so it reads the same
/// on GitHub's light and dark themes.
fn hero(icon_png: &[u8]) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 440" width="1200" height="440" role="img" aria-label="dirstats: see what is filling your disk">
  <defs>
    <radialGradient id="warm" cx="20%" cy="10%" r="65%"><stop offset="0" stop-color="#ffb38a"/><stop offset="1" stop-color="#ffb38a" stop-opacity="0"/></radialGradient>
    <radialGradient id="cool" cx="90%" cy="30%" r="55%"><stop offset="0" stop-color="#8a9bff"/><stop offset="1" stop-color="#8a9bff" stop-opacity="0"/></radialGradient>
    <radialGradient id="base" cx="50%" cy="100%" r="90%"><stop offset="0" stop-color="#5ad1c0"/><stop offset="0.75" stop-color="#2b3a67"/></radialGradient>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="18" stdDeviation="16" flood-color="#000" flood-opacity="0.35"/></filter>
    <filter id="glow"><feDropShadow dx="0" dy="1" stdDeviation="4" flood-color="#000" flood-opacity="0.3"/></filter>
    <clipPath id="card"><rect width="1200" height="440" rx="24"/></clipPath>
  </defs>
  <g clip-path="url(#card)">
    <rect width="1200" height="440" fill="url(#base)"/>
    <rect width="1200" height="440" fill="url(#warm)"/>
    <rect width="1200" height="440" fill="url(#cool)"/>
  </g>
  <image x="250" y="70" width="300" height="300" filter="url(#shadow)" href="data:image/png;base64,{icon}"/>
  <g fill="#fff" filter="url(#glow)" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Cantarell, 'Helvetica Neue', Arial, sans-serif">
    <text x="600" y="210" font-size="76" font-weight="700">dirstats</text>
    <text x="604" y="262" font-size="28" opacity="0.9">See what is filling your disk.</text>
  </g>
</svg>
"##,
        icon = base64(icon_png)
    )
}

/// Standard base64 with padding, for the PNG embedded in the banner.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
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
