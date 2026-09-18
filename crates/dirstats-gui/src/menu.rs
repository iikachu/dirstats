// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.


//! The context menu shared by the tree and the treemap, and the rows it is built from.

use dirstats_app::NodeId;
use eframe::egui::{self, Sense};

use crate::icons;

/// What a node's context menu asked for; applied after the menu closes.
#[derive(Clone, Copy, Debug)]
pub(super) enum NodeAction {
    Zoom,
    CopyPath,
    #[cfg(feature = "open")]
    Open,
    #[cfg(feature = "trash")]
    Trash,
    #[cfg(feature = "trash")]
    PutBack,
    /// macOS: drop the local copy of an iCloud item.
    #[cfg(feature = "icloud")]
    Evict,
    /// Windows: delete without the Recycle Bin, after the gate and a confirmation.
    #[cfg(all(windows, feature = "trash"))]
    DeletePermanently,
    /// Windows: open the gate dialog, then delete if it is accepted.
    #[cfg(all(windows, feature = "trash"))]
    EnablePermanentDelete,
}

/// What the platform calls its trash in menu labels.
pub(super) const TRASH_NAME: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };

/// Whether the permanent-delete item is offered and how it reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Permanent {
    /// Not on this platform: only the trash is offered.
    Unavailable,
    /// Offered as "Enable Permanent Delete…", which opens the gate dialog.
    Locked,
    /// Gate passed: offered as "Delete Permanently".
    Enabled,
}

/// Label of the zoom item for `node`: a folder zooms into itself, a file
/// into the folder holding it, and nothing is offered when that is where
/// the view already is.
pub(super) fn zoom_label(tree: &dirstats_app::Tree, current: Option<NodeId>, node: NodeId) -> Option<&'static str> {
    if !tree.children(node).is_empty() {
        return Some("Zoom in");
    }
    let parent = tree.node(node).parent?;
    (Some(parent) != current).then_some("Zoom in to containing folder")
}

/// Menu items for a node: the same in the tree and the treemap. `zoom` is
/// the label of the zoom item, if one is offered. Returns the chosen action.
pub(super) fn node_menu(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    zoom: Option<&str>,
    trashed: TrashState,
    permanent: Permanent,
    cloud: dirstats_app::cloud::CloudStatus,
) -> Option<NodeAction> {
    let mut action = None;
    ui.set_max_width(320.0);
    ui.set_min_width(200.0);
    // Rows touch each other; spacing is added explicitly where wanted.
    ui.spacing_mut().item_spacing.y = 0.0;
    // Header: file name in bold, its folder underneath in weak text, both on
    // one line each and cut with an ellipsis rather than wrapped.
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let parent = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(egui::Label::new(egui::RichText::new(name).strong()).truncate().selectable(false));
            if !parent.is_empty() {
                ui.add(egui::Label::new(egui::RichText::new(parent).weak().small()).truncate().selectable(false));
            }
        });
    });
    menu_separator(ui);
    // Icons only on actions, none on navigation; labels stay aligned either way.
    if let Some(label) = zoom
        && menu_item(ui, None, label, false).clicked()
    {
        action = Some(NodeAction::Zoom);
    }
    if menu_item(ui, Some(icons::Glyph::ContentCopy), "Copy path", false).clicked() {
        action = Some(NodeAction::CopyPath);
    }
    #[cfg(feature = "open")]
    if menu_item(ui, Some(icons::Glyph::OpenInNew), "Open", false).clicked() {
        action = Some(NodeAction::Open);
    }
    #[cfg(feature = "icloud")]
    match cloud {
        dirstats_app::cloud::CloudStatus::Local => {}
        dirstats_app::cloud::CloudStatus::Downloaded => {
            if menu_item(ui, Some(icons::Glyph::CloudOff), "Remove Download", false).clicked() {
                action = Some(NodeAction::Evict);
            }
        }
        dirstats_app::cloud::CloudStatus::Evicted => {
            ui.add_enabled_ui(false, |ui| menu_item(ui, Some(icons::Glyph::CloudOff), "Not Downloaded", false));
        }
    }
    #[cfg(not(feature = "icloud"))]
    let _ = cloud;
    #[cfg(feature = "trash")]
    {
        menu_separator(ui);
        match trashed {
            TrashState::Present => {
                if menu_item(ui, Some(icons::Glyph::Delete), &format!("Move to {TRASH_NAME}"), true).clicked() {
                    action = Some(NodeAction::Trash);
                }
                #[cfg(windows)]
                match permanent {
                    Permanent::Unavailable => {}
                    Permanent::Locked => {
                        if menu_item(ui, None, "Enable Permanent Delete…", false).clicked() {
                            action = Some(NodeAction::EnablePermanentDelete);
                        }
                    }
                    Permanent::Enabled => {
                        if menu_item(ui, None, "Delete Permanently", true).clicked() {
                            action = Some(NodeAction::DeletePermanently);
                        }
                    }
                }
            }
            TrashState::CanPutBack => {
                if menu_item(ui, Some(icons::Glyph::Undo), "Put Back", false).clicked() {
                    action = Some(NodeAction::PutBack);
                }
            }
            TrashState::Trashed => {
                ui.add_enabled_ui(false, |ui| menu_item(ui, Some(icons::Glyph::Delete), &format!("In {TRASH_NAME}"), false));
            }
            TrashState::Deleted => {
                ui.add_enabled_ui(false, |ui| menu_item(ui, Some(icons::Glyph::Delete), "Deleted", false));
            }
        }
    }
    #[cfg(not(all(windows, feature = "trash")))]
    let _ = (trashed, permanent);
    if action.is_some() {
        ui.close();
    }
    action
}

/// Trash state of the node a menu is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TrashState {
    Present,
    /// Trashed, and the app knows where it went.
    CanPutBack,
    /// Trashed, location unknown.
    Trashed,
    /// Deleted permanently (Windows), or under something that was.
    Deleted,
}

/// A menu row: optional leading icon in a fixed slot so labels line up,
/// then the label. `destructive` uses the error colour.
pub(super) fn menu_item(ui: &mut egui::Ui, glyph: Option<icons::Glyph>, label: &str, destructive: bool) -> egui::Response {
    const HEIGHT: f32 = 28.0;
    const PAD: f32 = 10.0;
    const SLOT: f32 = 20.0;
    const GAP: f32 = 10.0;
    let width = ui.available_width().max(180.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), Sense::click());
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
    }
    let color = if destructive { ui.visuals().error_fg_color } else { visuals.text_color() };
    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.min.x + PAD + SLOT / 2.0, rect.center().y), egui::vec2(16.0, 16.0));
    if let Some(glyph) = glyph {
        icons::paint(ui.painter(), icon_rect, glyph, color);
    }
    let text_pos = egui::pos2(rect.min.x + PAD + SLOT + GAP, rect.center().y);
    ui.painter().text(text_pos, egui::Align2::LEFT_CENTER, label, egui::TextStyle::Button.resolve(ui.style()), color);
    response
}

/// Thin rule with even breathing room, for use between menu groups.
pub(super) fn menu_separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, ui.visuals().widgets.noninteractive.bg_stroke);
    ui.add_space(4.0);
}
