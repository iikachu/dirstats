// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Application state shared by every dirstats front end.
//!
//! A front end owns an [`App`], forwards user intent as method calls, and
//! draws whatever the state says. Scans run on a worker thread; call
//! [`App::poll`] on every tick to pick up the result.

pub mod backup;
pub mod cloud;
#[cfg(feature = "trash")]
pub mod delete;
pub mod format;
pub mod locations;
pub mod scanner;

#[cfg(feature = "trash")]
pub use delete::{DeleteFailure, DeleteOutcome, DeleteStatus, RunningDelete};
pub use dirstats_scan::{self as scan, NodeId, ScanOptions, SizeMetric, Tree};
pub use scanner::{RunningScan, ScanStatus};

use std::io;
#[cfg(feature = "trash")]
use std::path::Path;
use std::path::PathBuf;

/// Navigation state within a finished [`Tree`].
#[derive(Clone, Debug)]
pub struct Cursor {
    /// Directory whose children are listed.
    pub dir: NodeId,
    /// Index into `tree.children(dir)`.
    pub selected: usize,
    /// Parent positions, innermost last, restored by [`App::back`].
    history: Vec<(NodeId, usize)>,
}

#[derive(Debug, Default)]
pub struct App {
    pub tree: Option<Tree>,
    pub cursor: Option<Cursor>,
    pub scan: Option<RunningScan>,
    pub options: ScanOptions,
    /// One-line status for the front end to show, cleared on the next action.
    pub message: Option<String>,
    /// Node under the pointer, for front ends with one.
    pub hovered: Option<NodeId>,
    /// Directories opened in a tree view.
    pub expanded: foldhash::HashSet<NodeId>,
    /// Nodes moved to the trash since the last scan, with where they went
    /// when known, which is what [`App::put_back`] needs: the trashed item's
    /// path on macOS, its trash entry's id elsewhere. Their
    /// descendants count as trashed too.
    pub trashed: foldhash::HashMap<NodeId, Option<PathBuf>>,
    /// Nodes deleted permanently since the last scan (Windows). Their
    /// descendants count as deleted too.
    pub deleted: foldhash::HashSet<NodeId>,
    /// Nodes whose iCloud download was removed since the scan; their
    /// subtrees are placeholders now, whatever the scanned sizes say.
    pub evicted: foldhash::HashSet<NodeId>,
    /// Permanent deletion in progress, if any. Front ends call
    /// [`App::poll_delete`] each tick to adopt the outcome.
    #[cfg(all(any(windows, target_os = "linux"), feature = "trash"))]
    pub delete: Option<RunningDelete>,
    /// Set by the permanent-delete gate for this run only; never saved.
    permanent_delete: bool,
    /// Whether the scanned tree sits on a Time Machine backup volume;
    /// probed once per scan in [`App::set_tree`].
    pub backup_volume: bool,
    /// Root of the last scan, for [`App::rescan`].
    last_root: Option<PathBuf>,
}

impl App {
    #[must_use]
    pub fn new(options: ScanOptions) -> Self {
        Self { options, ..Self::default() }
    }

    /// Whether permanent deletion is offered; only on Windows and Linux,
    /// where the trash can refuse items, and only once the gate is passed.
    #[must_use]
    pub fn permanent_delete(&self) -> bool {
        cfg!(any(windows, target_os = "linux")) && self.permanent_delete
    }

    /// Pass the permanent-delete gate until the app quits.
    pub fn enable_permanent_delete(&mut self) {
        self.permanent_delete = true;
    }

    /// Start scanning `root` on a worker thread, cancelling any running scan.
    pub fn start_scan(&mut self, root: impl Into<PathBuf>) {
        self.cancel_scan();
        let root = root.into();
        self.last_root = Some(root.clone());
        self.scan = Some(RunningScan::spawn(root, self.options.clone()));
    }

    /// Scan the last root again with the current options.
    pub fn rescan(&mut self) {
        if let Some(root) = self.last_root.clone() {
            self.start_scan(root);
        }
    }

    pub fn cancel_scan(&mut self) {
        if let Some(scan) = self.scan.take() {
            scan.cancel();
        }
    }

