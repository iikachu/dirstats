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

pub mod format;
pub mod scanner;

pub use dirstats_scan::{self as scan, NodeId, ScanOptions, SizeMetric, Tree};
pub use scanner::{RunningScan, ScanStatus};

#[cfg(any(feature = "open", feature = "trash"))]
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
    /// Root of the last scan, for [`App::rescan`].
    last_root: Option<PathBuf>,
}

impl App {
    #[must_use]
    pub fn new(options: ScanOptions) -> Self {
        Self { options, ..Self::default() }
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
        self.tree = Some(tree);
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
        let path = self.selected().and_then(|id| self.path_of(id)).ok_or(io::ErrorKind::NotFound)?;
        open::that_detached(&path)?;
        self.message = Some(format!("opened {}", path.display()));
        Ok(())
    }

    /// Move the selected entry to the trash. The tree is not rescanned.
    #[cfg(feature = "trash")]
    pub fn trash_selected(&mut self) -> io::Result<()> {
        let path = self.selected().and_then(|id| self.path_of(id)).ok_or(io::ErrorKind::NotFound)?;
        trash::delete(&path).map_err(io::Error::other)?;
        self.message = Some(format!("moved to trash: {}", path.display()));
        Ok(())
    }
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
