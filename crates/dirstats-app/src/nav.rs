// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Moving around a finished tree: the current directory and its
//! selection, the way back, and which directories a tree view has open.
//!
//! Everything here is a method on [`App`]:
//!
//! - where you are: [`App::dir`], [`App::entries`], [`App::selected`],
//!   [`App::breadcrumbs`]
//! - moving the selection: [`App::select`], [`App::move_selection`],
//!   [`App::select_first`], [`App::select_last`]
//! - moving between directories: [`App::enter`], [`App::back`],
//!   [`App::can_back`], [`App::zoom_to`], [`App::reveal`]
//! - tree views: [`App::toggle_expanded`], [`App::expand_to`],
//!   [`App::tree_rows`]

use crate::{App, NodeId};

impl App {
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
}

#[cfg(test)]
mod tests {
    use crate::tests::app_with_scan;

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