    /// Adopt a finished scan's tree if one is ready. Returns true when the tree changed.
    pub fn poll(&mut self) -> bool {
        let Some(scan) = &self.scan else { return false };
        match scan.try_finish() {
            ScanStatus::Running => false,
            ScanStatus::Done(result) => {
                self.scan = None;
                match result {
                    Ok(tree) => {
                        self.set_tree(tree);
                        true
                    }
                    Err(err) => {
                        self.message = Some(format!("scan failed: {err}"));
                        false
                    }
                }
            }
        }
    }

    pub fn set_tree(&mut self, tree: Tree) {
        self.cursor = Some(Cursor { dir: tree.root(), selected: 0, history: Vec::new() });
        self.hovered = None;
        self.expanded.clear();
        self.trashed.clear();
        self.deleted.clear();
        self.evicted.clear();
        self.backup_volume = backup::is_backup_volume(&tree.path(tree.root()));
        self.tree = Some(tree);
    }

    /// Whether `id` or any ancestor was moved to the trash since the scan.
    #[must_use]
    pub fn is_trashed(&self, id: NodeId) -> bool {
        !self.trashed.is_empty() && self.ancestor_or_self(id, |n| self.trashed.contains_key(&n))
    }

    /// Whether `id` or any ancestor was deleted permanently since the scan.
    #[must_use]
    pub fn is_deleted(&self, id: NodeId) -> bool {
        !self.deleted.is_empty() && self.ancestor_or_self(id, |n| self.deleted.contains(&n))
    }

    /// Whether `id` or an ancestor had its iCloud download removed since the scan.
    #[must_use]
    pub fn is_evicted(&self, id: NodeId) -> bool {
        !self.evicted.is_empty() && self.ancestor_or_self(id, |n| self.evicted.contains(&n))
    }

    /// Whether `id` no longer exists at its scanned path because of an
    /// action taken here: trashed or deleted, itself or through an ancestor.
    #[must_use]
    pub fn is_gone(&self, id: NodeId) -> bool {
        self.is_trashed(id) || self.is_deleted(id)
    }

    fn ancestor_or_self(&self, id: NodeId, mut pred: impl FnMut(NodeId) -> bool) -> bool {
        let Some(tree) = &self.tree else { return false };
        let mut current = Some(id);
        while let Some(n) = current {
            if pred(n) {
                return true;
            }
            current = tree.node(n).parent;
        }
        false
    }

