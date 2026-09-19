// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Permanent deletion on Windows and Linux, bypassing the trash.
//!
//! The tree is not re-enumerated: the scan already knows every path, so
//! the worker walks the scanned nodes, removes files first and then
//! directories bottom-up, the way WinDirStat does. Symlinks and junctions
//! are removed as links; their targets are never entered. Nothing here is
//! reversible, so front ends gate it behind [`crate::App::permanent_delete`]
//! and confirm each deletion themselves.
//!
//! Front ends start one with `App::delete_node_permanently` and adopt the
//! result with `App::poll_delete` each tick; both exist on Windows and
//! Linux only.

use dirstats_scan::{Kind, NodeId, Tree};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::Arc;
use std::time::Instant;

/// One path the worker could not remove, with the reason.
#[derive(Debug)]
pub struct DeleteFailure {
    pub path: PathBuf,
    pub error: io::Error,
}

/// What a finished deletion did.
#[derive(Debug, Default)]
pub struct DeleteOutcome {
    /// Files and directories removed.
    pub removed: u64,
    /// Paths that could not be removed, in the order they were tried.
    pub failures: Vec<DeleteFailure>,
    /// Stopped early because [`RunningDelete::cancel`] was called.
    pub cancelled: bool,
}

pub enum DeleteStatus {
    Running,
    Done(DeleteOutcome),
}

/// A permanent deletion running on its own thread.
#[derive(Debug)]
pub struct RunningDelete {
    /// Node the deletion was asked for.
    pub node: NodeId,
    pub path: PathBuf,
    pub started: Instant,
    /// Entries to remove, for progress.
    pub total: u64,
    done: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<DeleteOutcome>,
}

impl RunningDelete {
    /// Start removing `node` and everything under it.
    pub fn spawn(tree: &Tree, node: NodeId) -> Self {
        let (files, dirs) = collect(tree, node);
        let total = (files.len() + dirs.len()) as u64;
        let done = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = channel();
        {
            let (done, cancel) = (Arc::clone(&done), Arc::clone(&cancel));
            std::thread::Builder::new()
                .name("dirstats-delete".into())
                .spawn(move || {
                    let outcome = run(files, dirs, &done, &cancel);
                    // The receiver may be gone if the app dropped the handle.
                    let _ = sender.send(outcome);
                })
                .expect("spawn delete thread");
        }
        Self { node, path: tree.path(node), started: Instant::now(), total, done, cancel, receiver }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Entries removed or given up on so far.
    #[must_use]
    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    /// Non-blocking check for completion.
    pub fn try_finish(&self) -> DeleteStatus {
        match self.receiver.try_recv() {
            Ok(outcome) => DeleteStatus::Done(outcome),
            Err(TryRecvError::Empty) => DeleteStatus::Running,
            Err(TryRecvError::Disconnected) => DeleteStatus::Done(DeleteOutcome {
                failures: vec![DeleteFailure {
                    path: self.path.clone(),
                    error: io::Error::other("delete thread exited without a result"),
                }],
                ..DeleteOutcome::default()
            }),
        }
    }
}

impl Drop for RunningDelete {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Split the subtree at `node` into files and links (any order) and
/// directories deepest-first, so each directory is empty when its turn comes.
fn collect(tree: &Tree, node: NodeId) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let mut stack = vec![node];
    while let Some(id) = stack.pop() {
        let entry = tree.node(id);
        match entry.kind {
            Kind::Directory => {
                dirs.push(tree.path(id));
                stack.extend(tree.children(id).iter().copied());
            }
            // Links are removed as links; whatever they point at stays.
            Kind::File | Kind::Symlink | Kind::Other => files.push(tree.path(id)),
        }
    }
    // `dirs` is in pre-order, so reversing gives children before parents.
    dirs.reverse();
    (files, dirs)
}

fn run(files: Vec<PathBuf>, dirs: Vec<PathBuf>, done: &AtomicU64, cancel: &AtomicBool) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();
    let mut record = |path: PathBuf, result: io::Result<()>| {
        done.fetch_add(1, Ordering::Relaxed);
        match result {
            Ok(()) => outcome.removed += 1,
            Err(error) => outcome.failures.push(DeleteFailure { path, error }),
        }
    };
    for path in files {
        if cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            return outcome;
        }
        let result = remove_file_force(&path);
        record(path, result);
    }
    for path in dirs {
        if cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            return outcome;
        }
        let result = remove_dir_force(&path);
        record(path, result);
    }
    outcome
}

