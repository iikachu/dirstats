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
use crate::icons::{self, Glyph};
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

/// Width of every dialog: a compact utility panel, not a document.
#[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
const WIDTH: f32 = 380.0;

/// Corner radius of a dialog, small like the system's own alerts.
#[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
const RADIUS: u8 = 4;

/// The frame of a utility dialog: no margin of its own, so the body and the
/// footer band (see [`buttons`]) can each span the full width.
#[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
fn frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::window(&ctx.style()).inner_margin(0).corner_radius(RADIUS)
}

/// An alert body: `glyph` on the left in `tint`, then a bold title in body
/// size and whatever `body` adds, all in one tight column.
#[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
fn alert(ui: &mut egui::Ui, glyph: Glyph, tint: egui::Color32, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 16, bottom: 14 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
            icons::paint(ui.painter(), rect, glyph, tint);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(egui::RichText::new(title).strong());
                body(ui);
            });
        });
    });
}

/// A two-column table of facts ("Path", "Size"), keys dimmed, values
/// selectable so a path can be copied.
#[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
fn facts(ui: &mut egui::Ui, id: &str, rows: &[(&str, String)]) {
    egui::Grid::new(id).num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
        for (key, value) in rows {
            ui.label(egui::RichText::new(*key).weak().small());
            ui.add(egui::Label::new(egui::RichText::new(value).monospace().small()).wrap().selectable(true));
            ui.end_row();
        }
    });
}

/// The button row in a tinted footer band across the bottom of the dialog,
/// right-aligned in reading order (the last is the default action), as in
/// Windows and GTK message boxes. Returns which button was clicked.
#[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
fn buttons(ui: &mut egui::Ui, labels: &[(&str, Style)]) -> Option<usize> {
    let mut clicked = None;
    let visuals = ui.visuals().clone();
    let corners = egui::CornerRadius { nw: 0, ne: 0, sw: RADIUS, se: RADIUS };
    egui::Frame::new().fill(visuals.faint_bg_color).corner_radius(corners).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let size = ui.style().text_styles[&egui::TextStyle::Body].size;
        let height = ui.spacing().interact_size.y + 2.0;
        // A fixed-height row: a bare right-to-left layout would take all the
        // height left in the modal and centre the buttons in it.
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.spacing_mut().button_padding = egui::vec2(12.0, 3.0);
            for (i, (label, style)) in labels.iter().enumerate().rev() {
                let text = egui::RichText::new(*label).size(size);
                let button = match style {
                    Style::Plain => egui::Button::new(text),
                    Style::Default => egui::Button::new(text.strong()),
                    Style::Destructive => egui::Button::new(text.color(egui::Color32::WHITE)).fill(visuals.error_fg_color),
                };
                if ui.add(button.min_size(egui::vec2(80.0, height))).clicked() {
                    clicked = Some(i);
                }
            }
        });
    });
    clicked
}

/// How a dialog button looks.
#[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
#[derive(Clone, Copy)]
enum Style {
    Plain,
    Default,
    Destructive,
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

        // A clean deletion only needs the footer message; anything else gets a report.
        if let Some((path, outcome)) = self.app.poll_delete()
            && (!outcome.failures.is_empty() || outcome.cancelled)
        {
            self.dialog = Some(Dialog::Report { path, outcome });
        }

        let warn = ctx.style().visuals.warn_fg_color;
        let error = ctx.style().visuals.error_fg_color;