    /// Open or close a directory in a tree view. Returns the new state.
    pub fn toggle_expanded(&mut self, id: NodeId) -> bool {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
            return true;
        }
        false
    }

    /// Open every directory from the current directory down to `id`'s parent.
    pub fn expand_to(&mut self, id: NodeId) {
        let Some(tree) = &self.tree else { return };
        let stop = self.dir();
        let mut current = tree.node(id).parent;
        while let Some(dir) = current {
            if Some(dir) == stop {
                break;
            }
            self.expanded.insert(dir);
            current = tree.node(dir).parent;
        }
    }

    /// Rows of a tree view rooted at the current directory: each node in
    /// display order with its depth below the root, following `expanded`.
    #[must_use]
    pub fn tree_rows(&self) -> Vec<(NodeId, u32)> {
        let (Some(tree), Some(dir)) = (&self.tree, self.dir()) else { return Vec::new() };
        let mut rows = Vec::new();
        let mut stack: Vec<(NodeId, u32)> = tree.children(dir).iter().rev().map(|&c| (c, 0)).collect();
        while let Some((id, depth)) = stack.pop() {
            rows.push((id, depth));
            if self.expanded.contains(&id) {
                stack.extend(tree.children(id).iter().rev().map(|&c| (c, depth + 1)));
            }
        }
        rows
    }

    /// Current directory node.
    #[must_use]
    pub fn dir(&self) -> Option<NodeId> {
        self.cursor.as_ref().map(|c| c.dir)
    }

    /// Make `dir` the current directory, remembering the way back. Any node
    /// with children is accepted, so a treemap can zoom straight into a deep folder.
    pub fn zoom_to(&mut self, dir: NodeId) -> bool {
        let Some(tree) = &self.tree else { return false };
        if tree.children(dir).is_empty() {
            return false;
        }
        let Some(cursor) = &mut self.cursor else { return false };
        if cursor.dir == dir {
            return false;
        }
        cursor.history.push((cursor.dir, cursor.selected));
        cursor.dir = dir;
        cursor.selected = 0;
        true
    }

    /// Select `id` wherever it is: zoom to its parent, then pick it.
    pub fn reveal(&mut self, id: NodeId) -> bool {
        let Some(tree) = &self.tree else { return false };
        let Some(parent) = tree.node(id).parent else { return false };
        if self.dir() != Some(parent) {
            self.zoom_to(parent);
        }
        self.select(id)
    }

    #[must_use]
    pub fn is_scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// Children of the current directory, largest first.
    #[must_use]
    pub fn entries(&self) -> &[NodeId] {
        match (&self.tree, &self.cursor) {
            (Some(tree), Some(cursor)) => tree.children(cursor.dir),
            _ => &[],
        }
    }

    #[must_use]
    pub fn selected(&self) -> Option<NodeId> {
        let cursor = self.cursor.as_ref()?;
        self.entries().get(cursor.selected).copied()
    }

    /// Path from the root to the current directory, root first.
    #[must_use]
    pub fn breadcrumbs(&self) -> Vec<NodeId> {
        let (Some(tree), Some(cursor)) = (&self.tree, &self.cursor) else { return Vec::new() };
        let mut out = Vec::new();
        let mut current = Some(cursor.dir);
        while let Some(id) = current {
            out.push(id);
            current = tree.node(id).parent;
        }
        out.reverse();
        out
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.entries().len();
        if let Some(cursor) = &mut self.cursor
            && len > 0
        {
            let next = cursor.selected as isize + delta;
            cursor.selected = next.clamp(0, len as isize - 1) as usize;
        }
    }

    pub fn select_first(&mut self) {
        if let Some(cursor) = &mut self.cursor {
            cursor.selected = 0;
        }
    }

    pub fn select_last(&mut self) {
        let len = self.entries().len();
        if let Some(cursor) = &mut self.cursor {
            cursor.selected = len.saturating_sub(1);
        }
    }

    /// Select `id` if it is a child of the current directory.
    pub fn select(&mut self, id: NodeId) -> bool {
        let Some(index) = self.entries().iter().position(|&c| c == id) else { return false };
        if let Some(cursor) = &mut self.cursor {
            cursor.selected = index;
        }
        true
    }

    /// Descend into the selected entry if it has children.
    pub fn enter(&mut self) -> bool {
        let Some(id) = self.selected() else { return false };
        let Some(tree) = &self.tree else { return false };
        if tree.children(id).is_empty() {
            return false;
        }
        let cursor = self.cursor.as_mut().expect("cursor exists when selected() is Some");
        cursor.history.push((cursor.dir, cursor.selected));
        cursor.dir = id;
        cursor.selected = 0;
        true
    }

    /// Whether [`App::back`] has somewhere to go.
    #[must_use]
    pub fn can_back(&self) -> bool {
        self.cursor.as_ref().is_some_and(|c| !c.history.is_empty())
    }

    /// Return to the parent directory, reselecting the directory just left.
    pub fn back(&mut self) -> bool {
        let Some(cursor) = &mut self.cursor else { return false };
        let Some((dir, selected)) = cursor.history.pop() else { return false };
        cursor.dir = dir;
        cursor.selected = selected;
        true
    }

    #[must_use]
    pub fn path_of(&self, id: NodeId) -> Option<PathBuf> {
        self.tree.as_ref().map(|t| t.path(id))
    }

    /// Open the selected entry with the desktop's default handler.
    #[cfg(feature = "open")]
    pub fn open_selected(&mut self) -> io::Result<()> {
        let id = self.selected().ok_or(io::ErrorKind::NotFound)?;
        self.open_node(id)
    }

    /// Open `id` with the desktop's default handler.
    #[cfg(feature = "open")]
    pub fn open_node(&mut self, id: NodeId) -> io::Result<()> {
        let path = self.path_of(id).ok_or(io::ErrorKind::NotFound)?;
        open::that_detached(&path)?;
        self.message = Some(format!("opened {}", path.display()));
        Ok(())
    }

    /// Move the selected entry to the trash. The tree is not rescanned.
    #[cfg(feature = "trash")]
    pub fn trash_selected(&mut self) -> io::Result<()> {
        let id = self.selected().ok_or(io::ErrorKind::NotFound)?;
        self.trash_node(id)
    }

    /// Move `id` to the trash. The tree is not rescanned.
    ///
    /// On Windows the shell refuses, rather than permanently deleting,
    /// items too large for the Recycle Bin or on a drive without one; the
    /// error then names neither cause, so front ends offer
    /// [`App::delete_node_permanently`] as the next step.
    #[cfg(feature = "trash")]
    pub fn trash_node(&mut self, id: NodeId) -> io::Result<()> {
        let path = self.path_of(id).ok_or(io::ErrorKind::NotFound)?;
        self.check_removable(id)?;
        let location = dataless::materialising(|| platform_trash(&path))?;
        self.trashed.insert(id, location);
        self.message = Some(format!("moved to {}: {}", TRASH_NAME, path.display()));
        Ok(())
    }

    /// Remove the local copy of a synced iCloud item, keeping it in the cloud.
    #[cfg(feature = "icloud")]
    pub fn evict_node(&mut self, id: NodeId) -> io::Result<()> {
        let path = self.path_of(id).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such entry"))?;
        cloud::evict(&path)?;
        self.evicted.insert(id);
        self.message = Some(format!("removed download: {} (rescan to update sizes)", path.display()));
        Ok(())
    }

    /// Whether `id` is part of a Time Machine backup; see [`backup::NOTE`].
    #[must_use]
    pub fn is_time_machine(&self, id: NodeId) -> bool {
        // By ancestor names rather than `tree.path`, which allocates, since
        // front ends ask for every displayed row. The scan root's own path
        // was covered by the volume probe.
        let Some(tree) = &self.tree else { return false };
        self.backup_volume || self.ancestor_or_self(id, |n| &*tree.node(n).name == std::ffi::OsStr::new("Backups.backupdb"))
    }

    /// Why `id` must not be removed, if it is one of the places no disk
    /// usage tool should offer to delete: the scan root, a drive or
    /// filesystem root, or the user's home folder.
    pub fn check_removable(&self, id: NodeId) -> io::Result<()> {
        let refuse = |why: &str| Err(io::Error::new(io::ErrorKind::PermissionDenied, why.to_string()));
        let Some(tree) = &self.tree else { return refuse("no scan") };
        if id == tree.root() {
            return refuse("the scanned folder itself is not removable; scan its parent");
        }
        let path = tree.path(id);
        if path.parent().is_none_or(|p| p.as_os_str().is_empty()) {
            return refuse("a drive root is not removable");
        }
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
        if home.is_some_and(|home| home == path) {
            return refuse("the home folder is not removable");
        }
        if backup::BLOCKS_TRASH && self.is_time_machine(id) {
            return refuse(backup::NOTE);
        }
        Ok(())
    }

    /// Start deleting `id` permanently on a worker thread, bypassing the
    /// Recycle Bin. Refused unless [`App::permanent_delete`] is on. Only
    /// one deletion runs at a time. The front end is expected to have
    /// confirmed with the user; nothing here asks.
    #[cfg(all(any(windows, target_os = "linux"), feature = "trash"))]
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
    #[cfg(all(any(windows, target_os = "linux"), feature = "trash"))]
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

    /// Whether [`App::put_back`] can restore `id`: it was trashed by this
    /// app and the trash location is known.
    #[must_use]
    pub fn can_put_back(&self, id: NodeId) -> bool {
        self.trashed.get(&id).is_some_and(|location| location.is_some())
    }

    /// Move `id` back from the trash to where it was scanned.
    #[cfg(feature = "trash")]
    pub fn put_back(&mut self, id: NodeId) -> io::Result<()> {
        let original = self.path_of(id).ok_or(io::ErrorKind::NotFound)?;
        let Some(Some(location)) = self.trashed.get(&id).cloned() else {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "trash location unknown"));
        };
        if original.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "something else is at the original path"));
        }
        dataless::materialising(|| platform_put_back(&location, &original))?;
        self.trashed.remove(&id);
        self.message = Some(format!("put back: {}", original.display()));
        Ok(())
    }
}

