// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.


//! Selection, per-node state lookups and carrying out context-menu actions.

use dirstats_app::NodeId;

#[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
use crate::dialogs::Dialog;
#[cfg(feature = "trash")]
use crate::menu::TRASH_NAME;
use crate::menu::{NodeAction, Permanent, TrashState};
use crate::{Gui, Selection};

impl Gui {
    pub(super) fn selected_node(&self) -> Option<NodeId> {
        match &self.selection {
            Some(Selection::Node(id)) => Some(*id),
            _ => None,
        }
    }

    pub(super) fn selected_extension(&self) -> Option<&Option<String>> {
        match &self.selection {
            Some(Selection::Extension(ext)) => Some(ext),
            _ => None,
        }
    }

    /// Select a node, open the tree down to it and scroll it into view,
    /// without changing the zoom.
    pub(super) fn select(&mut self, id: NodeId) {
        self.selection = Some(Selection::Node(id));
        self.app.expand_to(id);
        self.scroll_to = Some(id);
    }

    pub(super) fn trash_state(&self, node: NodeId) -> TrashState {
        if self.app.can_put_back(node) {
            TrashState::CanPutBack
        } else if self.app.is_trashed(node) {
            TrashState::Trashed
        } else if self.app.is_deleted(node) {
            TrashState::Deleted
        } else if self.app.is_time_machine(node) {
            TrashState::TimeMachine
        } else {
            TrashState::Present
        }
    }

    pub(super) fn permanent(&self) -> Permanent {
        if !cfg!(any(windows, target_os = "linux")) {
            Permanent::Unavailable
        } else if self.app.permanent_delete() {
            Permanent::Enabled
        } else {
            Permanent::Locked
        }
    }

    /// iCloud status of `node`, looked up once per tree. The lookups cost
    /// microseconds and only displayed rows ask, so this stays cheap.
    pub(super) fn cloud_status(&mut self, node: NodeId) -> dirstats_app::cloud::CloudStatus {
        use dirstats_app::cloud::{CloudStatus, status};
        if self.app.is_evicted(node) {
            return CloudStatus::Evicted;
        }
        if let Some(&status) = self.cloud.get(&node) {
            return status;
        }
        let Some(tree) = &self.app.tree else { return CloudStatus::Local };
        let entry = tree.node(node);
        let is_dir = !tree.children(node).is_empty();
        let status = status(&tree.path(node), is_dir, entry.apparent_size, entry.allocated_size);
        self.cloud.insert(node, status);
        status
    }

    /// Carry out a context-menu action on `node`.
    pub(super) fn apply(&mut self, node: NodeId, action: NodeAction) {
        self.select(node);
        match action {
            NodeAction::Zoom => self.zoom(node),
            NodeAction::CopyPath => {
                if let Some(path) = self.app.path_of(node) {
                    let text = path.display().to_string();
                    self.app.message = Some(format!("copied {text}"));
                    self.pending_copy = Some(text);
                }
            }
            #[cfg(feature = "open")]
            NodeAction::Open => {
                if let Err(err) = self.app.open_node(node) {
                    self.app.message = Some(format!("open failed: {err}"));
                }
            }
            #[cfg(feature = "trash")]
            NodeAction::Trash => {
                if let Err(err) = self.app.trash_node(node) {
                    self.app.message = Some(format!("move to {TRASH_NAME} failed: {err}"));
                    // On Windows the usual causes are an item too large for
                    // the bin or a drive without one, on Linux a mount with no trash folder; offer the way past them.
                    #[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
                    if self.app.check_removable(node).is_ok() {
                        self.dialog = Some(Dialog::TrashFailed { node, error: err.to_string() });
                    }
                }
            }
            #[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
            NodeAction::DeletePermanently => self.ask_delete(node),
            #[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
            NodeAction::EnablePermanentDelete => match self.app.check_removable(node) {
                Ok(()) => self.dialog = Some(Dialog::EnablePermanent { then: Some(node) }),
                Err(err) => self.app.message = Some(format!("delete failed: {err}")),
            },
            #[cfg(feature = "trash")]
            NodeAction::PutBack => {
                if let Err(err) = self.app.put_back(node) {
                    self.app.message = Some(format!("put back failed: {err}"));
                }
            }
            #[cfg(feature = "icloud")]
            NodeAction::Evict => {
                if let Err(err) = self.app.evict_node(node) {
                    self.app.message = Some(format!("remove download failed: {err}"));
                }
            }
        }
    }

    /// Zoom into `id`, or its parent when it is a file.
    pub(super) fn zoom(&mut self, id: NodeId) {
        let Some(tree) = &self.app.tree else { return };
        let target = if tree.children(id).is_empty() { tree.node(id).parent } else { Some(id) };
        if let Some(target) = target
            && self.app.zoom_to(target)
        {
            self.map_key = None;
        }
    }
}
