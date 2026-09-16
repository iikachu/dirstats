// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors

//! Parallel, cross-platform disk usage scanning.
//!
//! [`scan`] walks a directory tree using native bulk directory APIs and
//! returns a compact [`Tree`] whose children are sorted by size.
//!
//! See `CREDITS.md` in the repository for the projects this crate builds on.

pub mod scan;
pub mod tree;

pub use scan::{Progress, ScanOptions, scan, scan_with};
pub use tree::{Kind, Node, NodeId, SizeMetric, Tree};
