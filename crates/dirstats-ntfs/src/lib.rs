// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// Port of WinDirStat's `FinderNtfs.cpp` (GPL-2.0-or-later, by WinDirStat
// Team, https://windirstat.net). See `CREDITS.md` in the repository.

//! NTFS fast path: scan a whole volume by reading its master file table
//! instead of walking its directories.
//!
//! [`scan_with`] stands in for [`dirstats_scan::scan_with`]. It reads the
//! table when, on Windows, the root is a drive root such as `C:\` on an
//! NTFS volume, [`ScanOptions::same_filesystem`] is set (the table cannot
//! follow mount points) and the process may open the volume (which takes
//! administrator rights), and walks otherwise.

// The treemap icon as an inline SVG; regenerate with
// `cargo run -p dirstats-treemap --example icon -- OUT_DIR` (dirstats-logo.url).
#![doc(
    html_logo_url = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Cdefs%3E%3CradialGradient id='c' cx='.3' cy='.3' r='.9'%3E%3Cstop offset='0' stop-color='%23fff' stop-opacity='.3'/%3E%3Cstop offset='.65' stop-color='%23fff' stop-opacity='0'/%3E%3Cstop offset='1' stop-color='%23000' stop-opacity='.12'/%3E%3C/radialGradient%3E%3CclipPath id='k'%3E%3Crect x='6.8' y='6.8' width='86.4' height='86.4' rx='10.8'/%3E%3C/clipPath%3E%3C/defs%3E%3Crect x='3' y='3' width='94' height='94' rx='13.5' fill='%231f1f1f'/%3E%3Cg clip-path='url(%23k)'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3' fill='%2300d6ce'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7' fill='%2300d6ce'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7' fill='%2300d6ce'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1' fill='%23ffa062'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8' fill='%23ffa062'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2' fill='%23ffa062'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8' fill='%23ffa062'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3' fill='%23ffa062'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4' fill='%2300d6ce'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2' fill='%23a6b5ff'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3' fill='%23ff95b7'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3' fill='%2379d652'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5' fill='%2379d652'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5' fill='%2379d652'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9' fill='%2379d652'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9' fill='%2379d652'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4' fill='%2379d652'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5' fill='%2379d652'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9' fill='%2379d652'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8' fill='%2379d652'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8' fill='%23ff95b7'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7' fill='%23ff95b7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1' fill='%23ff95b7'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8' fill='%23ff95b7'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8' fill='%23ff95b7'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8' fill='%23ff95b7'/%3E%3Cg fill='url(%23c)' stroke='%234d4d4d' stroke-width='.35' stroke-opacity='.6'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8'/%3E%3C/g%3E%3C/g%3E%3C/svg%3E",
    html_favicon_url = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Cdefs%3E%3CradialGradient id='c' cx='.3' cy='.3' r='.9'%3E%3Cstop offset='0' stop-color='%23fff' stop-opacity='.3'/%3E%3Cstop offset='.65' stop-color='%23fff' stop-opacity='0'/%3E%3Cstop offset='1' stop-color='%23000' stop-opacity='.12'/%3E%3C/radialGradient%3E%3CclipPath id='k'%3E%3Crect x='6.8' y='6.8' width='86.4' height='86.4' rx='10.8'/%3E%3C/clipPath%3E%3C/defs%3E%3Crect x='3' y='3' width='94' height='94' rx='13.5' fill='%231f1f1f'/%3E%3Cg clip-path='url(%23k)'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4' fill='%2300d6ce'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3' fill='%2300d6ce'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7' fill='%2300d6ce'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7' fill='%2300d6ce'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1' fill='%23ffa062'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8' fill='%23ffa062'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2' fill='%23ffa062'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8' fill='%23ffa062'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3' fill='%23ffa062'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4' fill='%2300d6ce'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6' fill='%23a6b5ff'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4' fill='%23a6b5ff'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3' fill='%23a6b5ff'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9' fill='%23a6b5ff'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2' fill='%23a6b5ff'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3' fill='%23ff95b7'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3' fill='%2379d652'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5' fill='%2379d652'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5' fill='%2379d652'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9' fill='%2379d652'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9' fill='%2379d652'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4' fill='%2379d652'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5' fill='%2379d652'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9' fill='%2379d652'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8' fill='%2379d652'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8' fill='%23ff95b7'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7' fill='%23ff95b7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1' fill='%23ff95b7'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8' fill='%23ff95b7'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8' fill='%23ff95b7'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8' fill='%23ff95b7'/%3E%3Cg fill='url(%23c)' stroke='%234d4d4d' stroke-width='.35' stroke-opacity='.6'%3E%3Crect x='89' y='84.8' width='4.2' height='8.4'/%3E%3Crect x='82.3' y='84.8' width='6.7' height='8.4'/%3E%3Crect x='82.3' y='76.5' width='10.9' height='8.3'/%3E%3Crect x='74' y='76.5' width='8.3' height='16.7'/%3E%3Crect x='59.9' y='76.5' width='14.1' height='16.7'/%3E%3Crect x='80.5' y='70.4' width='12.7' height='6.1'/%3E%3Crect x='80.5' y='60.6' width='12.7' height='9.8'/%3E%3Crect x='80.5' y='46.4' width='12.7' height='14.2'/%3E%3Crect x='59.9' y='63.7' width='20.6' height='12.8'/%3E%3Crect x='59.9' y='46.4' width='20.6' height='17.3'/%3E%3Crect x='87.4' y='32' width='5.8' height='14.4'/%3E%3Crect x='75.8' y='32' width='11.6' height='14.4'/%3E%3Crect x='67.6' y='40.4' width='8.2' height='6'/%3E%3Crect x='67.6' y='32' width='8.2' height='8.4'/%3E%3Crect x='59.9' y='32' width='7.7' height='14.4'/%3E%3Crect x='86.4' y='19.7' width='6.8' height='12.3'/%3E%3Crect x='75.1' y='19.7' width='11.3' height='12.3'/%3E%3Crect x='75.1' y='6.8' width='18.1' height='12.9'/%3E%3Crect x='59.9' y='6.8' width='15.2' height='25.2'/%3E%3Crect x='48.6' y='80.9' width='11.3' height='12.3'/%3E%3Crect x='32.9' y='80.9' width='15.7' height='12.3'/%3E%3Crect x='47.7' y='60.4' width='12.2' height='20.5'/%3E%3Crect x='32.9' y='60.4' width='14.8' height='20.5'/%3E%3Crect x='6.8' y='77.3' width='26.1' height='15.9'/%3E%3Crect x='28.8' y='60.4' width='4.1' height='16.9'/%3E%3Crect x='15.7' y='69.9' width='13.1' height='7.4'/%3E%3Crect x='15.7' y='60.4' width='13.1' height='9.5'/%3E%3Crect x='6.8' y='60.4' width='8.9' height='16.9'/%3E%3Crect x='50.8' y='40.6' width='9.1' height='19.8'/%3E%3Crect x='29.8' y='40.6' width='21' height='19.8'/%3E%3Crect x='17.2' y='52.7' width='12.6' height='7.7'/%3E%3Crect x='17.2' y='40.6' width='12.6' height='12.1'/%3E%3Crect x='6.8' y='40.6' width='10.4' height='19.8'/%3E%3Crect x='41.5' y='6.8' width='18.4' height='33.8'/%3E%3Crect x='6.8' y='6.8' width='34.7' height='33.8'/%3E%3C/g%3E%3C/g%3E%3C/svg%3E"
)]

