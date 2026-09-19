// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! What shows before there is a tree: the location picker and scan progress.

use dirstats_core::format;
use eframe::egui::{self, Sense};

use crate::theme::hover_fill;
use crate::{Gui, icons};

impl Gui {
    /// Home, disks and root to choose from, plus a Browse… row for any other
    /// folder, floated over the middle of the whole body when there is no
    /// scan and no tree. Clicking one starts its scan. When Full Disk Access
    /// is known to be missing (macOS), it says what that costs.
    pub(super) fn location_picker(&mut self, ui: &mut egui::Ui) {
        if self.locations.is_none() {
            self.full_disk_access = dirstats_core::locations::full_disk_access();
        }
        let locations = self.locations.get_or_insert_with(dirstats_core::locations::list);
        let rect = ui.available_rect_before_wrap();
        let width = (rect.width() - 2.0 * PAD - 32.0).clamp(200.0, 560.0);
        let row_height = 48.0;
        let height = 44.0
            + if self.app.message.is_some() { 22.0 } else { 0.0 }
            + if self.full_disk_access == Some(false) { 70.0 } else { 0.0 }
            + row_height * (locations.len() + 1) as f32;
        let top = (rect.center().y - height / 2.0).max(rect.min.y + 16.0 + PAD);
        let panel = egui::Rect::from_min_size(egui::pos2(rect.center().x - width / 2.0, top), egui::vec2(width, height));
        // Opaque card under the content so the column guide lines behind
        // it do not show through.
        const PAD: f32 = 20.0;
        let card = panel.expand(PAD);
        ui.painter().rect(card, 8.0, ui.visuals().panel_fill, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Outside);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(panel).layout(egui::Layout::top_down(egui::Align::Min)));
        child.label(egui::RichText::new("Choose a location to scan").heading().strong().size(22.0));
        if let Some(message) = &self.app.message {
            child.add_space(4.0);
            child.add(egui::Label::new(egui::RichText::new(message).color(child.visuals().error_fg_color)).truncate());
        }
        if self.full_disk_access == Some(false) {
            child.add_space(4.0);
            child.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(
                        "Without Full Disk Access some folders are skipped, and macOS asks about Desktop, Documents, Downloads and external disks one dialog at a time; the count pauses until each is answered.",
                    )
                    .color(ui.visuals().weak_text_color()),
                );
                #[cfg(target_os = "macos")]
                if ui.button("Open Privacy Settings…").clicked()
                    && let Err(err) = dirstats_core::locations::open_full_disk_access_settings()
                {
                    self.app.message = Some(format!("could not open System Settings: {err}"));
                }
            });
        }
        child.add_space(10.0);
        let mono = egui::TextStyle::Monospace.resolve(child.style());
        let body = egui::TextStyle::Body.resolve(child.style());
        let small = egui::TextStyle::Small.resolve(child.style());
        let mut chosen = None;
        for location in locations.iter() {
            let (row_rect, row) = child.allocate_exact_size(egui::vec2(width, row_height), Sense::click());
            let painter = child.painter();
            if row.hovered() {
                painter.rect_filled(row_rect, 4.0, hover_fill(child.visuals()));
            }
            let text = child.visuals().text_color();
            let weak = child.visuals().weak_text_color();
            let glyph = match location.kind {
                dirstats_core::locations::Kind::Home => icons::Glyph::Home,
                dirstats_core::locations::Kind::Volume => icons::Glyph::Storage,
                dirstats_core::locations::Kind::Root => icons::Glyph::Folder,
            };
            let icon_rect = egui::Rect::from_center_size(egui::pos2(row_rect.min.x + 22.0, row_rect.center().y), egui::vec2(22.0, 22.0));
            icons::paint(painter, icon_rect, glyph, text);
            let left = row_rect.min.x + 44.0;
            painter.text(egui::pos2(left, row_rect.min.y + 8.0), egui::Align2::LEFT_TOP, &location.name, body.clone(), text);
            painter.text(
                egui::pos2(left, row_rect.max.y - 8.0),
                egui::Align2::LEFT_BOTTOM,
                location.path.display().to_string(),
                small.clone(),
                weak,
            );
            if let (Some(total), Some(free)) = (location.total, location.free) {
                let used = total.saturating_sub(free);
                let right = row_rect.max.x - 10.0;
                painter.text(
                    egui::pos2(right, row_rect.min.y + 8.0),
                    egui::Align2::RIGHT_TOP,
                    format!("{} free of {}", format::size(free), format::size(total)),
                    mono.clone(),
                    weak,
                );
                // Fill bar for used space, like a Finder or Explorer drive row.
                let bar =
                    egui::Rect::from_min_max(egui::pos2(right - 160.0, row_rect.max.y - 16.0), egui::pos2(right, row_rect.max.y - 10.0));
                painter.rect_filled(bar, 2.0, child.visuals().faint_bg_color);
                let mut filled = bar;
                filled.set_width(bar.width() * (format::percent(used, total) / 100.0) as f32);
                painter.rect_filled(filled, 2.0, weak);
            }
            painter.hline(row_rect.x_range(), row_rect.max.y, egui::Stroke::new(1.0_f32, child.visuals().faint_bg_color));
            if row.clicked() {
                chosen = Some(location.path.clone());
            }
        }
        // Last row: a native folder dialog for scanning anywhere else.
        let (row_rect, row) = child.allocate_exact_size(egui::vec2(width, row_height), Sense::click());
        let painter = child.painter();
        if row.hovered() {
            painter.rect_filled(row_rect, 4.0, hover_fill(child.visuals()));
        }
        let text = child.visuals().text_color();
        let weak = child.visuals().weak_text_color();
        let icon_rect = egui::Rect::from_center_size(egui::pos2(row_rect.min.x + 22.0, row_rect.center().y), egui::vec2(22.0, 22.0));
        icons::paint(painter, icon_rect, icons::Glyph::FolderOpen, text);
        let left = row_rect.min.x + 44.0;
        painter.text(egui::pos2(left, row_rect.min.y + 8.0), egui::Align2::LEFT_TOP, "Browse…", body, text);
        painter.text(egui::pos2(left, row_rect.max.y - 8.0), egui::Align2::LEFT_BOTTOM, "Choose any folder to scan", small, weak);
        if row.clicked() {
            chosen = rfd::FileDialog::new().set_title("Choose a folder to scan").pick_folder();
        }
        if let Some(path) = chosen {
            self.app.start_scan(path);
            self.locations = None;
        }
    }

    /// Centred scan status shown in the treemap area until a tree arrives.
    /// Before any scan the location picker overlays the whole body instead.
    pub(super) fn scan_progress(&mut self, ui: &mut egui::Ui) {
        if self.app.scan.is_none() && self.app.tree.is_none() {
            return;
        }
        let rect = ui.available_rect_before_wrap();
        let (title, root, detail) = match &self.app.scan {
            Some(scan) => (
                "Scanning".to_string(),
                scan.root.display().to_string(),
                format!("{} entries · {} skipped · {:.1}s", scan.entries(), scan.errors(), scan.started.elapsed().as_secs_f64()),
            ),
            None => ("No scan".to_string(), String::new(), self.app.message.clone().unwrap_or_default()),
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Center)));
        child.add_space((rect.height() / 2.0 - 40.0).max(0.0));
        if self.app.scan.is_some() {
            child.add(egui::Spinner::new().size(28.0));
            child.add_space(8.0);
        }
        child.label(egui::RichText::new(title).heading().strong().size(28.0));
        if !root.is_empty() {
            child.add_space(2.0);
            child.add(egui::Label::new(egui::RichText::new(root).strong().size(18.0)).truncate());
        }
        child.add_space(4.0);
        child.label(egui::RichText::new(detail).strong().size(16.0).color(child.visuals().weak_text_color()));
        if self.app.scan.is_some() && self.full_disk_access == Some(false) {
            child.add_space(12.0);
            child.label(
                egui::RichText::new(
                    "If the count stops, macOS is probably asking for permission in a dialog, possibly behind this window.",
                )
                .color(child.visuals().weak_text_color()),
            );
        }
    }
}
