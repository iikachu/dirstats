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

/// Material Symbols glyphs inlined as polygons (Apache-2.0, by Google).
/// Coordinates are the 960-unit viewBox of the SVGs, y flipped to point down.
/// Each chevron is split into two convex arms so it can be filled directly.
mod icons {
    use eframe::egui::{self, Color32, Pos2, Rect, Shape};

    /// `chevron_right`: `M504-480 320-664l56-56 240 240-240 240-56-56 184-184Z`
    pub const CHEVRON_RIGHT: [[(f32, f32); 4]; 2] = [
        [(504.0, 480.0), (320.0, 296.0), (376.0, 240.0), (616.0, 480.0)],
        [(616.0, 480.0), (376.0, 720.0), (320.0, 664.0), (504.0, 480.0)],
    ];
    /// `expand_more` (`keyboard_arrow_down`): `M480-344 240-584l56-56 184 184 184-184 56 56-240 240Z`
    pub const KEYBOARD_ARROW_DOWN: [[(f32, f32); 4]; 2] = [
        [(480.0, 616.0), (240.0, 376.0), (296.0, 320.0), (480.0, 504.0)],
        [(480.0, 504.0), (664.0, 320.0), (720.0, 376.0), (480.0, 616.0)],
    ];

    /// Paint a glyph scaled to fit `rect`, keeping its aspect.
    pub fn paint(painter: &egui::Painter, rect: Rect, glyph: &[[(f32, f32); 4]; 2], color: Color32) {
        let side = rect.width().min(rect.height());
        let scale = side / 960.0;
        let origin = rect.center() - egui::vec2(side, side) / 2.0;
        for arm in glyph {
            let points: Vec<Pos2> = arm.iter().map(|&(x, y)| origin + egui::vec2(x * scale, y * scale)).collect();
            painter.add(Shape::convex_polygon(points, color, egui::Stroke::NONE));
        }
    }
}

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
    /// User-adjustable column widths; `None` until the font is known.
    columns: Option<Columns>,
    /// Row to bring into view on the next frame, set when selecting from the treemap.
    scroll_to: Option<NodeId>,
}

/// Widths of every column in the flat header. The treemap takes whatever is
/// left between the size and extensions columns.
#[derive(Clone, Copy, Debug)]
struct Columns {
    name: f32,
    bar: f32,
    share: f32,
    size: f32,
    ext_name: f32,
    ext_share: f32,
    ext_size: f32,
}

impl Columns {
    const MIN: Columns =
        Columns { name: 80.0, bar: 24.0, share: 40.0, size: 56.0, ext_name: 60.0, ext_share: 40.0, ext_size: 56.0 };
    /// Least width the treemap keeps when other columns grow.
    const MIN_MAP: f32 = 120.0;
    /// Width of the draggable divider between columns.
    const DIVIDER: f32 = 6.0;

    fn initial(window: f32, mono_char: f32) -> Self {
        Self {
            name: (window * 0.22).clamp(160.0, 420.0),
            bar: 60.0,
            share: mono_char * 6.0,
            size: mono_char * 10.0,
            ext_name: 140.0,
            ext_share: mono_char * 6.0,
            ext_size: mono_char * 10.0,
        }
    }

    fn tree_width(&self) -> f32 {
        self.name + self.bar + self.share + self.size
    }

    fn extensions_width(&self) -> f32 {
        self.ext_name + self.ext_share + self.ext_size
    }
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
            columns: None,
            scroll_to: None,
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
    /// Select a node, open the tree down to it and scroll it into view,
    /// without changing the zoom.
    fn select(&mut self, id: NodeId) {
        self.selected = Some(id);
        self.app.expand_to(id);
        self.scroll_to = Some(id);
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

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| self.footer(ui));
        // Keep the panel's background fill but no margin, so the columns run edge to edge.
        let frame = egui::Frame::central_panel(&ctx.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| self.body(ui));
    }
}