/// Remove a file or link, clearing read-only, hidden and system attributes
/// and retrying when the first attempt is refused, then falling back to
/// delete-on-close, which also handles files opened with delete sharing.
fn remove_file_force(path: &Path) -> io::Result<()> {
    let first = match std::fs::remove_file(path) {
        Ok(()) => return Ok(()),
        // A directory symlink or junction shows up as a link in the scan but
        // needs the directory call.
        Err(_) if path.is_dir() => return std::fs::remove_dir(path),
        Err(err) => err,
    };
    if first.kind() != io::ErrorKind::PermissionDenied {
        return Err(first);
    }
    if sys::clear_protective_attributes(path)? && std::fs::remove_file(path).is_ok() {
        return Ok(());
    }
    sys::delete_on_close(path).map_err(|_| first)
}

/// Remove an emptied directory, clearing its attributes first when refused.
fn remove_dir_force(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied && sys::clear_protective_attributes(path)? => {
            std::fs::remove_dir(path).map_err(|_| err)
        }
        Err(err) => Err(err),
    }
}

#[cfg(any(windows, target_os = "linux"))]
impl crate::App {
    /// Start deleting `id` permanently on a worker thread, bypassing the
    /// Recycle Bin. Refused unless [`crate::App::permanent_delete`] is on. Only
    /// one deletion runs at a time. The front end is expected to have
    /// confirmed with the user; nothing here asks.
    pub fn delete_node_permanently(&mut self, id: NodeId) -> io::Result<()> {
        if !self.permanent_delete() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "permanent delete is not enabled"));
        }
        if self.delete.is_some() {
            return Err(io::Error::new(io::ErrorKind::ResourceBusy, "a deletion is already running"));
        }
        self.check_removable(id)?;
        if self.is_gone(id) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "already removed"));
        }
        let tree = self.tree.as_ref().ok_or(io::ErrorKind::NotFound)?;
        self.delete = Some(RunningDelete::spawn(tree, id));
        Ok(())
    }

    /// Adopt a finished deletion: the node counts as deleted when its path
    /// is gone, whatever happened underneath. Returns the path and outcome
    /// for the front end to report when a deletion has just finished.
    pub fn poll_delete(&mut self) -> Option<(PathBuf, DeleteOutcome)> {
        let running = self.delete.as_ref()?;
        let DeleteStatus::Done(outcome) = running.try_finish() else { return None };
        let running = self.delete.take()?;
        if !running.path.exists() {
            self.deleted.insert(running.node);
        }
        self.message = Some(if outcome.cancelled {
            format!("delete cancelled after {} items: {}", outcome.removed, running.path.display())
        } else if outcome.failures.is_empty() {
            format!("deleted {} items: {}", outcome.removed, running.path.display())
        } else {
            format!("delete failed for {} of {} items: {}", outcome.failures.len(), running.total, running.path.display())
        });
        Some((running.path.clone(), outcome))
    }
}

