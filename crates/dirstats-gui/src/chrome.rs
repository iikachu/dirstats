// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.


//! Toolbar above the columns and status footer below them.

use dirstats_app::format;
use dirstats_app::treemap::Style;
use eframe::egui::{self, Sense};

use crate::theme::disabled_icon;
use crate::{Gui, icons};

impl Gui {
    /// Toolbar: back, breadcrumbs, totals; layout toggle and rescan on the right.
    pub(super) fn header(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.x = 6.0;
        let icon_button = |ui: &mut egui::Ui, glyph: icons::Glyph, enabled: bool, tip: &str| -> egui::Response {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), if enabled { Sense::click() } else { Sense::hover() });
            let visuals = ui.style().interact(&response);
            if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
                ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
            }
            let color = if enabled { visuals.text_color() } else { disabled_icon(ui.visuals()) };
            icons::paint(ui.painter(), rect.shrink(4.0), glyph, color);
            if enabled { response.on_hover_text(tip) } else { response }
        };

        ui.horizontal(|ui| {
            ui.set_height(26.0);
            if let Some(scan) = &self.app.scan {
                ui.add(egui::Label::new(egui::RichText::new(scan.root.display().to_string()).strong().size(15.0)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        self.app.cancel_scan();
                    }
                });
                return;
            }
            let Some(tree) = &self.app.tree else {
                ui.label(egui::RichText::new(crate::APP_NAME).strong().size(15.0));
                return;
            };

            // Right cluster first so the crumbs can take the rest of the width.
            let mut zoom_target = None;
            let mut rescan = false;
            let mut style = self.style;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                rescan = ui.button("Rescan").clicked();
                ui.add_space(6.0);
                // Segmented layout toggle.
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, (value, label)) in [(Style::Rows, "Rows"), (Style::Squarified, "Squarified")].iter().enumerate() {
                    let selected = style == *value;
                    let button = egui::Button::new(egui::RichText::new(*label).strong()).selected(selected).corner_radius(if i == 0 {
                        egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 }
                    } else {
                        egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }
                    });
                    if ui.add(button).clicked() {
                        style = *value;
                    }
                }
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.add_space(10.0);
                if let Some(dir) = self.app.dir() {
                    ui.label(
                        egui::RichText::new(format!("{}   {} files", format::size(tree.size(dir)), tree.node(dir).file_count))
                            .monospace()
                            .color(ui.visuals().weak_text_color()),
                    );
                }
                ui.add_space(6.0);

                // Left cluster: back button and breadcrumbs, truncating from the right.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let can_back = self.app.can_back();
                    if icon_button(ui, icons::Glyph::ChevronLeft, can_back, "Back (Backspace)").clicked() {
                        zoom_target = Some(None);
                    }
                    // Crumbs joined by slashes, as the path is written. Each
                    // lights up under the pointer with a pill; ancestors are
                    // links, with an underline, a hand and a darker pill while
                    // pressed. The current folder's pill is fainter and a click
                    // on it does nothing.
                    let crumbs = self.app.breadcrumbs();
                    let text = ui.visuals().text_color();
                    let mut separated = true;
                    for (i, &id) in crumbs.iter().enumerate() {
                        let last = i + 1 == crumbs.len();
                        // A root that is itself a separator, or ends in one,
                        // needs no second one after it.
                        if !separated {
                            ui.add(
                                egui::Label::new(egui::RichText::new(std::path::MAIN_SEPARATOR_STR).size(15.0).color(ui.visuals().weak_text_color()))
                                    .selectable(false),
                            );
                        }
                        let name = tree.node(id).name.to_string_lossy().into_owned();
                        separated = name.ends_with(std::path::MAIN_SEPARATOR);
                        // Held back so the pill can go beneath the text.
                        let pill = ui.painter().add(egui::Shape::Noop);
                        let rich = egui::RichText::new(name).size(15.0);
                        let rich = if last { rich.strong() } else { rich.color(text) };
                        let sense = if last { Sense::hover() } else { Sense::click() };
                        let crumb = ui.add(egui::Label::new(rich).sense(sense).selectable(false).truncate());
                        if crumb.hovered() {
                            let visuals = ui.visuals();
                            let strength = if last {
                                0.07
                            } else if crumb.is_pointer_button_down_on() {
                                0.28
                            } else {
                                0.14
                            };
                            let fill = visuals.panel_fill.lerp_to_gamma(visuals.text_color(), strength);
                            ui.painter().set(pill, egui::Shape::rect_filled(crumb.rect.expand2(egui::vec2(4.0, 2.0)), 4.0, fill));
                            if !last {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                ui.painter().hline(crumb.rect.x_range(), crumb.rect.max.y - 1.0, egui::Stroke::new(1.0_f32, text));
                            }
                        }
                        if !last && crumb.clicked() {
                            zoom_target = Some(Some(id));
                        }
                    }
                });
            });

            if style != self.style {
                self.style = style;
                self.map_key = None;
            }
            if rescan {
                self.app.rescan();
            }
            let moved = match zoom_target {
                Some(None) => self.app.back(),
                Some(Some(id)) => self.app.zoom_to(id),
                None => false,
            };
            if moved {
                self.map_key = None;
            }
        });
    }

    /// One fixed-height line: the hovered path, or the last message.
    pub(super) fn footer(&mut self, ui: &mut egui::Ui) {
        const HOLD: std::time::Duration = std::time::Duration::from_secs(4);
        // Track when the message last changed.
        match (&self.app.message, &self.message_since) {
            (Some(m), Some((_, seen))) if m == seen => {}
            (Some(m), _) => self.message_since = Some((std::time::Instant::now(), m.clone())),
            (None, _) => self.message_since = None,
        }
        let is_failure = self.app.message.as_deref().is_some_and(|m| m.contains("failed"));
        let fresh = self.message_since.as_ref().is_some_and(|(at, _)| at.elapsed() < HOLD);
        if fresh {
            ui.ctx().request_repaint_after(HOLD);
        }
        ui.horizontal(|ui| {
            ui.set_height(ui.text_style_height(&egui::TextStyle::Monospace));
            let hovered = self.app.tree.as_ref().zip(self.app.hovered);
            match (&self.app.message, hovered) {
                // A failure, or any fresh message, outranks the hover path.
                (Some(message), _) if is_failure || fresh => {
                    let text = egui::RichText::new(message);
                    ui.label(if is_failure { text.color(ui.visuals().error_fg_color).strong() } else { text });
                }
                (_, Some((tree, hovered))) => {
                    ui.monospace(format!("{:>10}  {}", format::size(tree.size(hovered)), tree.path(hovered).display()));
                }
                (Some(message), None) => {
                    ui.label(message);
                }
                (None, None) => {
                    ui.label(" ");
                }
            }
        });
    }
}
