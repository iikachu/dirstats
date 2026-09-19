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