impl Gui {
    /// One flat header across the window, then the tree, treemap and
    /// extensions laid out under their columns.
    fn body(&mut self, ui: &mut egui::Ui) {
        // Resolve the font before taking the fonts lock: touching the style
        // inside that closure deadlocks the context.
        let mono_font = egui::TextStyle::Monospace.resolve(ui.style());
        let mono_char = ui.fonts_mut(|f| f.glyph_width(&mono_font, '0'));
        let full = ui.available_rect_before_wrap();
        let mut columns = *self.columns.get_or_insert_with(|| Columns::initial(full.width(), mono_char));
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;

        // Header strip with titles and draggable dividers.
        let header = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), row_height + 4.0));
        ui.painter().rect_filled(header, 0.0, ui.visuals().faint_bg_color);
        let total_fixed = columns.tree_width() + columns.extensions_width();
        let map_width = (full.width() - total_fixed).max(Columns::MIN_MAP);
        let widths = [
            columns.name,
            columns.bar,
            columns.share,
            columns.size,
            map_width,
            columns.ext_name,
            columns.ext_share,
            columns.ext_size,
        ];
        let titles = ["Name", "", "%", "Size", "Treemap", "Extension", "%", "Size"];
        let right_aligned = [false, false, true, true, false, false, true, true];
        let mut x = full.min.x;
        let mut starts = [0.0; 8];
        for (i, &w) in widths.iter().enumerate() {
            starts[i] = x;
            let cell = egui::Rect::from_min_size(egui::pos2(x, header.min.y), egui::vec2(w, header.height()));
            let layout = if right_aligned[i] {
                egui::Layout::right_to_left(egui::Align::Center)
            } else {
                egui::Layout::left_to_right(egui::Align::Center)
            };
            ui.scope_builder(egui::UiBuilder::new().max_rect(cell.shrink2(egui::vec2(6.0, 0.0))).layout(layout), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(titles[i]).strong()).truncate());
            });
            x += w;
            // Divider at the right edge of every column but the last.
            if i + 1 < widths.len() {
                let grip = egui::Rect::from_center_size(egui::pos2(x, header.center().y), egui::vec2(Columns::DIVIDER, header.height()));
                let response = ui.interact(grip, ui.id().with(("divider", i)), Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                let stroke = if response.dragged() { ui.visuals().selection.stroke } else { ui.visuals().widgets.noninteractive.bg_stroke };
                // Guide lines run the full height only between regions; inside
                // the file list the header tick is enough.
                let range = if i == 3 || i == 4 { full.y_range() } else { header.y_range() };
                ui.painter().vline(x, range, stroke);
                let delta = response.drag_delta().x;
                if delta != 0.0 {
                    let min = Columns::MIN;
                    // Each divider resizes the column to its left; the treemap
                    // absorbs the difference. The treemap's own right edge
                    // resizes the extension column the other way.
                    let (col, min_w, sign) = match i {
                        0 => (&mut columns.name, min.name, 1.0),
                        1 => (&mut columns.bar, min.bar, 1.0),
                        2 => (&mut columns.share, min.share, 1.0),
                        3 => (&mut columns.size, min.size, 1.0),
                        4 => (&mut columns.ext_name, min.ext_name, -1.0),
                        5 => (&mut columns.ext_name, min.ext_name, 1.0),
                        _ => (&mut columns.ext_share, min.ext_share, 1.0),
                    };
                    *col = (*col + sign * delta).max(min_w);
                    // Keep the treemap from being squeezed out.
                    let overflow = columns.tree_width() + columns.extensions_width() + Columns::MIN_MAP - full.width();
                    if overflow > 0.0 {
                        let col = match i {
                            0 => &mut columns.name,
                            1 => &mut columns.bar,
                            2 => &mut columns.share,
                            3 => &mut columns.size,
                            4 | 5 => &mut columns.ext_name,
                            _ => &mut columns.ext_share,
                        };
                        *col -= overflow;
                    }
                }
            }
        }
        self.columns = Some(columns);

        // Body regions under the header.
        let body = egui::Rect::from_min_max(egui::pos2(full.min.x, header.max.y + 1.0), full.max);
        let region = |from: usize, to: usize| {
            let x0 = starts[from];
            let x1 = if to + 1 < starts.len() { starts[to + 1] } else { full.max.x };
            egui::Rect::from_min_max(egui::pos2(x0, body.min.y), egui::pos2(x1, body.max.y))
        };
        let tree_rect = region(0, 3);
        let map_rect = region(4, 4);
        let ext_rect = region(5, 7);

        let mut tree_ui = ui.new_child(egui::UiBuilder::new().max_rect(tree_rect).id_salt("tree"));
        tree_ui.set_clip_rect(tree_rect);
        let tree_columns = [starts[0], starts[1], starts[2], starts[3], starts[4]];
        self.entry_list(&mut tree_ui, tree_columns, row_height);

        let mut map_ui = ui.new_child(egui::UiBuilder::new().max_rect(map_rect).id_salt("map"));
        map_ui.set_clip_rect(map_rect);
        self.treemap(&mut map_ui);

        let mut ext_ui = ui.new_child(egui::UiBuilder::new().max_rect(ext_rect).id_salt("extensions"));
        ext_ui.set_clip_rect(ext_rect);
        let ext_edges = [starts[5], starts[6], starts[7], full.max.x];
        self.legend(&mut ext_ui, ext_edges, row_height);
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

    /// Extensions ranked by total size, largest first, in cells aligned to
    /// the header: swatch and name, share, size.
    fn legend(&mut self, ui: &mut egui::Ui, edges: [f32; 4], row_height: f32) {
        let Some(colors) = &self.colors else {
            ui.label("waiting for scan…");
            return;
        };
        let total: u64 = colors.entries().iter().map(|(_, size, _)| size).sum();
        let entries = colors.entries();
        let pad = 6.0;
        let mono = egui::TextStyle::Monospace.resolve(ui.style());
        egui::ScrollArea::vertical().id_salt("legend").auto_shrink([false, false]).show_rows(ui, row_height, entries.len(), |ui, range| {
            for (ext, size, color) in &entries[range] {
                let (row_rect, row) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), Sense::hover());
                if row.hovered() {
                    ui.painter().rect_filled(row_rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let text = ui.visuals().text_color();
                let (top, bottom) = (row_rect.min.y, row_rect.max.y);
                let cell = |from: f32, to: f32| egui::Rect::from_min_max(egui::pos2(from, top), egui::pos2(to, bottom));

                let name_cell = cell(edges[0], edges[1]);
                let [r, g, b] = color.to_srgb();
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
                    ui.painter().with_clip_rect(c).text(egui::pos2(c.max.x - pad, c.center().y), egui::Align2::RIGHT_CENTER, value, mono.clone(), text);
                }
            }
        });
    }

    /// Rows of the tree. `edges` are the absolute x positions of the name,
    /// bar, share and size columns and the right edge of size, straight from
    /// the header, so cells always line up with it.
    fn entry_list(&mut self, ui: &mut egui::Ui, edges: [f32; 5], row_height: f32) {
        let Some(tree) = &self.app.tree else {
            ui.label("waiting for scan…");
            return;
        };
        let rows = self.app.tree_rows();
        let selected = self.selected;
        let indent = 16.0;
        let pad = 6.0;
        let mono = egui::TextStyle::Monospace.resolve(ui.style());

        let mut select = None;
        let mut toggle = None;
        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(target) = self.scroll_to.take()
            && let Some(index) = rows.iter().position(|&(id, _)| id == target)
        {
            // Centre the row: show_rows spaces rows by height plus item spacing.
            let step = row_height + ui.spacing().item_spacing.y;
            let offset = index as f32 * step - (ui.available_height() - row_height) / 2.0;
            scroll = scroll.vertical_scroll_offset(offset.max(0.0));
        }
        scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
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
                let (top, bottom) = (row_rect.min.y, row_rect.max.y);
                let cell = |from: f32, to: f32| egui::Rect::from_min_max(egui::pos2(from, top), egui::pos2(to, bottom));

                // Name column: indent, expander, then a truncating label clipped to the column.
                let name_cell = cell(edges[0], edges[1]);
                let expander_rect = egui::Rect::from_min_size(
                    egui::pos2(edges[0] + pad + indent * depth as f32, top),
                    egui::vec2(18.0, row_height),
                );
                if is_dir {
                    let glyph = if self.app.expanded.contains(&id) { &icons::KEYBOARD_ARROW_DOWN } else { &icons::CHEVRON_RIGHT };
                    let response = ui.interact(expander_rect, ui.id().with(("expander", id)), Sense::click());
                    let color = if response.hovered() { ui.visuals().strong_text_color() } else { text };
                    icons::paint(&ui.painter().with_clip_rect(name_cell), expander_rect.shrink(1.0), glyph, color);
                    if response.clicked() {
                        toggle = Some(id);
                    }
                }
                let mut name = node.name.to_string_lossy().into_owned();
                if is_dir {
                    name.push('/');
                }
                if node.error {
                    name.push_str("  !");
                }
                let label_rect = egui::Rect::from_min_max(egui::pos2(expander_rect.max.x + 2.0, top), egui::pos2(name_cell.max.x - pad, bottom));
                if label_rect.width() > 4.0 {
                    let mut name_ui = ui.new_child(egui::UiBuilder::new().max_rect(label_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    name_ui.set_clip_rect(label_rect.intersect(ui.clip_rect()));
                    name_ui.add(egui::Label::new(egui::RichText::new(name).color(text)).truncate().selectable(false));
                }

                // Bar column.
                let bar = cell(edges[1], edges[2]).shrink2(egui::vec2(pad, 5.0));
                if bar.width() > 0.0 {
                    ui.painter().rect_filled(bar, 2.0, ui.visuals().faint_bg_color);
                    let mut filled = bar;
                    filled.set_width(bar.width() * (share / 100.0) as f32);
                    ui.painter().rect_filled(filled, 2.0, ui.visuals().weak_text_color());
                }

                // Share and size: right-aligned monospace, clipped to their cells.
                for (from, to, value) in [(edges[2], edges[3], format!("{share:.1}")), (edges[3], edges[4], format::size(size))] {
                    let c = cell(from, to);
                    ui.painter().with_clip_rect(c).text(
                        egui::pos2(c.max.x - pad, c.center().y),
                        egui::Align2::RIGHT_CENTER,
                        value,
                        mono.clone(),
                        text,
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

