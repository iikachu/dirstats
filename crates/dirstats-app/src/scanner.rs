// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Background scan handle.

#[cfg(feature = "ntfs-mft")]
use dirstats_ntfs::scan_with;
#[cfg(not(feature = "ntfs-mft"))]
use dirstats_scan::scan_with;
use dirstats_scan::{Progress, ScanOptions, Tree};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::Arc;
use std::time::Instant;

/// Scan `root` on the calling thread, through the same fast paths as
/// [`RunningScan`].
pub fn scan(root: impl AsRef<std::path::Path>, options: &ScanOptions) -> io::Result<Tree> {
    scan_with(root, options, &AtomicBool::new(false), &Progress::default())
}

pub enum ScanStatus {
    Running,
    Done(io::Result<Tree>),
}

/// A scan running on its own thread.
#[derive(Debug)]
pub struct RunningScan {
    pub root: PathBuf,
    pub started: Instant,
    pub progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<io::Result<Tree>>,
}

impl RunningScan {
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

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn entries(&self) -> u64 {
        self.progress.entries.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn errors(&self) -> u64 {
        self.progress.errors.load(Ordering::Relaxed)
    }

    /// Non-blocking check for completion.
    pub fn try_finish(&self) -> ScanStatus {
        match self.receiver.try_recv() {
            Ok(result) => ScanStatus::Done(result),
            Err(TryRecvError::Empty) => ScanStatus::Running,
            Err(TryRecvError::Disconnected) => {
                ScanStatus::Done(Err(io::Error::other("scan thread exited without a result")))
            }
        }
    }
}

impl Drop for RunningScan {
    fn drop(&mut self) {
        self.cancel();
    }
}
