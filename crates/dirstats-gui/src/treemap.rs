// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! The treemap pane: cached base render, the pulsing highlight overlay and pointer handling.

use dirstats_core::treemap::TreemapOptions;
use dirstats_core::treemap::render::{ExtensionColors, render};
use eframe::egui::{self, Color32, ColorImage, Sense, TextureOptions};

use crate::menu::{node_menu, zoom_label};
use crate::{Gui, Highlight, Selection};

/// Period of the highlight pulse, in seconds.
pub(super) const PULSE_SECONDS: f64 = 2.4;

/// How far toward the sRGB gamut edge a highlighted colour's chroma moves.
/// Relative to the edge rather than a fixed factor, so every hue gets a
/// comparable perceived boost.
pub(super) const HIGHLIGHT_TOWARD_MAX: f64 = 0.7;
/// Lightness added to highlighted boxes.
pub(super) const HIGHLIGHT_LIGHTNESS: f64 = 0.06;

/// The vivid version of a colour used for hover and the legend swatch.
pub(super) fn vivid(color: dirstats_core::treemap::Oklch) -> dirstats_core::treemap::Oklch {
    color.lighten(HIGHLIGHT_LIGHTNESS).toward_max_chroma(HIGHLIGHT_TOWARD_MAX)
}

impl Gui {
    /// Re-render the treemap when the directory, size or tree changed, or
    /// when `map_key` was cleared (a layout style change or zoom), then bring
    /// the highlight overlay up to date with what is hovered.
    pub(super) fn ensure_map(&mut self, ctx: &egui::Context, width: u32, height: u32) {
        let (Some(tree), Some(dir), Some(colors)) = (&self.app.tree, self.app.dir(), &self.colors) else {
            return;
        };
        let key = (self.tree_version, dir, width, height);
        if self.map_key != Some(key) {
            let options = TreemapOptions { style: self.style, ..Default::default() };
            let map = render(tree, dir, width, height, &options, |t, id| colors.color(t, id));
            let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &map.pixels);
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture("treemap", image, TextureOptions::LINEAR)),
            }
            self.map = Some(map);
            self.map_key = Some(key);
            // Fresh, fully transparent overlay at the new size.
            let clear =
                ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &vec![0; width as usize * height as usize * 4]);
            match &mut self.highlight {
                Some(texture) => texture.set(clear, TextureOptions::LINEAR),
                None => self.highlight = Some(ctx.load_texture("treemap-highlight", clear, TextureOptions::LINEAR)),
            }
            self.highlight_bounds = None;
            self.highlight_key = None;
        }

        // Highlight overlay: updated once per hovered target, pulsed at draw
        // time. Only the hovered leaves are re-shaded, with the same cushion
        // surfaces as the base render, and only the region that changed is
        // uploaded, so the cost follows the highlighted area, not the map.
        let target = self.hovered_highlight.clone();
        let highlight_key = target.as_ref().map(|t| (key, t.clone()));
        if self.highlight_key == highlight_key && (highlight_key.is_some() || self.highlight_bounds.is_none()) {
            return;
        }
        let (Some(map), Some(texture)) = (&self.map, &mut self.highlight) else { return };
        let leaves: Vec<(usize, dirstats_core::treemap::Oklch)> = match &target {
            None => Vec::new(),
            Some(Highlight::Subtree(root)) => {
                let start = map.item_index(*root).unwrap_or(map.items.len());
                let run = map.subtree(*root);
                (start..start + run.len())
                    .filter(|&i| map.items[i].leaf)
                    .map(|i| (i, vivid(colors.color(tree, map.items[i].node))))
                    .collect()
            }
            Some(Highlight::Extension(ext)) => map
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.leaf && *ext == ExtensionColors::extension(tree.node(item.node)))
                .map(|(i, item)| (i, vivid(colors.color(tree, item.node))))
                .collect(),
        };
        // Upload one region covering both what was lit and what will be.
        let union = |a: dirstats_core::treemap::Rect, b: dirstats_core::treemap::Rect| {
            dirstats_core::treemap::Rect::new(a.left.min(b.left), a.top.min(b.top), a.right.max(b.right), a.bottom.max(b.bottom))
        };
        let mut bounds = self.highlight_bounds;
        for &(i, _) in &leaves {
            let r = map.items[i].rect;
            if !r.is_empty() {
                bounds = Some(bounds.map_or(r, |b| union(b, r)));
            }
        }
        if let Some(region) = bounds {
            let options = TreemapOptions { style: self.style, ..Default::default() };
            let pixels = map.shade_leaves(&options, region, leaves.iter().copied());
            let image = ColorImage::from_rgba_unmultiplied([region.width() as usize, region.height() as usize], &pixels);
            texture.set_partial([region.left as usize, region.top as usize], image, TextureOptions::LINEAR);
        }
        // Remember only the region that now holds pixels.
        self.highlight_bounds = leaves.iter().map(|&(i, _)| map.items[i].rect).filter(|r| !r.is_empty()).reduce(union);
        self.highlight_key = highlight_key;
    }

    /// The treemap pane, filling `ui`: the base render, the pulsing
    /// highlight, hollowed-out trashed and evicted boxes and selection
    /// outlines. Click selects, double-click zooms to the enclosing folder
    /// and right-click opens the context menu. Until there is a render it
    /// shows the scan progress instead.
    pub(super) fn treemap(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (width, height) = (available.x.max(1.0) as u32, available.y.max(1.0) as u32);
        self.ensure_map(ui.ctx(), width, height);
        let (Some(texture), Some(map)) = (&self.texture, &self.map) else {
            self.scan_progress(ui);
            return;
        };
        let response = ui.add(egui::Image::new((texture.id(), available)).sense(Sense::click()));
        let origin = response.rect.min;
        let painter = ui.painter_at(response.rect);
        let to_screen = |r: dirstats_core::treemap::Rect| {
            egui::Rect::from_min_max(origin + egui::vec2(r.left as f32, r.top as f32), origin + egui::vec2(r.right as f32, r.bottom as f32))
        };
        // Pulse the vivid overlay over the base while something is hovered.
        if self.highlight_bounds.is_some()
            && let Some(highlight) = &self.highlight
        {
            let t = ui.input(|i| i.time);
            let phase = (t * std::f64::consts::TAU / PULSE_SECONDS).sin() * 0.5 + 0.5;
            let alpha = (0.35 + 0.65 * phase) as f32;
            ui.painter().image(
                highlight.id(),
                response.rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE.gamma_multiply(alpha),
            );
            ui.ctx().request_repaint();
        }

        let hovered = response.hover_pos().and_then(|pos| {
            let (x, y) = ((pos.x - origin.x) as i32, (pos.y - origin.y) as i32);
            map.hit_test(x, y)
        });
        if response.hovered() {
            self.app.hovered = hovered;
            if let Some(node) = hovered
                && self.hover_active
            {
                self.next_highlight = Some(Highlight::Subtree(node));
            }
        }
        // Trashed and evicted boxes are hollowed out: panel fill with a faint
        // outline, so the space they took is visible but empty until the
        // next rescan.
        if !self.app.trashed.is_empty() || !self.app.evicted.is_empty() {
            let fill = ui.visuals().panel_fill;
            let edge = egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color);
            for item in map.items.iter().filter(|item| item.leaf) {
                if self.app.is_trashed(item.node) || self.app.is_evicted(item.node) {
                    let r = to_screen(item.rect);
                    painter.rect_filled(r, 0.0, fill);
                    painter.rect_stroke(r, 0.0, edge, egui::StrokeKind::Inside);
                }
            }
        }
        let outline = |item: &dirstats_core::treemap::render::VisibleItem, color: Color32, width: f32| {
            painter.rect_stroke(to_screen(item.rect), 0.0, egui::Stroke::new(width, color), egui::StrokeKind::Inside);
        };
        match &self.selection {
            Some(Selection::Node(selected)) => {
                if let Some(item) = map.item(*selected) {
                    outline(item, Color32::WHITE, 2.0);
                }
            }
            Some(Selection::Extension(ext)) => {
                if let Some(tree) = &self.app.tree {
                    for item in map.items.iter().filter(|item| item.leaf) {
                        let node = tree.node(item.node);
                        if node.kind != dirstats_core::scan::Kind::Directory && ExtensionColors::extension(node) == *ext {
                            outline(item, Color32::WHITE, 2.0);
                        }
                    }
                }
            }
            None => {}
        }

        if let Some(node) = hovered {
            if response.double_clicked() {
                // Cells are files, so this zooms to the folder holding one;
                // the first click of the pair has already selected it.
                self.zoom(node);
            } else if response.clicked() {
                self.select(node);
            }
            if response.secondary_clicked() {
                self.menu_node = Some(node);
                self.select(node);
            }
        }
        // The menu is drawn every frame from the pinned node, not from hover.
        if let Some(node) = self.menu_node {
            let (path, zoom) = match &self.app.tree {
                Some(tree) => (tree.path(node), zoom_label(tree, self.app.dir(), node)),
                None => (std::path::PathBuf::new(), None),
            };
            let trash_state = self.trash_state(node);
            let permanent = self.permanent();
            let mut action = None;
            let cloud = self.cloud_status(node);
            let menu = response.context_menu(|ui| action = node_menu(ui, &path, zoom, trash_state, permanent, cloud));
            if menu.is_none() {
                self.menu_node = None;
            }
            if let Some(action) = action {
                self.menu_node = None;
                self.apply(node, action);
            }
        }
    }
}
