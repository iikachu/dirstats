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

/// What a cached treemap render depends on: tree version, zoom directory and size.
type MapKey = (u64, NodeId, u32, u32);
/// What the highlight layer depends on: the base render plus what is hovered.
type HighlightKey = (MapKey, Highlight);

/// What the pulsing layer makes vivid.
#[derive(Clone, Debug, PartialEq)]
enum Highlight {
    /// Every file with this extension.
    Extension(Option<String>),
    /// A node and, for a directory, everything under it.
    Subtree(NodeId),
}

/// What is selected. Selecting one kind clears the other.
#[derive(Clone, Debug, PartialEq)]
enum Selection {
    /// A node anywhere under the zoom directory.
    Node(NodeId),
    /// Every file with this extension (`None` is "no extension").
    Extension(Option<String>),
}

/// What a node's context menu asked for; applied after the menu closes.
#[derive(Clone, Copy, Debug)]
enum NodeAction {
    Zoom,
    CopyPath,
    #[cfg(feature = "open")]
    Open,
    #[cfg(feature = "trash")]
    Trash,
}

/// Menu items for a node: the same in the tree and the treemap. `is_dir`
/// decides whether "Zoom in" is offered. Returns the chosen action.
fn node_menu(ui: &mut egui::Ui, path: &std::path::Path, is_dir: bool) -> Option<NodeAction> {
    let mut action = None;
    ui.set_max_width(320.0);
    // Header: file name in bold, its folder underneath in small weak text,
    // both on one line each and cut with an ellipsis rather than wrapped.
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let parent = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
        ui.add(egui::Label::new(egui::RichText::new(name).strong()).truncate().selectable(false));
        if !parent.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(parent).weak().small()).truncate().selectable(false));
        }
    });
    ui.separator();
    if is_dir && ui.button("Zoom in").clicked() {
        action = Some(NodeAction::Zoom);
    }
    if ui.button("Copy path").clicked() {
        action = Some(NodeAction::CopyPath);
    }
    #[cfg(feature = "open")]
    if ui.button("Open").clicked() {
        action = Some(NodeAction::Open);
    }
    #[cfg(feature = "trash")]
    if ui.button("Move to trash").clicked() {
        action = Some(NodeAction::Trash);
    }
    if action.is_some() {
        ui.close();
    }
    action
}

/// Period of the highlight pulse.
const PULSE_SECONDS: f64 = 2.4;

/// How far toward the sRGB gamut edge a highlighted colour's chroma moves.
/// Relative to the edge rather than a fixed factor, so every hue gets a
/// comparable perceived boost.
const HIGHLIGHT_TOWARD_MAX: f64 = 0.7;
/// Lightness added to highlighted boxes.
const HIGHLIGHT_LIGHTNESS: f64 = 0.06;

