// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Treemap layout and cushion rendering for [`dirstats_scan::Tree`].
//!
//! - [`layout`] arranges sibling sizes into rectangles.
//! - [`color`] does all shading in OKLCH; sRGB only at the pixel.
//! - [`render()`] draws a shaded treemap into an RGBA buffer.
//! - [`icon`] draws the dirstats app icon as a treemap.
//!
//! See `CREDITS.md` in the repository for the projects this crate builds on.

// docs.rs only allows https: images (its CSP is `img-src 'self' https:`), so
// the logo is a link to the generated file, not a data URL.
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/iikachu/dirstats/HEAD/assets/dirstats-logo.svg",
    html_favicon_url = "https://raw.githubusercontent.com/iikachu/dirstats/HEAD/assets/dirstats-logo.svg"
)]

pub mod color;
pub mod icon;
pub mod layout;
pub mod render;

pub use layout::{Rect, Style};
pub use color::{Oklch, Rgb};
pub use render::{ExtensionColors, Shading, Treemap, TreemapOptions, render};