/// Actions that move or open files must be allowed to fetch an evicted
/// iCloud item, while the scanner keeps the process from ever doing so
/// (see the scan crate). A thread-scoped policy overrides the process
/// one, so the acting thread turns fetching on just for the operation.
#[cfg(target_os = "macos")]
mod dataless {
    use std::ffi::c_int;
    // From <sys/resource.h>; not in the libc crate this project pins.
    const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;
    const IOPOL_SCOPE_THREAD: c_int = 1;
    const IOPOL_MATERIALIZE_DATALESS_FILES_DEFAULT: c_int = 0;
    const IOPOL_MATERIALIZE_DATALESS_FILES_ON: c_int = 2;

    unsafe extern "C" {
        fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
    }

    pub fn materialising<T>(f: impl FnOnce() -> T) -> T {
        // SAFETY: plain policy calls on the current thread; failure only
        // means the policy is unsupported and the operation runs as is.
        unsafe { setiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, IOPOL_SCOPE_THREAD, IOPOL_MATERIALIZE_DATALESS_FILES_ON) };
        let result = f();
        unsafe { setiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, IOPOL_SCOPE_THREAD, IOPOL_MATERIALIZE_DATALESS_FILES_DEFAULT) };
        result
    }
}

#[cfg(not(target_os = "macos"))]
mod dataless {
    pub fn materialising<T>(f: impl FnOnce() -> T) -> T {
        f()
    }
}

