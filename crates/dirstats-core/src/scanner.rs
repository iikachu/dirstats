// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Background scan handle.

#[cfg(all(windows, feature = "ntfs-mft"))]
use dirstats_ntfs::scan_with;
#[cfg(not(all(windows, feature = "ntfs-mft")))]
use dirstats_scan::scan_with;
use dirstats_scan::{Progress, ScanOptions, Tree};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Instant;

/// Scan `root` on the calling thread, through the same fast paths as
/// [`RunningScan`]. It cannot be cancelled and reports no progress.
pub fn scan(root: impl AsRef<std::path::Path>, options: &ScanOptions) -> io::Result<Tree> {
    scan_with(root, options, &AtomicBool::new(false), &Progress::default())
}

/// Answer from [`RunningScan::try_finish`].
pub enum ScanStatus {
    /// No result yet.
    Running,
    /// The scan's result; an error too if the scan thread died without one.
    Done(io::Result<Tree>),
}

/// A scan running on its own thread. Dropping the handle cancels it.
#[derive(Debug)]
pub struct RunningScan {
    /// Directory being scanned.
    pub root: PathBuf,
    /// When [`RunningScan::spawn`] was called.
    pub started: Instant,
    /// Counters the scan thread updates as it goes.
    pub progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<io::Result<Tree>>,
}

impl RunningScan {
    /// Start scanning `root` on a new `dirstats-scan` thread. Panics if the
    /// thread cannot be spawned.
    pub fn spawn(root: PathBuf, options: ScanOptions) -> Self {
        let progress = Arc::new(Progress::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = channel();
        {
            let (root, progress, cancel) = (root.clone(), Arc::clone(&progress), Arc::clone(&cancel));
            std::thread::Builder::new()
                .name("dirstats-scan".into())
                .spawn(move || {
                    let result = scan_with(&root, &options, &cancel, &progress);
                    // The receiver may be gone if the app dropped the scan.
                    let _ = sender.send(result);
                })
                .expect("spawn scan thread");
        }
        Self { root, started: Instant::now(), progress, cancel, receiver }
    }

    /// Ask the scan to stop; it then finishes early with an error.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Entries added to the tree so far.
    #[must_use]
    pub fn entries(&self) -> u64 {
        self.progress.entries.load(Ordering::Relaxed)
    }

    /// Directories that could not be listed plus entries whose metadata
    /// could not be read, so far.
    #[must_use]
    pub fn errors(&self) -> u64 {
        self.progress.errors.load(Ordering::Relaxed)
    }

    /// Non-blocking check for completion.
    pub fn try_finish(&self) -> ScanStatus {
        match self.receiver.try_recv() {
            Ok(result) => ScanStatus::Done(result),
            Err(TryRecvError::Empty) => ScanStatus::Running,
            Err(TryRecvError::Disconnected) => ScanStatus::Done(Err(io::Error::other("scan thread exited without a result"))),
        }
    }
}

impl Drop for RunningScan {
    fn drop(&mut self) {
        self.cancel();
    }
}
