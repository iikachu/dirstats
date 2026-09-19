// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Permanent-delete modals (Windows and Linux): the gate, the confirmation, progress and the report.

use dirstats_core::{NodeId, format};
use eframe::egui;

use crate::Gui;
use crate::menu::TRASH_NAME;

/// When permanent delete is the way forward.
const WHEN: &str = if cfg!(windows) {
    "items too large to recycle or on drives without a Recycle Bin"
} else {
    "items on drives or mounts without a Trash folder"
};

/// Why the trash refused an item.
#[cfg(feature = "trash")]
const TRASH_REFUSED: &str = if cfg!(windows) {
    "The item may be too large to recycle, or the drive has no Recycle Bin."
} else {
    "The drive or mount may have no Trash folder, or it could not be created."
};

/// A modal in front of the window; at most one at a time.
#[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
#[derive(Debug)]
pub(super) enum Dialog {
    /// The permanent-delete gate. `then` is the node whose deletion was
    /// asked for, confirmed next if the gate is accepted.
    EnablePermanent { then: Option<NodeId> },
    /// Confirm deleting `node` without the Recycle Bin or Trash.
    ConfirmDelete(NodeId),
    /// The Recycle Bin or Trash refused `node`, with the error it gave; offer
    /// permanent deletion instead.
    #[cfg(feature = "trash")]
    TrashFailed { node: NodeId, error: String },
    /// A finished deletion of `path` that was cancelled or left failures.
    Report { path: std::path::PathBuf, outcome: dirstats_core::DeleteOutcome },
}

impl Gui {
    /// Open the confirmation for deleting `node` permanently, or explain
    /// why that is refused.
    #[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
    pub(super) fn ask_delete(&mut self, node: NodeId) {
        match self.app.check_removable(node) {
            Ok(()) if self.app.delete.is_some() => self.app.message = Some("delete failed: a deletion is already running".into()),
            Ok(()) => self.dialog = Some(Dialog::ConfirmDelete(node)),
            Err(err) => self.app.message = Some(format!("delete failed: {err}")),
        }
    }

    /// Collect a finished deletion (opening a report if it was cancelled or
    /// left failures), then draw the running-deletion progress or, when
    /// none is running, whichever dialog is open. Both are modal: nothing
    /// behind them takes input.
    #[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        use egui::{Modal, RichText};
        const WIDTH: f32 = 440.0;

        // A clean deletion only needs the footer message; anything else gets a report.
        if let Some((path, outcome)) = self.app.poll_delete()
            && (!outcome.failures.is_empty() || outcome.cancelled)
        {
            self.dialog = Some(Dialog::Report { path, outcome });
        }

        if let Some(running) = &self.app.delete {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            let (done, total) = (running.done(), running.total.max(1));
            let path = running.path.display().to_string();
            let mut cancel = false;
            Modal::new(egui::Id::new("delete-progress")).show(ctx, |ui| {
                ui.set_width(WIDTH);
                ui.heading("Deleting permanently");
                ui.add(egui::Label::new(RichText::new(path).weak()).truncate());
                ui.add_space(8.0);
                ui.add(egui::ProgressBar::new(done as f32 / total as f32).text(format!("{done} of {total} items")));
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                });
            });
            if cancel {
                running.cancel();
            }
            return;
        }

        let Some(dialog) = self.dialog.take() else { return };
        let mut next = None;
        let mut confirmed_delete = None;
        let mut enable = None;
        let response = Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = 8.0;
            match &dialog {
                Dialog::EnablePermanent { then } => {
                    ui.heading("Enable permanent delete?");
                    ui.label(format!(
                        "Files deleted this way skip the {TRASH_NAME} and cannot be recovered. \
                         Use it for {WHEN}. \
                         You will be asked to confirm each deletion.",
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Enable").strong()).clicked() {
                            enable = Some(*then);
                        }
                        if ui.button("Cancel").clicked() {
                            next = Some(None);
                        }
                    });
                }
                Dialog::ConfirmDelete(node) => {
                    let path = self.app.path_of(*node).unwrap_or_default();
                    let tree = self.app.tree.as_ref();
                    let is_dir = tree.is_some_and(|t| !t.children(*node).is_empty());
                    let size = tree.map(|t| format::size(t.size(*node))).unwrap_or_default();
                    ui.heading(if is_dir { "Delete this folder permanently?" } else { "Delete this file permanently?" });
                    ui.add(egui::Label::new(RichText::new(path.display().to_string()).strong()).wrap());
                    let count = tree.map(|t| t.node(*node).file_count).unwrap_or_default();
                    ui.label(RichText::new(if is_dir { format!("{size}, {count} files") } else { size }).weak());
                    ui.label(
                        RichText::new(format!(
                            "It will not go to the {TRASH_NAME} and cannot be recovered. \
                                        Links are removed without touching what they point at."
                        ))
                        .color(ui.visuals().error_fg_color),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let delete = egui::Button::new(RichText::new("Delete Permanently").strong().color(ui.visuals().error_fg_color));
                        if ui.add(delete).clicked() {
                            confirmed_delete = Some(*node);
                        }
                        if ui.button("Cancel").clicked() {
                            next = Some(None);
                        }
                    });
                }
                #[cfg(feature = "trash")]
                Dialog::TrashFailed { node, error } => {
                    let path = self.app.path_of(*node).unwrap_or_default();
                    ui.heading(format!("Couldn't move to the {TRASH_NAME}"));
                    ui.add(egui::Label::new(RichText::new(path.display().to_string()).strong()).wrap());
                    ui.label(TRASH_REFUSED);
                    ui.label(RichText::new(error).weak().small());
                    let enabled = self.app.permanent_delete();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if enabled { "Delete Permanently…" } else { "Enable Permanent Delete…" };
                        if ui.button(RichText::new(label).strong()).clicked() {
                            next = Some(Some(if enabled {
                                Dialog::ConfirmDelete(*node)
                            } else {
                                Dialog::EnablePermanent { then: Some(*node) }
                            }));
                        }
                        if ui.button("Cancel").clicked() {
                            next = Some(None);
                        }
                    });
                }
                Dialog::Report { path, outcome } => {
                    ui.heading(if outcome.cancelled { "Deletion cancelled" } else { "Some items were not deleted" });
                    ui.add(egui::Label::new(RichText::new(path.display().to_string()).weak()).truncate());
                    ui.label(format!("{} removed, {} failed.", outcome.removed, outcome.failures.len()));
                    if !outcome.failures.is_empty() {
                        ui.label(RichText::new("Files in use and files needing administrator rights cannot be removed here.").weak());
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            for failure in &outcome.failures {
                                ui.add(egui::Label::new(RichText::new(failure.path.display().to_string()).monospace().small()).truncate());
                                ui.add(egui::Label::new(RichText::new(failure.error.to_string()).weak().small()).truncate());
                            }
                        });
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("OK").clicked() {
                            next = Some(None);
                        }
                    });
                }
            }
        });
        if let Some(then) = enable {
            self.app.enable_permanent_delete();
            next = Some(then.map(Dialog::ConfirmDelete));
        }
        if let Some(node) = confirmed_delete {
            if let Err(err) = self.app.delete_node_permanently(node) {
                self.app.message = Some(format!("delete failed: {err}"));
            }
            next = Some(None);
        }
        self.dialog = match next {
            Some(dialog) => dialog,
            // Clicking the backdrop or pressing Escape dismisses without acting.
            None if response.should_close() => None,
            None => Some(dialog),
        };
    }
}