/// What the platform calls its trash, for messages.
#[cfg(feature = "trash")]
const TRASH_NAME: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };

/// Move `path` to the trash and return where it went, when the platform
/// reports it. On macOS the direct NSFileManager call is used rather than
/// scripting Finder, so no Automation permission is requested, and the
/// resulting URL is kept for Put Back. Elsewhere the trash crate's default
/// applies and the location is unknown.
#[cfg(all(feature = "trash", target_os = "macos"))]
fn platform_trash(path: &Path) -> io::Result<Option<PathBuf>> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let Some(utf8) = path.to_str() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"));
    };
    let url = NSURL::fileURLWithPath(&NSString::from_str(utf8));
    let mut resulting = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|err| io::Error::other(err.localizedDescription().to_string()))?;
    Ok(resulting.and_then(|url| url.path()).map(|p| PathBuf::from(p.to_string())))
}

/// Elsewhere (the Windows Recycle Bin, the freedesktop trash on Linux and
/// BSD) the trash entry is looked up after the move, the most recent one
/// from `path`, and its id kept for Put Back.
#[cfg(all(feature = "trash", not(target_os = "macos")))]
fn platform_trash(path: &Path) -> io::Result<Option<PathBuf>> {
    // Paths are compared canonical, as the scan may name the folder by
    // its short (8.3) name or a symlink and the trash by the real one.
    let parent = path.parent().and_then(|p| p.canonicalize().ok());
    trash::delete(path).map_err(io::Error::other)?;
    let (Some(parent), Some(name)) = (parent, path.file_name()) else { return Ok(None) };
    // The item is in the bin whether or not it can be found there again.
    let Ok(items) = trash::os_limited::list() else { return Ok(None) };
    Ok(items
        .into_iter()
        .filter(|item| item.name == name && item.original_parent.canonicalize().is_ok_and(|p| p == parent))
        .max_by_key(|item| item.time_deleted)
        .map(|item| PathBuf::from(item.id)))
}

/// Move a trashed item from `location`, as [`platform_trash`] reported it,
/// back to `original`.
#[cfg(all(feature = "trash", target_os = "macos"))]
fn platform_put_back(location: &Path, original: &Path) -> io::Result<()> {
    std::fs::rename(location, original)
}

