// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! The dirstats library: application state shared by every front end,
//! the background work behind it, and the file actions it offers.
//!
//! A front end owns an [`App`], forwards user intent as method calls, and
//! draws whatever the state says. Scans run on a worker thread; call
//! [`App::poll`] on every tick to pick up the result.
//!
//! The layers below are re-exported, so this is the only dirstats crate a
//! front end or another program needs: [`scan`] builds the sized tree and
//! [`treemap`] lays it out and renders it.

pub mod backup;
pub mod cloud;
#[cfg(feature = "delete")]
pub mod delete;
pub mod format;
pub mod locations;
pub mod nav;
#[cfg(feature = "open")]
pub mod open;
pub mod scanner;
#[cfg(feature = "trash")]
pub mod trash;

#[cfg(feature = "delete")]
pub use delete::{DeleteFailure, DeleteOutcome, DeleteStatus, RunningDelete};
pub use dirstats_scan::{self as scan, NodeId, ScanOptions, SizeMetric, Tree};
pub use dirstats_treemap as treemap;
pub use scanner::{RunningScan, ScanStatus};

use std::io;
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
    /// when known, which is what `App::put_back` (`trash` feature) needs:
    /// the trashed item's path on macOS, its trash entry's id elsewhere.
    /// Their descendants count as trashed too.
    pub trashed: foldhash::HashMap<NodeId, Option<PathBuf>>,
    /// Nodes deleted permanently since the last scan (Windows). Their
    /// descendants count as deleted too.
    pub deleted: foldhash::HashSet<NodeId>,
    /// Nodes whose iCloud download was removed since the scan; their
    /// subtrees are placeholders now, whatever the scanned sizes say.
    pub evicted: foldhash::HashSet<NodeId>,
    /// Permanent deletion in progress, if any. Front ends call
    /// [`App::poll_delete`] each tick to adopt the outcome.
    #[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
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

    #[must_use]
    pub fn is_scanning(&self) -> bool {
        self.scan.is_some()
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

    /// Whether `App::put_back` (`trash` feature) can restore `id`: it was trashed by this
    /// app and the trash location is known.
    #[must_use]
    pub fn can_put_back(&self, id: NodeId) -> bool {
        self.trashed.get(&id).is_some_and(|location| location.is_some())
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

    #[must_use]
    pub fn path_of(&self, id: NodeId) -> Option<PathBuf> {
        self.tree.as_ref().map(|t| t.path(id))
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
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;

    pub(crate) fn app_with_scan() -> App {
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
}
