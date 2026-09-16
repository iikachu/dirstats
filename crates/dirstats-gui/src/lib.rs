// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Graphical front end: an entry list, a glow treemap and an extension
//! legend, driven entirely by [`dirstats_app::App`].

use dirstats_app::{App, NodeId, format};
use dirstats_treemap::render::{ExtensionColors, render};
use dirstats_treemap::{Style, Treemap, TreemapOptions};
use eframe::egui::{self, Color32, ColorImage, Key, Sense, TextureHandle, TextureOptions};

/// Open the window and run until it is closed.
pub fn run(app: App) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_title("dirstats"),
        ..Default::default()
    };
    eframe::run_native("dirstats", options, Box::new(|_| Ok(Box::new(Gui::new(app)))))
}

struct Gui {
    app: App,
    colors: Option<ExtensionColors>,
    /// Bumped whenever a new tree arrives so cached renders are invalidated.
    tree_version: u64,
    map: Option<Treemap>,
    map_key: Option<(u64, NodeId, u32, u32)>,
    texture: Option<TextureHandle>,
    style: Style,
}

impl Gui {
    fn new(app: App) -> Self {
        Self {
            app,
            colors: None,
            tree_version: 0,
            map: None,
            map_key: None,
            texture: None,
            style: Style::Squarified,
        }
    }

    fn tree_changed(&mut self) {
        self.tree_version += 1;
        self.colors = self.app.tree.as_ref().map(ExtensionColors::rank);
        self.map = None;
        self.map_key = None;
    }

    /// Re-render the treemap when the directory, size or tree changed.
    fn ensure_map(&mut self, ctx: &egui::Context, width: u32, height: u32) {
        let (Some(tree), Some(dir), Some(colors)) = (&self.app.tree, self.app.dir(), &self.colors) else {
            return;
        };
        let key = (self.tree_version, dir, width, height);
        if self.map_key == Some(key) {
            return;
        }
        let options = TreemapOptions { style: self.style, ..Default::default() };
        let map = render(tree, dir, width, height, &options, |t, id| colors.color(t, id));
        let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &map.pixels);
        match &mut self.texture {
            Some(texture) => texture.set(image, TextureOptions::LINEAR),
            None => self.texture = Some(ctx.load_texture("treemap", image, TextureOptions::LINEAR)),
        }
        self.map = Some(map);
        self.map_key = Some(key);
    }
}

impl eframe::App for Gui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.app.poll() {
            self.tree_changed();
        }
        if self.app.is_scanning() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if ctx.input(|i| i.key_pressed(Key::Backspace)) && self.app.back() {
            self.map_key = None;
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) && self.app.enter() {
            self.map_key = None;
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| self.footer(ui));
        egui::SidePanel::left("entries").default_width(420.0).show(ctx, |ui| self.entry_list(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| self.treemap(ui));
    }
}