#[cfg(windows)]
mod sys {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY,
        FILE_ATTRIBUTE_SYSTEM, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileDispositionInfo, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, OPEN_EXISTING,
        SetFileAttributesW, SetFileInformationByHandle,
    };

    /// Wide, NUL-terminated form of `path` with the long-path prefix, so
    /// the calls work past 260 characters without any system setting.
    fn wide(path: &Path) -> Vec<u16> {
        let path = if path.as_os_str().as_encoded_bytes().starts_with(br"\\") {
            path.as_os_str().to_owned()
        } else {
            let mut prefixed = std::ffi::OsString::from(r"\\?\");
            prefixed.push(path.as_os_str());
            prefixed
        };
        path.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Strip read-only, hidden and system. Returns whether anything changed.
    pub fn clear_protective_attributes(path: &Path) -> io::Result<bool> {
        const PROTECTIVE: u32 = FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM;
        let wide = wide(path);
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
        if attributes == INVALID_FILE_ATTRIBUTES {
            return Err(io::Error::last_os_error());
        }
        if attributes & PROTECTIVE == 0 {
            return Ok(false);
        }
        let mut cleared = attributes & !PROTECTIVE;
        if cleared == 0 {
            cleared = FILE_ATTRIBUTE_NORMAL;
        }
        // SAFETY: as above.
        if unsafe { SetFileAttributesW(wide.as_ptr(), cleared) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(true)
    }

    /// Open with only DELETE access and mark the file to go when the last
    /// handle closes. Works on files other processes hold open with delete
    /// sharing, where `DeleteFileW` is refused.
    pub fn delete_on_close(path: &Path) -> io::Result<()> {
        let wide = wide(path);
        // SAFETY: `wide` is NUL-terminated; the handle is closed below.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                DELETE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: `info` is a valid FILE_DISPOSITION_INFO for the class given.
        let ok = unsafe {
            SetFileInformationByHandle(
                handle,
                FileDispositionInfo,
                std::ptr::from_ref(&info).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        };
        let err = if ok == 0 { Some(io::Error::last_os_error()) } else { None };
        // SAFETY: `handle` came from CreateFileW and is closed once.
        unsafe { CloseHandle(handle) };
        err.map_or(Ok(()), Err)
    }
}

/// Elsewhere the retries are no-ops; the module exists so the walker can be
/// unit-tested on any platform.
#[cfg(not(windows))]
mod sys {
    use std::io;
    use std::path::Path;

    pub fn clear_protective_attributes(_path: &Path) -> io::Result<bool> {
        Ok(false)
    }

    pub fn delete_on_close(path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dirstats_scan::{ScanOptions, scan};
    use std::fs;

    fn wait(running: &RunningDelete) -> DeleteOutcome {
        loop {
            if let DeleteStatus::Done(outcome) = running.try_finish() {
                return outcome;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn removes_a_tree_bottom_up_and_reports_counts() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        fs::write(dir.path().join("a/b/deep"), b"x").unwrap();
        fs::write(dir.path().join("a/top"), b"y").unwrap();
        fs::write(dir.path().join("keep"), b"z").unwrap();
        let tree = scan(dir.path(), &ScanOptions::default()).unwrap();
        let a = tree.children(tree.root()).iter().copied().find(|&id| tree.node(id).name.as_ref() == "a").unwrap();
        let running = RunningDelete::spawn(&tree, a);
        assert_eq!(running.total, 4);
        let outcome = wait(&running);
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
        assert_eq!(outcome.removed, 4);
        assert!(!outcome.cancelled);
        assert!(!dir.path().join("a").exists());
        assert!(dir.path().join("keep").exists(), "siblings are untouched");
    }

    #[cfg(unix)]
    #[test]
    fn links_are_removed_without_following() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target/file"), b"x").unwrap();
        fs::create_dir(dir.path().join("victim")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("target"), dir.path().join("victim/link")).unwrap();
        let tree = scan(dir.path(), &ScanOptions::default()).unwrap();
        let victim =
            tree.children(tree.root()).iter().copied().find(|&id| tree.node(id).name.as_ref() == "victim").unwrap();
        let outcome = wait(&RunningDelete::spawn(&tree, victim));
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
        assert!(!dir.path().join("victim").exists());
        assert!(dir.path().join("target/file").exists(), "link target must survive");
    }

    #[test]
    fn failures_are_collected_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("d")).unwrap();
        fs::write(dir.path().join("d/gone"), b"x").unwrap();
        fs::write(dir.path().join("d/stays"), b"y").unwrap();
        let tree = scan(dir.path(), &ScanOptions::default()).unwrap();
        let d = tree.children(tree.root())[0];
        // Remove one file behind the scan's back so its deletion fails.
        fs::remove_file(dir.path().join("d/gone")).unwrap();
        let outcome = wait(&RunningDelete::spawn(&tree, d));
        assert_eq!(outcome.failures.len(), 1);
        assert!(outcome.failures[0].path.ends_with("gone"));
        assert_eq!(outcome.removed, 2, "the other file and the directory still go");
        assert!(!dir.path().join("d").exists());
    }
}