        if let Some(running) = &self.app.delete {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            let (done, total) = (running.done(), running.total.max(1));
            let path = running.path.display().to_string();
            let mut cancel = false;
            Modal::new(egui::Id::new("delete-progress")).frame(frame(ctx)).show(ctx, |ui| {
                ui.set_width(WIDTH);
                alert(ui, Glyph::DeleteForever, error, "Deleting permanently", |ui| {
                    ui.add(egui::Label::new(RichText::new(path).monospace().small().weak()).truncate());
                    ui.add(egui::ProgressBar::new(done as f32 / total as f32).desired_height(6.0));
                    ui.label(RichText::new(format!("{done} of {total} items")).small().weak());
                });
                cancel = buttons(ui, &[("Cancel", Style::Plain)]).is_some();
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
        let response = Modal::new(egui::Id::new("dialog")).frame(frame(ctx)).show(ctx, |ui| {
            ui.set_width(WIDTH);
            match &dialog {
                Dialog::EnablePermanent { then } => {
                    alert(ui, Glyph::DeleteForever, warn, "Enable permanent delete?", |ui| {
                        ui.label(format!(
                            "Items deleted this way skip the {TRASH_NAME} and cannot be recovered. \
                             Use it for {WHEN}. Each deletion asks first.",
                        ));
                    });
                    match buttons(ui, &[("Cancel", Style::Plain), ("Enable", Style::Default)]) {
                        Some(0) => next = Some(None),
                        Some(_) => enable = Some(*then),
                        None => {}
                    }
                }
                Dialog::ConfirmDelete(node) => {
                    let path = self.app.path_of(*node).unwrap_or_default();
                    let tree = self.app.tree.as_ref();
                    let is_dir = tree.is_some_and(|t| !t.children(*node).is_empty());
                    let size = tree.map(|t| format::size(t.size(*node))).unwrap_or_default();
                    let count = tree.map(|t| t.node(*node).file_count).unwrap_or_default();
                    let title = if is_dir { "Delete this folder permanently?" } else { "Delete this file permanently?" };
                    alert(ui, Glyph::DeleteForever, error, title, |ui| {
                        let mut rows = vec![("Path", path.display().to_string()), ("Size", size)];
                        if is_dir {
                            rows.push(("Files", count.to_string()));
                        }
                        facts(ui, "delete-facts", &rows);
                        ui.label(
                            RichText::new(format!("Skips the {TRASH_NAME}; this cannot be undone. \
                                            Links are removed, not what they point at."))
                                .small()
                                .weak(),
                        );
                    });
                    match buttons(ui, &[("Cancel", Style::Plain), ("Delete Permanently", Style::Destructive)]) {
                        Some(0) => next = Some(None),
                        Some(_) => confirmed_delete = Some(*node),
                        None => {}
                    }
                }
                #[cfg(feature = "trash")]
                Dialog::TrashFailed { node, error: reason } => {
                    let path = self.app.path_of(*node).unwrap_or_default();
                    alert(ui, Glyph::DeleteForever, warn, &format!("Couldn't move to the {TRASH_NAME}"), |ui| {
                        ui.label(TRASH_REFUSED);
                        facts(ui, "trash-facts", &[("Path", path.display().to_string()), ("Error", reason.clone())]);
                    });
                    let enabled = self.app.permanent_delete();
                    let label = if enabled { "Delete Permanently…" } else { "Enable Permanent Delete…" };
                    match buttons(ui, &[("Cancel", Style::Plain), (label, Style::Default)]) {
                        Some(0) => next = Some(None),
                        Some(_) => {
                            next = Some(Some(if enabled {
                                Dialog::ConfirmDelete(*node)
                            } else {
                                Dialog::EnablePermanent { then: Some(*node) }
                            }))
                        }
                        None => {}
                    }
                }
                Dialog::Report { path, outcome } => {
                    let title = if outcome.cancelled { "Deletion cancelled" } else { "Some items were not deleted" };
                    alert(ui, Glyph::DeleteForever, warn, title, |ui| {
                        facts(ui, "report-facts", &[
                            ("Path", path.display().to_string()),
                            ("Removed", outcome.removed.to_string()),
                            ("Failed", outcome.failures.len().to_string()),
                        ]);
                        if !outcome.failures.is_empty() {
                        ui.label(RichText::new("Files in use and files needing administrator rights cannot be removed here.").small().weak());
                        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(180.0).auto_shrink([false, true]).show(ui, |ui| {
                                egui::Grid::new("report-failures").num_columns(2).striped(true).spacing([10.0, 1.0]).show(ui, |ui| {
                                    for failure in &outcome.failures {
                                        ui.add(egui::Label::new(RichText::new(failure.path.display().to_string()).monospace().small()).truncate());
                                        ui.add(egui::Label::new(RichText::new(failure.error.to_string()).weak().small()).truncate());
                                        ui.end_row();
                                    }
                                });
                            });
                        });
                        }
                    });
                    if buttons(ui, &[("OK", Style::Default)]).is_some() {
                        next = Some(None);
                    }
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