pub mod mft;
#[cfg(windows)]
mod volume;

use dirstats_scan::{Progress, ScanOptions, Tree};
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Scan `root` to completion.
pub fn scan(root: impl AsRef<Path>, options: &ScanOptions) -> io::Result<Tree> {
    scan_with(root, options, &AtomicBool::new(false), &Progress::default())
}

/// Scan `root`, reporting into `progress` and stopping with
/// [`io::ErrorKind::Interrupted`] once `cancel` is set.
///
/// When the table cannot be read for a reason other than cancellation,
/// `progress.entries` is reset to zero and the walk runs instead. A tree
/// read from the table never sets [`Node::error`](dirstats_scan::Node::error)
/// and counts nothing in `progress.errors`.
pub fn scan_with(
    root: impl AsRef<Path>,
    options: &ScanOptions,
    cancel: &AtomicBool,
    progress: &Progress,
) -> io::Result<Tree> {
    #[cfg(windows)]
    {
        use std::sync::atomic::Ordering;
        // The table lists one volume only, so it cannot follow mount points.
        if options.same_filesystem
            && let Some(device) = volume::device_path(root.as_ref())
        {
            match volume::scan(root.as_ref(), &device, options, cancel, progress) {
                Err(err) if err.kind() != io::ErrorKind::Interrupted => {
                    // Not NTFS, not elevated, or unreadable: walk instead.
                    progress.entries.store(0, Ordering::Relaxed);
                }
                result => return result,
            }
        }
    }
    dirstats_scan::scan_with(root, options, cancel, progress)
}

/// Scan the volume at `root` (a drive root such as `C:\`) from its master
/// file table, failing instead of falling back to a walk. For benchmarks.
#[cfg(windows)]
#[doc(hidden)]
pub fn scan_mft(root: impl AsRef<Path>, options: &ScanOptions) -> io::Result<Tree> {
    let root = root.as_ref();
    let device = volume::device_path(root)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a drive root"))?;
    volume::scan(root, &device, options, &AtomicBool::new(false), &Progress::default())
}
