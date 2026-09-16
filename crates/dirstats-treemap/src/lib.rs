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
//! - [`render`] draws a cushion-shaded treemap into an RGBA buffer.
//!
//! See `CREDITS.md` in the repository for the projects this crate builds on.

pub mod layout;
pub mod render;

pub use layout::{Rect, Style};
pub use render::{Treemap, TreemapOptions, render};