/// Restored through the trash rather than renamed, so its record of the
/// item (the Recycle Bin's `$I` file, the `.trashinfo`) goes with it.
#[cfg(all(feature = "trash", not(target_os = "macos")))]
fn platform_put_back(location: &Path, original: &Path) -> io::Result<()> {
    let items = trash::os_limited::list().map_err(io::Error::other)?;
    let Some(item) = items.into_iter().find(|item| Path::new(&item.id) == location) else {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("no longer in the {TRASH_NAME}")));
    };
    trash::os_limited::restore_all([item]).map_err(io::Error::other)?;
    if original.exists() { Ok(()) } else { Err(io::Error::other("restored somewhere else")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn app_with_scan() -> App {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/big"), vec![0u8; 5000]).unwrap();
        fs::write(dir.path().join("small"), vec![0u8; 10]).unwrap();
        let mut app = App::new(ScanOptions { size_metric: SizeMetric::Apparent, ..Default::default() });
        app.start_scan(dir.path());
        while !app.poll() {
            assert!(app.is_scanning(), "{:?}", app.message);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Keep the tempdir alive for the test by leaking it; it is tiny.
        std::mem::forget(dir);
        app
    }

    #[test]
    fn navigates_in_and_out() {
        let mut app = app_with_scan();
        assert_eq!(app.entries().len(), 2);
        assert!(app.enter(), "first entry is the larger directory");
        assert_eq!(app.breadcrumbs().len(), 2);
        assert_eq!(app.entries().len(), 1);
        assert!(!app.enter(), "files have no children");
        assert!(app.back());
        assert_eq!(app.cursor.as_ref().unwrap().selected, 0);
        app.move_selection(5);
        assert_eq!(app.cursor.as_ref().unwrap().selected, 1);
        assert!(!app.back());
    }

    #[test]
    fn tree_rows_follow_expansion() {
        let mut app = app_with_scan();
        assert_eq!(app.tree_rows().len(), 2, "two top-level entries");
        let sub = app.entries()[0];
        assert!(app.toggle_expanded(sub));
        let rows = app.tree_rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].1, 1, "child of sub is one level deeper");
        assert!(!app.toggle_expanded(sub));
        assert_eq!(app.tree_rows().len(), 2);
    }

    /// Moves a real temporary file to the system trash, and back again; run explicitly with
    /// `cargo test -p dirstats-app --features trash -- --ignored`.
    #[cfg(feature = "trash")]
    #[test]
    #[ignore]
    fn trashes_a_file_without_prompting() {
        let mut app = app_with_scan();
        let small = app.entries()[1];
        let path = app.path_of(small).unwrap();
        assert!(path.exists());
        app.trash_node(small).unwrap();
        assert!(!path.exists(), "file should have moved to the trash");
        assert!(app.is_trashed(small));
        assert!(app.message.as_deref().unwrap().starts_with("moved to "));
        assert!(app.can_put_back(small), "trash location not found");
        app.put_back(small).unwrap();
        assert!(path.exists(), "file should be back");
        assert!(!app.is_trashed(small));
    }

    #[test]
    fn trashed_state_covers_descendants_and_clears_on_rescan() {
        let mut app = app_with_scan();
        let sub = app.entries()[0];
        let big = app.tree.as_ref().unwrap().children(sub)[0];
        app.trashed.insert(sub, None);
        assert!(app.is_trashed(sub) && app.is_trashed(big));
        assert!(!app.is_trashed(app.entries()[1]));
        let tree = app.tree.clone().unwrap();
        app.set_tree(tree);
        assert!(!app.is_trashed(sub));
    }

    #[test]
    fn refuses_to_remove_the_scan_root_and_home() {
        let app = app_with_scan();
        let root = app.tree.as_ref().unwrap().root();
        assert!(app.check_removable(root).is_err());
        assert!(app.check_removable(app.entries()[0]).is_ok());
        assert!(app.is_gone(app.entries()[0]) == false);
    }

    #[test]
    fn zooms_and_reveals_deep_nodes() {
        let mut app = app_with_scan();
        let tree = app.tree.as_ref().unwrap();
        let sub = tree.children(tree.root())[0];
        let big = tree.children(sub)[0];
        assert!(app.reveal(big));
        assert_eq!(app.dir(), Some(sub));
        assert_eq!(app.selected(), Some(big));
        assert!(app.back());
        assert_eq!(app.dir(), Some(app.tree.as_ref().unwrap().root()));
    }
}