/// The vivid version of a colour used for hover and the legend swatch.
fn vivid(color: dirstats_treemap::Oklch) -> dirstats_treemap::Oklch {
    color.lighten(HIGHLIGHT_LIGHTNESS).toward_max_chroma(HIGHLIGHT_TOWARD_MAX)
}

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
    /// `chevron_left`: `chevron_right` mirrored horizontally.
    pub const CHEVRON_LEFT: [[(f32, f32); 4]; 2] = [
        [(456.0, 480.0), (640.0, 296.0), (584.0, 240.0), (344.0, 480.0)],
        [(344.0, 480.0), (584.0, 720.0), (640.0, 664.0), (456.0, 480.0)],
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
    map_key: Option<MapKey>,
    texture: Option<TextureHandle>,
    /// The same map with the hovered extension made vivid, blended over the base.
    highlight: Option<TextureHandle>,
    highlight_key: Option<HighlightKey>,
    style: Style,
    /// Current selection: a node or an extension, never both.
    selection: Option<Selection>,
    /// User-adjustable column widths; `None` until the font is known.
    columns: Option<Columns>,
    /// Row to bring into view on the next frame, set when selecting from the treemap.
    scroll_to: Option<NodeId>,
    /// What the pointer is over in the legend or the tree; its boxes pulse vivid.
    hovered_highlight: Option<Highlight>,
    /// Hover collected during this frame, applied at the end of `body`.
    next_highlight: Option<Highlight>,
    /// Hover pulses only while the mouse is driving; keyboard navigation
    /// switches it off until the pointer moves again.
    hover_active: bool,
    /// Box under the pointer when the treemap was right-clicked; the menu is
    /// built from this so it survives the pointer moving onto the menu.
    menu_node: Option<NodeId>,
    /// Text to put on the clipboard at the end of the frame.
    pending_copy: Option<String>,
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
            highlight: None,
            highlight_key: None,
            style: Style::Squarified,
            selection: None,
            columns: None,
            scroll_to: None,
            hovered_highlight: None,
            next_highlight: None,
            hover_active: true,
            menu_node: None,
            pending_copy: None,
        }
    }

    fn tree_changed(&mut self) {
        self.tree_version += 1;
        self.selection = None;
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
        }

        // Highlight layer: rendered once per hovered target, pulsed at draw time.
        let Some(target) = &self.hovered_highlight else {
            self.highlight_key = None;
            return;
        };
        let highlight_key = (key, target.clone());
        if self.highlight_key.as_ref() == Some(&highlight_key) {
            return;
        }
        let options = TreemapOptions { style: self.style, ..Default::default() };
        let matches = |t: &dirstats_app::Tree, id: NodeId| match target {
            Highlight::Extension(ext) => *ext == ExtensionColors::extension(t.node(id)),
            Highlight::Subtree(root) => {
                let mut current = Some(id);
                while let Some(n) = current {
                    if n == *root {
                        return true;
                    }
                    current = t.node(n).parent;
                }
                false
            }
        };
        let vivid = render(tree, dir, width, height, &options, |t, id| {
            let color = colors.color(t, id);
            if matches(t, id) { vivid(color) } else { color }
        });
        let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &vivid.pixels);
        match &mut self.highlight {
            Some(texture) => texture.set(image, TextureOptions::LINEAR),
            None => self.highlight = Some(ctx.load_texture("treemap-highlight", image, TextureOptions::LINEAR)),
        }
        self.highlight_key = Some(highlight_key);
    }
}

impl Gui {
    fn selected_node(&self) -> Option<NodeId> {
        match &self.selection {
            Some(Selection::Node(id)) => Some(*id),
            _ => None,
        }
    }

    fn selected_extension(&self) -> Option<&Option<String>> {
        match &self.selection {
            Some(Selection::Extension(ext)) => Some(ext),
            _ => None,
        }
    }

    /// Select a node, open the tree down to it and scroll it into view,
    /// without changing the zoom.
    fn select(&mut self, id: NodeId) {
        self.selection = Some(Selection::Node(id));
        self.app.expand_to(id);
        self.scroll_to = Some(id);
    }

    /// Carry out a context-menu action on `node`.
    fn apply(&mut self, node: NodeId, action: NodeAction) {
        self.select(node);
        match action {
            NodeAction::Zoom => self.zoom(node),
            NodeAction::CopyPath => {
                if let Some(path) = self.app.path_of(node) {
                    let text = path.display().to_string();
                    self.app.message = Some(format!("copied {text}"));
                    self.pending_copy = Some(text);
                }
            }
            #[cfg(feature = "open")]
            NodeAction::Open => {
                if let Err(err) = self.app.open_node(node) {
                    self.app.message = Some(format!("open failed: {err}"));
                }
            }
            #[cfg(feature = "trash")]
            NodeAction::Trash => {
                if let Err(err) = self.app.trash_node(node) {
                    self.app.message = Some(format!("trash failed: {err}"));
                }
            }
        }
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
            && let Some(id) = self.selected_node()
        {
            self.zoom(id);
        }

        let toolbar_frame = egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(10, 6));
        egui::TopBottomPanel::top("toolbar").frame(toolbar_frame).show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| self.footer(ui));
        // Keep the panel's background fill but no margin, so the columns run edge to edge.
        let frame = egui::Frame::central_panel(&ctx.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| self.body(ui));
        if let Some(text) = self.pending_copy.take() {
            ctx.copy_text(text);
        }
    }
}

