// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.


//! The extension legend to the right of the treemap.

use dirstats_core::format;
use eframe::egui::{self, Color32, Sense};

use crate::theme::hover_fill;
use crate::treemap::{PULSE_SECONDS, vivid};
use crate::{Gui, Highlight, Selection};

impl Gui {
    /// Extensions ranked by total size, largest first, in cells aligned to
    /// the header: swatch and name, share, size. Hovering a row highlights
    /// that extension's boxes in the treemap; clicking one selects it.
    pub(super) fn legend(&mut self, ui: &mut egui::Ui, edges: [f32; 4], row_height: f32) {
        let Some(colors) = &self.colors else {
            return;
        };
        let total: u64 = colors.entries().iter().map(|(_, size, _)| size).sum();
        let entries = colors.entries();
        let pad = 6.0;
        let mono = egui::TextStyle::Monospace.resolve(ui.style());
        let mut hovered_extension = None;
        let mut clicked_extension = None;
        let selected_extension = self.selected_extension().cloned();
        let selected_extension = selected_extension.as_ref();
        egui::ScrollArea::vertical().id_salt("legend").auto_shrink([false, false]).show_rows(ui, row_height, entries.len(), |ui, range| {
            for (ext, size, color) in &entries[range] {
                let (row_rect, row) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), Sense::click());
                let hovered = row.hovered();
                let is_selected = selected_extension == Some(ext);
                if hovered {
                    hovered_extension = Some(ext.clone());
                }
                if row.clicked() {
                    clicked_extension = Some(ext.clone());
                }
                if is_selected {
                    ui.painter().rect_filled(row_rect, 0.0, ui.visuals().selection.bg_fill);
                } else if hovered {
                    ui.painter().rect_filled(row_rect, 0.0, hover_fill(ui.visuals()));
                }
                let text = if is_selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
                let (top, bottom) = (row_rect.min.y, row_rect.max.y);
                let cell = |from: f32, to: f32| egui::Rect::from_min_max(egui::pos2(from, top), egui::pos2(to, bottom));

                let name_cell = cell(edges[0], edges[1]);
                // The swatch pulses with the boxes it stands for.
                let swatch_color = if hovered {
                    let t = ui.input(|i| i.time);
                    let phase = (t * std::f64::consts::TAU / PULSE_SECONDS).sin() * 0.5 + 0.5;
                    let k = 0.35 + 0.65 * phase;
                    let peak = vivid(*color);
                    // Same straight-line mix the treemap blend produces.
                    dirstats_core::treemap::Oklch::new(
                        color.l + (peak.l - color.l) * k,
                        color.c + (peak.c - color.c) * k,
                        color.h,
                    )
                } else {
                    *color
                };
                let [r, g, b] = swatch_color.to_srgb();
                let swatch = egui::Rect::from_center_size(egui::pos2(name_cell.min.x + pad + 7.0, name_cell.center().y), egui::vec2(14.0, 14.0));
                ui.painter().with_clip_rect(name_cell).rect_filled(swatch, 3.0, Color32::from_rgb(r, g, b));
                let full = ext.as_deref().map_or("(none)".to_string(), |e| format!(".{e}"));
                let label_rect = egui::Rect::from_min_max(egui::pos2(swatch.max.x + pad, top), egui::pos2(name_cell.max.x - pad, bottom));
                if label_rect.width() > 4.0 {
                    let mut name_ui = ui.new_child(egui::UiBuilder::new().max_rect(label_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    name_ui.set_clip_rect(label_rect.intersect(ui.clip_rect()));
                    name_ui.add(egui::Label::new(&full).truncate().selectable(false)).on_hover_text(&full);
                }
                for (from, to, value) in [
                    (edges[1], edges[2], format!("{:.1}", format::percent(*size, total))),
                    (edges[2], edges[3], format::size(*size)),
                ] {
                    let c = cell(from, to);
                    if c.width() <= 0.0 {
                        continue;
                    }
                    ui.painter().with_clip_rect(c).text(egui::pos2(c.max.x - pad, c.center().y), egui::Align2::RIGHT_CENTER, value, mono.clone(), text);
                }
            }
        });
        if let Some(ext) = hovered_extension
            && self.hover_active
        {
            self.next_highlight = Some(Highlight::Extension(ext));
        }
        if let Some(ext) = clicked_extension {
            self.selection = Some(Selection::Extension(ext));
        }
    }
}
