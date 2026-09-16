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
    /// Selected node, anywhere under the zoom directory.
    selected: Option<NodeId>,
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
            selected: None,
        }
    }

    fn tree_changed(&mut self) {
        self.tree_version += 1;
        self.selected = None;
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

impl Gui {
    /// Select a node and open the tree down to it, without changing the zoom.
    fn select(&mut self, id: NodeId) {
        self.selected = Some(id);
        self.app.expand_to(id);
    }

    /// Zoom into `id`, or its parent when it is a file.
    fn zoom(&mut self, id: NodeId) {
        let Some(tree) = &self.app.tree else { return };
        let target = if tree.children(id).is_empty() { tree.node(id).parent } else { Some(id) };
        if let Some(target) = target
            && self.app.zoom_to(target)
        {
            self.map_key = None;
        }
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
        if ctx.input(|i| i.key_pressed(Key::Enter))
            && let Some(id) = self.selected
        {
            self.zoom(id);
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| self.footer(ui));
        // Panel widths follow the window and the font rather than fixed pixels.
        let window = ctx.content_rect().width();
        // Resolve the font before taking the fonts lock: touching the style
        // inside that closure deadlocks the context.
        let mono_font = egui::TextStyle::Monospace.resolve(&ctx.style());
        let mono_char = ctx.fonts_mut(|f| f.glyph_width(&mono_font, '0'));
        let spacing = ctx.style().spacing.item_spacing.x;
        // Figures are "100.0%  999.9 GiB" (18 monospace chars) plus swatch and gaps.
        let legend_fixed = mono_char * 18.0 + 14.0 + spacing * 4.0 + 16.0;
        egui::SidePanel::left("entries")
            .default_width((window * 0.32).clamp(280.0, 600.0))
            .min_width(240.0)
            .show(ctx, |ui| self.entry_list(ui));
        egui::SidePanel::right("extensions")
            .default_width(legend_fixed + 120.0)
            .min_width(legend_fixed + 40.0)
            .show(ctx, |ui| self.legend(ui));
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

    /// One fixed-height line: the hovered path, or the last message.
    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_height(ui.text_style_height(&egui::TextStyle::Monospace));
            if let (Some(tree), Some(hovered)) = (&self.app.tree, self.app.hovered) {
                ui.monospace(format!("{:>10}  {}", format::size(tree.size(hovered)), tree.path(hovered).display()));
            } else if let Some(message) = &self.app.message {
                ui.label(message);
            } else {
                ui.label(" ");
            }
        });
    }

    /// Extensions ranked by total size with their swatches, largest first.
    fn legend(&mut self, ui: &mut egui::Ui) {
        ui.heading("Extensions");
        let Some(colors) = &self.colors else {
            ui.label("waiting for scan…");
            return;
        };
        let total: u64 = colors.entries().iter().map(|(_, size, _)| size).sum();
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        let entries = colors.entries();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_height, entries.len(), |ui, range| {
            for (ext, size, color) in &entries[range] {
                ui.horizontal(|ui| {
                    let [r, g, b] = color.to_srgb();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                    ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(r, g, b));
                    let full = ext.as_deref().map_or("(none)".to_string(), |e| format!(".{e}"));
                    // Right-to-left: the figures take their width first and the
                    // label gets whatever is left, ending in an ellipsis if cut.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.monospace(format!("{:>5.1}%  {:>9}", format::percent(*size, total), format::size(*size)));
                        ui.add(egui::Label::new(&full).truncate()).on_hover_text(&full);
                    });
                });
            }
        });
    }

    fn entry_list(&mut self, ui: &mut egui::Ui) {
        let Some(tree) = &self.app.tree else {
            ui.label("waiting for scan…");
            return;
        };
        let rows = self.app.tree_rows();
        let selected = self.selected;
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        let indent = 16.0;
        let mono = egui::TextStyle::Monospace.resolve(ui.style());
        let mono_char = ui.fonts_mut(|f| f.glyph_width(&mono, '0'));
        // Fixed columns, right to left: size (9 chars), share (6 chars), bar.
        let size_w = mono_char * 9.0;
        let share_w = mono_char * 6.0;
        let bar_w = 60.0;
        let spacing = ui.spacing().item_spacing.x;

        // Header.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (w, title) in [(size_w, "Size"), (share_w, "%")] {
                    ui.allocate_ui_with_layout(egui::vec2(w, row_height), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.strong(title);
                    });
                }
                ui.add_space(bar_w);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.strong("Name");
                });
            });
        });
        ui.separator();

        let mut select = None;
        let mut toggle = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_height, rows.len(), |ui, range| {
            for &(id, depth) in &rows[range] {
                let node = tree.node(id);
                let size = tree.size(id);
                let parent_size = node.parent.map_or(size, |p| tree.size(p));
                let share = format::percent(size, parent_size);
                let is_dir = !tree.children(id).is_empty();
                let is_selected = selected == Some(id);

                // Whole-row background and click target.
                let (row_rect, row) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), Sense::click());
                if is_selected {
                    ui.painter().rect_filled(row_rect, 2.0, ui.visuals().selection.bg_fill);
                } else if row.hovered() {
                    ui.painter().rect_filled(row_rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let text = if is_selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };

                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                child.add_space(indent * depth as f32);
                let arrow = if !is_dir {
                    " "
                } else if self.app.expanded.contains(&id) {
                    "▾"
                } else {
                    "▸"
                };
                let expander = child.add_sized([14.0, row_height], egui::Label::new(egui::RichText::new(arrow).color(text)).sense(Sense::click()));
                if is_dir && expander.clicked() {
                    toggle = Some(id);
                }
                let mut name = node.name.to_string_lossy().into_owned();
                if is_dir {
                    name.push('/');
                }
                if node.error {
                    name.push_str("  !");
                }
                let fixed = size_w + share_w + bar_w + spacing * 3.0;
                let name_w = (child.available_width() - fixed).max(20.0);
                child.allocate_ui_with_layout(
                    egui::vec2(name_w, row_height),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| ui.add(egui::Label::new(egui::RichText::new(name).color(text)).truncate()),
                );

                let (bar, _) = child.allocate_exact_size(egui::vec2(bar_w, row_height - 10.0), Sense::hover());
                child.painter().rect_filled(bar, 2.0, child.visuals().faint_bg_color);
                let mut filled = bar;
                filled.set_width(bar.width() * (share / 100.0) as f32);
                child.painter().rect_filled(filled, 2.0, child.visuals().weak_text_color());
                for (w, value) in [(share_w, format!("{share:.1}")), (size_w, format::size(size))] {
                    child.allocate_ui_with_layout(
                        egui::vec2(w, row_height),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| ui.label(egui::RichText::new(value).monospace().color(text)),
                    );
                }

                if row.clicked() {
                    select = Some(id);
                }
                if row.double_clicked() {
                    if is_dir {
                        toggle = Some(id);
                    }
                    select = Some(id);
                }
                if row.hovered() {
                    self.app.hovered = Some(id);
                }
                if is_selected && is_dir && ui.input(|i| i.key_pressed(Key::Space)) {
                    toggle = Some(id);
                }
            }
        });
        if let Some(id) = toggle {
            self.app.toggle_expanded(id);
        }
        if let Some(id) = select {
            self.selected = Some(id);
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
        if let Some(selected) = self.selected {
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
                self.select(node);
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
                    self.select(node);
                    if let Err(err) = self.app.open_node(node) {
                        self.app.message = Some(format!("open failed: {err}"));
                    }
                    ui.close();
                }
                #[cfg(feature = "trash")]
                if ui.button("Move to trash").clicked() {
                    self.select(node);
                    if let Err(err) = self.app.trash_node(node) {
                        self.app.message = Some(format!("trash failed: {err}"));
                    }
                    ui.close();
                }
            });
        }
        if let Some(node) = zoom {
            self.zoom(node);
        }
    }
}