impl Gui {
    /// One flat header across the window, then the tree, treemap and
    /// extensions laid out under their columns.
    fn body(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_pressed()) {
            self.hover_active = true;
        }
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

        // The treemap is drawn before the legend, so hover takes effect next frame.
        if self.hovered_highlight != self.next_highlight {
            self.hovered_highlight = self.next_highlight.take();
            ui.ctx().request_repaint();
        } else {
            self.next_highlight = None;
        }
    }
}

impl Gui {
    /// Toolbar: back, breadcrumbs, totals; layout toggle and rescan on the right.
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.x = 6.0;
        let icon_button = |ui: &mut egui::Ui, glyph: &[[(f32, f32); 4]; 2], enabled: bool, tip: &str| -> egui::Response {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), if enabled { Sense::click() } else { Sense::hover() });
            let visuals = ui.style().interact(&response);
            if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
                ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
            }
            let color = if enabled { visuals.text_color() } else { ui.visuals().weak_text_color().gamma_multiply(0.5) };
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
                ui.label(egui::RichText::new("No scan").strong().size(15.0));
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
                    if icon_button(ui, &icons::CHEVRON_LEFT, can_back, "Back (Backspace)").clicked() {
                        zoom_target = Some(None);
                    }
                    let crumbs = self.app.breadcrumbs();
                    for (i, &id) in crumbs.iter().enumerate() {
                        if i > 0 {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                            icons::paint(ui.painter(), rect, &icons::CHEVRON_RIGHT, ui.visuals().weak_text_color());
                        }
                        let name = tree.node(id).name.to_string_lossy().into_owned();
                        if i + 1 == crumbs.len() {
                            ui.add(egui::Label::new(egui::RichText::new(name).strong().size(15.0)).truncate());
                        } else {
                            let link = ui.add(egui::Label::new(egui::RichText::new(name).size(15.0)).sense(Sense::click()).truncate());
                            if link.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if link.clicked() {
                                zoom_target = Some(Some(id));
                            }
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
                    ui.painter().rect_filled(row_rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
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
                    dirstats_treemap::Oklch::new(
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

    /// Keyboard navigation in the tree. Up and down move through the visible
    /// rows; Home and End (or Cmd+Up/Down on macOS, Ctrl+Home/End elsewhere)
    /// jump to the ends; Page Up and Page Down (or Option+Up/Down on macOS)
    /// move by a screenful; right expands a directory or steps into its first
    /// child; left collapses it or steps to the parent. `rows` is refreshed
    /// when the expansion changes.
    fn keyboard_navigation(&mut self, ui: &egui::Ui, rows: &mut Vec<(NodeId, u32)>, row_step: f32) {
        let Some(tree) = &self.app.tree else { return };
        let page = ((ui.available_height() / row_step).floor() as usize).max(1);
        let last = rows.len().saturating_sub(1);
        #[derive(Clone, Copy, PartialEq)]
        enum Nav {
            Up,
            Down,
            Left,
            Right,
            First,
            Last,
            PageUp,
            PageDown,
        }
        let nav = ui.input(|i| {
            let m = i.modifiers;
            // Ctrl+Home/End on Linux and Windows arrive as plain Home/End here.
            let cmd = m.mac_cmd;
            let alt = m.alt;
            if i.key_pressed(Key::Home) || (cmd && i.key_pressed(Key::ArrowUp)) {
                Some(Nav::First)
            } else if i.key_pressed(Key::End) || (cmd && i.key_pressed(Key::ArrowDown)) {
                Some(Nav::Last)
            } else if i.key_pressed(Key::PageUp) || (alt && i.key_pressed(Key::ArrowUp)) {
                Some(Nav::PageUp)
            } else if i.key_pressed(Key::PageDown) || (alt && i.key_pressed(Key::ArrowDown)) {
                Some(Nav::PageDown)
            } else if i.key_pressed(Key::ArrowUp) {
                Some(Nav::Up)
            } else if i.key_pressed(Key::ArrowDown) {
                Some(Nav::Down)
            } else if i.key_pressed(Key::ArrowLeft) {
                Some(Nav::Left)
            } else if i.key_pressed(Key::ArrowRight) {
                Some(Nav::Right)
            } else {
                None
            }
        });
        let Some(nav) = nav else { return };
        self.hover_active = false;
        if rows.is_empty() {
            return;
        }
        let index = self.selected_node().and_then(|id| rows.iter().position(|&(r, _)| r == id));
        let (left, right) = (nav == Nav::Left, nav == Nav::Right);
        let mut target = None;
        match nav {
            Nav::Down => target = Some(index.map_or(0, |i| (i + 1).min(last))),
            Nav::Up => target = Some(index.map_or(0, |i| i.saturating_sub(1))),
            Nav::First => target = Some(0),
            Nav::Last => target = Some(last),
            Nav::PageDown => target = Some(index.map_or(0, |i| (i + page).min(last))),
            Nav::PageUp => target = Some(index.map_or(0, |i| i.saturating_sub(page))),
            Nav::Left | Nav::Right => {}
        }
        if target.is_some() {
        } else if let Some(i) = index {
            let (id, _) = rows[i];
            let is_dir = !tree.children(id).is_empty();
            if right && is_dir {
                if self.app.expanded.contains(&id) {
                    target = Some(i + 1); // first child follows its parent
                } else {
                    self.app.toggle_expanded(id);
                    *rows = self.app.tree_rows();
                    target = Some(i);
                }
            } else if left {
                if is_dir && self.app.expanded.contains(&id) {
                    self.app.toggle_expanded(id);
                    *rows = self.app.tree_rows();
                    target = Some(i);
                } else if let Some(parent) = tree.node(id).parent {
                    target = rows.iter().position(|&(r, _)| r == parent);
                }
            }
        } else if right || left {
            target = Some(0);
        }
        if let Some(i) = target
            && let Some(&(id, _)) = rows.get(i)
        {
            self.selection = Some(Selection::Node(id));
            self.scroll_to = Some(id);
        }
    }

    /// Rows of the tree. `edges` are the absolute x positions of the name,
    /// bar, share and size columns and the right edge of size, straight from
    /// the header, so cells always line up with it.
    fn entry_list(&mut self, ui: &mut egui::Ui, edges: [f32; 5], row_height: f32) {
        if self.app.tree.is_none() {
            ui.label("waiting for scan…");
            return;
        }
        let mut rows = self.app.tree_rows();
        self.keyboard_navigation(ui, &mut rows, row_height + ui.spacing().item_spacing.y);
        let tree = self.app.tree.as_ref().expect("checked above");
        let selected = self.selected_node();
        let indent = 16.0;
        let pad = 6.0;
        let mono = egui::TextStyle::Monospace.resolve(ui.style());

        let mut select = None;
        let mut toggle = None;
        let mut menu_action: Option<(NodeId, NodeAction)> = None;
        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(target) = self.scroll_to.take()
            && let Some(index) = rows.iter().position(|&(id, _)| id == target)
        {
            // show_rows spaces rows by height plus item spacing. Scroll only
            // when the row is outside the view, and then just far enough.
            let step = row_height + ui.spacing().item_spacing.y;
            let view = ui.available_height();
            let row_top = index as f32 * step;
            let current = ui.ctx().memory(|m| m.data.get_temp::<f32>(ui.id().with("tree-scroll"))).unwrap_or(0.0);
            let offset = if row_top < current {
                Some(row_top)
            } else if row_top + row_height > current + view {
                Some(row_top + row_height - view)
            } else {
                None
            };
            if let Some(offset) = offset {
                scroll = scroll.vertical_scroll_offset(offset.max(0.0));
            }
        }
        let scroll_id = ui.id().with("tree-scroll");
        let output = scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
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
                    ui.painter().rect_filled(row_rect, 0.0, ui.visuals().selection.bg_fill);
                } else if row.hovered() {
                    ui.painter().rect_filled(row_rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
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

                if row.clicked() || row.secondary_clicked() {
                    select = Some(id);
                }
                let path = tree.path(id);
                row.context_menu(|ui| {
                    if let Some(action) = node_menu(ui, &path, is_dir) {
                        menu_action = Some((id, action));
                    }
                });
                if row.double_clicked() {
                    if is_dir {
                        toggle = Some(id);
                    }
                    select = Some(id);
                }
                if row.hovered() {
                    self.app.hovered = Some(id);
                    if self.hover_active {
                        self.next_highlight = Some(Highlight::Subtree(id));
                    }
                }
                if is_selected && is_dir && ui.input(|i| i.key_pressed(Key::Space)) {
                    toggle = Some(id);
                }
            }
        });
        ui.ctx().memory_mut(|m| m.data.insert_temp(scroll_id, output.state.offset.y));
        if let Some(id) = toggle {
            self.app.toggle_expanded(id);
        }
        if let Some(id) = select {
            self.selection = Some(Selection::Node(id));
        }
        if let Some((id, action)) = menu_action {
            self.apply(id, action);
        }
    }

    /// Centred scan status shown in the treemap area until a tree arrives.
    fn scan_progress(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let (title, root, detail) = match &self.app.scan {
            Some(scan) => (
                "Scanning".to_string(),
                scan.root.display().to_string(),
                format!(
                    "{} entries · {} errors · {:.1}s",
                    scan.entries(),
                    scan.errors(),
                    scan.started.elapsed().as_secs_f64()
                ),
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
    }

    fn treemap(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (width, height) = (available.x.max(1.0) as u32, available.y.max(1.0) as u32);
        self.ensure_map(ui.ctx(), width, height);
        let (Some(texture), Some(map)) = (&self.texture, &self.map) else {
            self.scan_progress(ui);
            return;
        };
        let response = ui.add(egui::Image::new((texture.id(), available)).sense(Sense::click()));
        // Pulse the vivid layer over the base while an extension is hovered.
        if self.highlight_key.is_some()
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
        let origin = response.rect.min;
        let painter = ui.painter_at(response.rect);

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
        match &self.selection {
            Some(Selection::Node(selected)) => outline(*selected, Color32::WHITE, 2.0),
            Some(Selection::Extension(ext)) => {
                if let Some(tree) = &self.app.tree {
                    for item in &map.items {
                        let node = tree.node(item.node);
                        if node.kind != dirstats_app::scan::Kind::Directory && ExtensionColors::extension(node) == *ext {
                            outline(item.node, Color32::WHITE, 2.0);
                        }
                    }
                }
            }
            None => {}
        }

        if let Some(node) = hovered {
            if response.clicked() {
                self.select(node);
            }
            if response.secondary_clicked() {
                self.menu_node = Some(node);
                self.select(node);
            }
        }
        // The menu is drawn every frame from the pinned node, not from hover.
        if let Some(node) = self.menu_node {
            let (path, is_dir) = match &self.app.tree {
                Some(tree) => (tree.path(node), !tree.children(node).is_empty()),
                None => (std::path::PathBuf::new(), false),
            };
            let mut action = None;
            let menu = response.context_menu(|ui| action = node_menu(ui, &path, is_dir));
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