impl Gui {
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(scan) = &self.app.scan {
                ui.spinner();
                ui.label(format!("scanning {}: {} entries, {} errors", scan.root.display(), scan.entries(), scan.errors()));
                if ui.button("Cancel").clicked() {
                    self.app.cancel_scan();
                }
                return;
            }
            let Some(tree) = &self.app.tree else {
                ui.label("no scan");
                return;
            };
            let crumbs = self.app.breadcrumbs();
            let mut target = None;
            for (i, &id) in crumbs.iter().enumerate() {
                if i > 0 {
                    ui.label("›");
                }
                let name = tree.node(id).name.to_string_lossy().into_owned();
                if i + 1 == crumbs.len() {
                    ui.strong(name);
                } else if ui.link(name).clicked() {
                    target = Some(id);
                }
            }
            if let Some(dir) = crumbs.last() {
                ui.separator();
                ui.label(format!("{}  ·  {} files", format::size(tree.size(*dir)), tree.node(*dir).file_count));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Rescan").clicked() {
                    self.app.rescan();
                }
                egui::ComboBox::from_id_salt("layout").selected_text(format!("{:?}", self.style)).show_ui(ui, |ui| {
                    for style in [Style::Squarified, Style::Rows] {
                        if ui.selectable_value(&mut self.style, style, format!("{style:?}")).changed() {
                            self.map_key = None;
                        }
                    }
                });
            });
            if let Some(id) = target
                && self.app.zoom_to(id)
            {
                self.map_key = None;
            }
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if let Some(message) = &self.app.message {
                ui.label(message);
                ui.separator();
            }
            if let (Some(tree), Some(hovered)) = (&self.app.tree, self.app.hovered) {
                ui.monospace(format!("{}  {}", format::size(tree.size(hovered)), tree.path(hovered).display()));
                ui.separator();
            }
            if let Some(colors) = &self.colors {
                for (ext, total, color) in colors.entries().iter().take(14) {
                    let [r, g, b] = color.to_srgb();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
                    let label = ext.as_deref().map_or("(none)".to_string(), |e| format!(".{e}"));
                    ui.label(format!("{label} {}", format::size(*total)));
                }
            }
        });
    }

    fn entry_list(&mut self, ui: &mut egui::Ui) {
        let Some(tree) = &self.app.tree else {
            ui.label("waiting for scan…");
            return;
        };
        let Some(dir) = self.app.dir() else { return };
        let total = tree.size(dir);
        let entries: Vec<NodeId> = self.app.entries().to_vec();
        let selected = self.app.selected();
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        let mut select = None;
        let mut enter = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_height, entries.len(), |ui, range| {
            for &id in &entries[range] {
                let node = tree.node(id);
                let size = tree.size(id);
                let share = format::percent(size, total);
                let mut name = node.name.to_string_lossy().into_owned();
                if !tree.children(id).is_empty() {
                    name.push('/');
                }
                let response = ui.horizontal(|ui| {
                    let is_selected = selected == Some(id);
                    let response = ui.selectable_label(is_selected, format!("{:>10}", format::size(size)));
                    let (bar, _) = ui.allocate_exact_size(egui::vec2(80.0, row_height - 8.0), Sense::hover());
                    ui.painter().rect_filled(bar, 2.0, ui.visuals().faint_bg_color);
                    let mut filled = bar;
                    filled.set_width(bar.width() * (share / 100.0) as f32);
                    ui.painter().rect_filled(filled, 2.0, ui.visuals().selection.bg_fill);
                    ui.label(format!("{share:>5.1}%"));
                    ui.label(name);
                    if node.error {
                        ui.colored_label(Color32::RED, "!");
                    }
                    response
                });
                let row = response.response.union(response.inner);
                if row.clicked() {
                    select = Some(id);
                }
                if row.double_clicked() {
                    enter = Some(id);
                }
                if row.hovered() {
                    self.app.hovered = Some(id);
                }
            }
        });
        if let Some(id) = select {
            self.app.select(id);
        }
        if let Some(id) = enter
            && self.app.zoom_to(id)
        {
            self.map_key = None;
        }
    }

    fn treemap(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (width, height) = (available.x.max(1.0) as u32, available.y.max(1.0) as u32);
        self.ensure_map(ui.ctx(), width, height);
        let (Some(texture), Some(map)) = (&self.texture, &self.map) else {
            ui.centered_and_justified(|ui| ui.label("waiting for scan…"));
            return;
        };
        let response = ui.add(egui::Image::new((texture.id(), available)).sense(Sense::click()));
        let origin = response.rect.min;
        let painter = ui.painter_at(response.rect);

        let hovered = response.hover_pos().and_then(|pos| {
            let (x, y) = ((pos.x - origin.x) as i32, (pos.y - origin.y) as i32);
            map.hit_test(x, y)
        });
        if response.hovered() {
            self.app.hovered = hovered;
        }
        let outline = |node: NodeId, color: Color32, width: f32| {
            if let Some(item) = map.item(node) {
                let r = item.rect;
                let rect = egui::Rect::from_min_max(
                    origin + egui::vec2(r.left as f32, r.top as f32),
                    origin + egui::vec2(r.right as f32, r.bottom as f32),
                );
                painter.rect_stroke(rect, 0.0, egui::Stroke::new(width, color), egui::StrokeKind::Inside);
            }
        };
        if let Some(selected) = self.app.selected() {
            outline(selected, Color32::WHITE, 2.0);
        }
        if let Some(node) = hovered {
            outline(node, Color32::from_white_alpha(160), 1.0);
        }

        let mut zoom = None;
        if let Some(node) = hovered {
            if response.double_clicked() {
                zoom = Some(node);
            } else if response.clicked() {
                self.app.reveal(node);
            }
            response.context_menu(|ui| {
                let path = self.app.path_of(node);
                ui.label(path.as_ref().map_or(String::new(), |p| p.display().to_string()));
                ui.separator();
                if ui.button("Zoom in").clicked() {
                    zoom = Some(node);
                    ui.close();
                }
                #[cfg(feature = "open")]
                if ui.button("Open").clicked() {
                    self.app.reveal(node);
                    if let Err(err) = self.app.open_selected() {
                        self.app.message = Some(format!("open failed: {err}"));
                    }
                    ui.close();
                }
                #[cfg(feature = "trash")]
                if ui.button("Move to trash").clicked() {
                    self.app.reveal(node);
                    if let Err(err) = self.app.trash_selected() {
                        self.app.message = Some(format!("trash failed: {err}"));
                    }
                    ui.close();
                }
            });
        }
        if let Some(node) = zoom {
            // Zoom into the deepest directory containing the clicked leaf.
            let target = if self.app.tree.as_ref().is_some_and(|t| !t.children(node).is_empty()) {
                Some(node)
            } else {
                self.app.tree.as_ref().and_then(|t| t.node(node).parent)
            };
            if let Some(target) = target
                && self.app.zoom_to(target)
            {
                self.map_key = None;
            }
        }
    }
}
