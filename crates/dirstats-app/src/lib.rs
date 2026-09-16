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
    /// when known (macOS), which is what [`App::put_back`] needs. Their
    /// descendants count as trashed too.
    pub trashed: foldhash::HashMap<NodeId, Option<PathBuf>>,
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
        self.expanded.clear();
        self.trashed.clear();
        self.tree = Some(tree);
    }

    /// Whether `id` or any ancestor was moved to the trash since the scan.
    #[must_use]
    pub fn is_trashed(&self, id: NodeId) -> bool {
        if self.trashed.is_empty() {
            return false;
        }
        let Some(tree) = &self.tree else { return false };
        let mut current = Some(id);
        while let Some(n) = current {
            if self.trashed.contains_key(&n) {
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
    #[cfg(feature = "trash")]
    pub fn trash_node(&mut self, id: NodeId) -> io::Result<()> {
        let path = self.path_of(id).ok_or(io::ErrorKind::NotFound)?;
        let location = platform_trash(&path)?;
        self.trashed.insert(id, location);
        self.message = Some(format!("moved to trash: {}", path.display()));
        Ok(())
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
        std::fs::rename(&location, &original)?;
        self.trashed.remove(&id);
        self.message = Some(format!("put back: {}", original.display()));
        Ok(())
    }
}

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

#[cfg(all(feature = "trash", not(target_os = "macos")))]
fn platform_trash(path: &Path) -> io::Result<Option<PathBuf>> {
    trash::delete(path).map_err(io::Error::other)?;
    Ok(None)
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

    /// Moves a real temporary file to the system trash; run explicitly with
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
        assert!(app.message.as_deref().unwrap().starts_with("moved to trash"));
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
