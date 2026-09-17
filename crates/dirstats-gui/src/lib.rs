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
    #[cfg(feature = "trash")]
    PutBack,
}

/// Menu items for a node: the same in the tree and the treemap. `is_dir`
/// decides whether "Zoom in" is offered. Returns the chosen action.
fn node_menu(ui: &mut egui::Ui, path: &std::path::Path, is_dir: bool, trashed: TrashState) -> Option<NodeAction> {
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
    if is_dir && menu_item(ui, None, "Zoom in", false).clicked() {
        action = Some(NodeAction::Zoom);
    }
    if menu_item(ui, Some(icons::Glyph::ContentCopy), "Copy path", false).clicked() {
        action = Some(NodeAction::CopyPath);
    }
    #[cfg(feature = "open")]
    if menu_item(ui, Some(icons::Glyph::OpenInNew), "Open", false).clicked() {
        action = Some(NodeAction::Open);
    }
    #[cfg(feature = "trash")]
    {
        menu_separator(ui);
        match trashed {
            TrashState::Present => {
                if menu_item(ui, Some(icons::Glyph::Delete), "Move to Trash", true).clicked() {
                    action = Some(NodeAction::Trash);
                }
            }
            TrashState::CanPutBack => {
                if menu_item(ui, Some(icons::Glyph::Undo), "Put Back", false).clicked() {
                    action = Some(NodeAction::PutBack);
                }
            }
            TrashState::Trashed => {
                ui.add_enabled_ui(false, |ui| menu_item(ui, Some(icons::Glyph::Delete), "In Trash", false));
            }
        }
    }
    #[cfg(not(feature = "trash"))]
    let _ = trashed;
    if action.is_some() {
        ui.close();
    }
    action
}

/// Trash state of the node a menu is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrashState {
    Present,
    /// Trashed, and the app knows where it went.
    CanPutBack,
    /// Trashed, location unknown.
    Trashed,
}

/// A menu row: optional leading icon in a fixed slot so labels line up,
/// then the label. `destructive` uses the error colour.
fn menu_item(ui: &mut egui::Ui, glyph: Option<icons::Glyph>, label: &str, destructive: bool) -> egui::Response {
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
fn menu_separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, ui.visuals().widgets.noninteractive.bg_stroke);
    ui.add_space(4.0);
}

/// Local date and time, minute precision.
fn format_time(time: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time).format("%Y-%m-%d %H:%M").to_string()
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

mod icons {
    //! Material Symbols Outlined glyphs (Apache-2.0, by Google), each the
    //! `d` attribute of its 24px SVG in the 960-unit viewBox with y up.
    //! A glyph is rasterised once with an even-odd scanline fill into a
    //! cached alpha texture, then drawn tinted, so paths with holes (the
    //! copy sheets, the can) render exactly as designed.

    use eframe::egui::{self, Color32, Rect, TextureHandle, TextureOptions};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    #[allow(dead_code)] // Open and the trash glyphs are only reachable with their features.
    pub enum Glyph {
        ChevronRight,
        /// `chevron_right` mirrored; Material's `chevron_left` is the same shape.
        ChevronLeft,
        /// `expand_more`, also published as `keyboard_arrow_down`.
        ExpandMore,
        ContentCopy,
        OpenInNew,
        Delete,
        Undo,
        /// Column picker.
        ViewColumn,
    }

    impl Glyph {
        fn path(self) -> &'static str {
            match self {
                Glyph::ChevronRight | Glyph::ChevronLeft => "M504-480 320-664l56-56 240 240-240 240-56-56 184-184Z",
                Glyph::ExpandMore => "M480-344 240-584l56-56 184 184 184-184 56 56-240 240Z",
                Glyph::ContentCopy => "M360-240q-33 0-56.5-23.5T280-320v-480q0-33 23.5-56.5T360-880h360q33 0 56.5 23.5T800-800v480q0 33-23.5 56.5T720-240H360Zm0-80h360v-480H360v480ZM200-80q-33 0-56.5-23.5T120-160v-560h80v560h440v80H200Zm160-240v-480 480Z",
                Glyph::OpenInNew => "M200-120q-33 0-56.5-23.5T120-200v-560q0-33 23.5-56.5T200-840h280v80H200v560h560v-280h80v280q0 33-23.5 56.5T760-120H200Zm188-212-56-56 372-372H560v-80h280v280h-80v-144L388-332Z",
                Glyph::Delete => "M280-120q-33 0-56.5-23.5T200-200v-520h-40v-80h200v-40h240v40h200v80h-40v520q0 33-23.5 56.5T680-120H280Zm400-600H280v520h400v-520ZM360-280h80v-360h-80v360Zm160 0h80v-360h-80v360ZM280-720v520-520Z",
                Glyph::ViewColumn => "M121-280v-400q0-33 23.5-56.5T201-760h559q33 0 56.5 23.5T840-680v400q0 33-23.5 56.5T760-200H201q-33 0-56.5-23.5T121-280Zm79 0h133v-400H200v400Zm213 0h133v-400H413v400Zm213 0h133v-400H626v400Z",
                Glyph::Undo => "M280-200v-80h284q63 0 109.5-40T720-420q0-60-46.5-100T564-560H312l104 104-56 56-200-200 200-200 56 56-104 104h252q97 0 166.5 63T800-420q0 94-69.5 157T564-200H280Z",
            }
        }

        fn mirrored(self) -> bool {
            self == Glyph::ChevronLeft
        }
    }

    /// Texture side in pixels; glyphs are drawn at 14–18px so this is plenty.
    const TEXTURE_SIDE: usize = 48;
    /// Sub-samples per pixel per axis.
    const SUPERSAMPLE: usize = 4;

    /// Paint `glyph` tinted with `color`, scaled to fit `rect`.
    pub fn paint(painter: &egui::Painter, rect: Rect, glyph: Glyph, color: Color32) {
        let texture = texture_for(painter.ctx(), glyph);
        let side = rect.width().min(rect.height());
        let square = Rect::from_center_size(rect.center(), egui::vec2(side, side));
        painter.image(texture.id(), square, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), color);
    }

    /// The glyph's cached alpha texture, rasterised on first use.
    fn texture_for(ctx: &egui::Context, glyph: Glyph) -> TextureHandle {
        let key = egui::Id::new(("dirstats-icon", glyph));
        if let Some(texture) = ctx.data(|d| d.get_temp::<TextureHandle>(key)) {
            return texture;
        }
        let alpha = rasterise(glyph.path(), glyph.mirrored());
        let pixels: Vec<Color32> = alpha.into_iter().map(Color32::from_white_alpha).collect();
        let image = egui::ColorImage { size: [TEXTURE_SIDE, TEXTURE_SIDE], source_size: egui::vec2(TEXTURE_SIDE as f32, TEXTURE_SIDE as f32), pixels };
        let texture = ctx.load_texture(format!("icon-{glyph:?}"), image, TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(key, texture.clone()));
        texture
    }

    /// Even-odd scanline coverage of the path at `TEXTURE_SIDE` square.
    fn rasterise(d: &str, mirrored: bool) -> Vec<u8> {
        let rings = flatten_svg_path(d);
        // Edges in texture sub-sample space.
        let scale = (TEXTURE_SIDE * SUPERSAMPLE) as f32 / 960.0;
        let mut edges: Vec<((f32, f32), (f32, f32))> = Vec::new();
        for ring in &rings {
            for i in 0..ring.len() {
                let (ax, ay) = ring[i];
                let (bx, by) = ring[(i + 1) % ring.len()];
                let fx = |x: f32| if mirrored { 960.0 - x } else { x } * scale;
                let fy = |y: f32| (y + 960.0) * scale;
                edges.push(((fx(ax), fy(ay)), (fx(bx), fy(by))));
            }
        }
        let samples = TEXTURE_SIDE * SUPERSAMPLE;
        let mut coverage = vec![0u32; TEXTURE_SIDE * TEXTURE_SIDE];
        let mut crossings: Vec<f32> = Vec::new();
        for sy in 0..samples {
            let y = sy as f32 + 0.5;
            crossings.clear();
            for &((ax, ay), (bx, by)) in &edges {
                if (ay <= y) != (by <= y) {
                    crossings.push(ax + (y - ay) / (by - ay) * (bx - ax));
                }
            }
            crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for pair in crossings.chunks(2) {
                if pair.len() < 2 {
                    break;
                }
                let (x0, x1) = (pair[0].max(0.0), pair[1].min(samples as f32));
                let mut sx = x0.floor() as usize;
                while (sx as f32 + 0.5) < x1 && sx < samples {
                    if sx as f32 + 0.5 >= x0 {
                        coverage[(sy / SUPERSAMPLE) * TEXTURE_SIDE + sx / SUPERSAMPLE] += 1;
                    }
                    sx += 1;
                }
            }
        }
        let full = (SUPERSAMPLE * SUPERSAMPLE) as u32;
        coverage.into_iter().map(|c| (c.min(full) * 255 / full) as u8).collect()
    }

    /// Parse and flatten SVG path syntax (M, L, H, V, Q, T, Z, absolute or
    /// relative) into closed rings in path units.
    fn flatten_svg_path(d: &str) -> Vec<Vec<(f32, f32)>> {
        struct State {
            rings: Vec<Vec<(f32, f32)>>,
            ring: Vec<(f32, f32)>,
            x: f32,
            y: f32,
            sx: f32,
            sy: f32,
            last_ctrl: Option<(f32, f32)>,
        }
        impl State {
            fn close(&mut self) {
                // A path that draws back to its start would repeat the first point.
                if self.ring.len() > 1 && self.ring.first() == self.ring.last() {
                    self.ring.pop();
                }
                if self.ring.len() > 2 {
                    self.rings.push(std::mem::take(&mut self.ring));
                }
                self.ring.clear();
            }
            fn quad(&mut self, cx: f32, cy: f32, ex: f32, ey: f32) {
                let (x0, y0) = (self.x, self.y);
                for i in 1..=8 {
                    let t = i as f32 / 8.0;
                    let u = 1.0 - t;
                    self.ring.push((u * u * x0 + 2.0 * u * t * cx + t * t * ex, u * u * y0 + 2.0 * u * t * cy + t * t * ey));
                }
                self.x = ex;
                self.y = ey;
                self.last_ctrl = Some((cx, cy));
            }
            fn apply(&mut self, cmd: char, nums: &[f32]) {
                let rel = cmd.is_ascii_lowercase();
                let abs = |s: &State, dx: f32, dy: f32| if rel { (s.x + dx, s.y + dy) } else { (dx, dy) };
                match cmd.to_ascii_uppercase() {
                    'M' => {
                        for (i, pair) in nums.chunks(2).enumerate() {
                            let (nx, ny) = abs(self, pair[0], pair[1]);
                            if i == 0 {
                                self.close();
                                self.sx = nx;
                                self.sy = ny;
                            }
                            self.x = nx;
                            self.y = ny;
                            self.ring.push((nx, ny));
                        }
                        self.last_ctrl = None;
                    }
                    'L' => {
                        for pair in nums.chunks(2) {
                            let (nx, ny) = abs(self, pair[0], pair[1]);
                            self.x = nx;
                            self.y = ny;
                            self.ring.push((nx, ny));
                        }
                        self.last_ctrl = None;
                    }
                    'H' => {
                        for &v in nums {
                            self.x = if rel { self.x + v } else { v };
                            self.ring.push((self.x, self.y));
                        }
                        self.last_ctrl = None;
                    }
                    'V' => {
                        for &v in nums {
                            self.y = if rel { self.y + v } else { v };
                            self.ring.push((self.x, self.y));
                        }
                        self.last_ctrl = None;
                    }
                    'Q' => {
                        for q in nums.chunks(4) {
                            let (cx, cy) = abs(self, q[0], q[1]);
                            let (ex, ey) = abs(self, q[2], q[3]);
                            self.quad(cx, cy, ex, ey);
                        }
                    }
                    'T' => {
                        for pair in nums.chunks(2) {
                            let (ex, ey) = abs(self, pair[0], pair[1]);
                            // Reflect the previous control point through the current point.
                            let (cx, cy) = self.last_ctrl.map_or((self.x, self.y), |(px, py)| (2.0 * self.x - px, 2.0 * self.y - py));
                            self.quad(cx, cy, ex, ey);
                        }
                    }
                    'Z' => {
                        self.x = self.sx;
                        self.y = self.sy;
                        self.close();
                        self.last_ctrl = None;
                    }
                    _ => {}
                }
            }
        }

        let mut state = State { rings: Vec::new(), ring: Vec::new(), x: 0.0, y: 0.0, sx: 0.0, sy: 0.0, last_ctrl: None };
        let mut cmd = 'M';
        let mut nums: Vec<f32> = Vec::new();
        let mut token = String::new();
        let flush = |token: &mut String, nums: &mut Vec<f32>| {
            if !token.is_empty() {
                nums.push(token.parse().unwrap_or(0.0));
                token.clear();
            }
        };
        for c in d.chars() {
            if c.is_ascii_alphabetic() {
                flush(&mut token, &mut nums);
                state.apply(cmd, &nums);
                nums.clear();
                cmd = c;
                if cmd.eq_ignore_ascii_case(&'z') {
                    state.apply(cmd, &[]);
                    cmd = 'M';
                }
            } else if c == ',' || c.is_whitespace() || (c == '-' && !token.is_empty()) || (c == '.' && token.contains('.')) {
                // Separator, or the start of a new number packed against the last.
                flush(&mut token, &mut nums);
                if c == '-' || c == '.' {
                    token.push(c);
                }
            } else {
                token.push(c);
            }
        }
        flush(&mut token, &mut nums);
        state.apply(cmd, &nums);
        state.close();
        state.rings
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn chevron_flattens_to_one_ring_of_six_points() {
            let rings = flatten_svg_path(Glyph::ChevronRight.path());
            assert_eq!(rings.len(), 1);
            assert_eq!(rings[0].len(), 6);
        }

        #[test]
        fn packed_numbers_parse() {
            // "-56-56" is two numbers; "23.5-56.5" too.
            let rings = flatten_svg_path("M0 0l-56-56 23.5-56.5Z");
            assert_eq!(rings[0], vec![(0.0, 0.0), (-56.0, -56.0), (-32.5, -112.5)]);
        }

        #[test]
        fn holes_stay_clear_and_solids_fill() {
            // Copy glyph: the front sheet is a ring with a rectangular hole.
            let alpha = rasterise(Glyph::ContentCopy.path(), false);
            let at = |x: usize, y: usize| alpha[y * TEXTURE_SIDE + x];
            // Centre of the front sheet is inside its hole.
            assert_eq!(at(27, 30), 0);
            // On the sheet's left border stroke.
            assert!(at(15, 30) > 200, "{}", at(15, 30));
            // Outside everything.
            assert_eq!(at(1, 1), 0);
        }
    }
}

/// Open the window and run until it is closed.
pub fn run(app: App) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_title("dirstats"),
        ..Default::default()
    };
    eframe::run_native(
        "dirstats",
        options,
        Box::new(|cc| {
            apply_theme(&cc.egui_ctx);
            Ok(Box::new(Gui::new(app)))
        }),
    )
}

/// Row fill under the pointer: the panel colour nudged toward the text
/// colour, so it reads as a hover on either theme and keeps text and weak
/// text at or above 4.5:1.
fn hover_fill(visuals: &egui::Visuals) -> Color32 {
    visuals.panel_fill.lerp_to_gamma(visuals.text_color(), 0.12)
}

/// Colour of a disabled icon button: about 3:1 or better against the panel on either theme.
fn disabled_icon(visuals: &egui::Visuals) -> Color32 {
    visuals.panel_fill.lerp_to_gamma(visuals.text_color(), 0.55)
}

/// Both themes with text contrast at WCAG 2.1 AA or better. egui's dark
/// defaults give 5.1:1 for text and about 2.7:1 for weak text; the light
/// defaults are fine for text but weak text is around 3:1.
fn apply_theme(ctx: &egui::Context) {
    let mut dark = egui::Visuals::dark();
    dark.widgets.noninteractive.fg_stroke.color = Color32::from_gray(210); // 11:1 on gray 27
    dark.widgets.inactive.fg_stroke.color = Color32::from_gray(210);
    dark.weak_text_color = Some(Color32::from_gray(156)); // 6.3:1 on the panel, 4.8:1 on a hovered row
    dark.selection.stroke.color = Color32::WHITE; // 7.4:1 on the selection blue
    ctx.set_visuals_of(egui::Theme::Dark, dark);

    let mut light = egui::Visuals::light();
    light.widgets.noninteractive.fg_stroke.color = Color32::from_gray(50); // 12:1 on gray 248
    light.widgets.inactive.fg_stroke.color = Color32::from_gray(50);
    light.weak_text_color = Some(Color32::from_gray(95)); // 6.1:1 on the panel, 4.9:1 on a hovered row
    light.selection.stroke.color = Color32::from_gray(20); // 11:1 on the light selection blue
    ctx.set_visuals_of(egui::Theme::Light, light);
}

struct Gui {
    app: App,
    colors: Option<ExtensionColors>,
    /// Bumped whenever a new tree arrives so cached renders are invalidated.
    tree_version: u64,
    map: Option<Treemap>,
    map_key: Option<MapKey>,
    texture: Option<TextureHandle>,
    /// Transparent overlay the size of the map; the hovered target's leaves
    /// are re-shaded vivid into it (cushions intact) and it is blended over
    /// the base with a pulsing alpha.
    highlight: Option<TextureHandle>,
    /// Region of `highlight` currently holding pixels, cleared on the next change.
    highlight_bounds: Option<dirstats_treemap::Rect>,
    highlight_key: Option<HighlightKey>,
    style: Style,
    /// Current selection: a node or an extension, never both.
    selection: Option<Selection>,
    /// User-adjustable column widths; `None` until the font is known.
    columns: Option<Columns>,
    /// Optional tree columns the user has switched on.
    show: ShownColumns,
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
    /// When the current footer message appeared, so it outranks the hover
    /// path for a while; failures stay until the next action.
    message_since: Option<(std::time::Instant, String)>,
}

/// Which optional columns are shown. Name and Extension are always there.
/// The counts and date start off so the plain layout is what opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShownColumns {
    bar: bool,
    share: bool,
    size: bool,
    items: bool,
    files: bool,
    dirs: bool,
    modified: bool,
    ext_share: bool,
    ext_size: bool,
}

impl Default for ShownColumns {
    fn default() -> Self {
        Self { bar: true, share: true, size: true, items: false, files: false, dirs: false, modified: false, ext_share: true, ext_size: true }
    }
}

/// Widths of every column in the flat header. The treemap takes whatever is
/// left between the size and extensions columns.
#[derive(Clone, Copy, Debug)]
struct Columns {
    name: f32,
    bar: f32,
    share: f32,
    size: f32,
    items: f32,
    files: f32,
    dirs: f32,
    modified: f32,
    ext_name: f32,
    ext_share: f32,
    ext_size: f32,
}

impl Columns {
    const MIN: Columns = Columns {
        name: 80.0,
        bar: 24.0,
        share: 40.0,
        size: 56.0,
        items: 40.0,
        files: 40.0,
        dirs: 40.0,
        modified: 60.0,
        ext_name: 60.0,
        ext_share: 40.0,
        ext_size: 56.0,
    };
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
            items: mono_char * 8.0,
            files: mono_char * 8.0,
            dirs: mono_char * 7.0,
            modified: mono_char * 17.0,
            ext_name: 140.0,
            ext_share: mono_char * 6.0,
            ext_size: mono_char * 10.0,
        }
    }

    /// Width of the tree columns that are shown.
    fn tree_width(&self, show: ShownColumns) -> f32 {
        let optional = [
            (show.bar, self.bar),
            (show.share, self.share),
            (show.size, self.size),
            (show.items, self.items),
            (show.files, self.files),
            (show.dirs, self.dirs),
            (show.modified, self.modified),
        ];
        self.name + optional.iter().filter(|(on, _)| *on).map(|(_, w)| w).sum::<f32>()
    }

    /// Width of the extension columns that are shown.
    fn extensions_width(&self, show: ShownColumns) -> f32 {
        self.ext_name + if show.ext_share { self.ext_share } else { 0.0 } + if show.ext_size { self.ext_size } else { 0.0 }
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
            highlight_bounds: None,
            highlight_key: None,
            style: Style::Squarified,
            selection: None,
            columns: None,
            show: ShownColumns::default(),
            scroll_to: None,
            hovered_highlight: None,
            next_highlight: None,
            hover_active: true,
            menu_node: None,
            pending_copy: None,
            message_since: None,
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
            // Fresh, fully transparent overlay at the new size.
            let clear = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &vec![0; width as usize * height as usize * 4]);
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
        let leaves: Vec<(usize, dirstats_treemap::Oklch)> = match &target {
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
        let union = |a: dirstats_treemap::Rect, b: dirstats_treemap::Rect| {
            dirstats_treemap::Rect::new(a.left.min(b.left), a.top.min(b.top), a.right.max(b.right), a.bottom.max(b.bottom))
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

    fn trash_state(&self, node: NodeId) -> TrashState {
        if self.app.can_put_back(node) {
            TrashState::CanPutBack
        } else if self.app.is_trashed(node) {
            TrashState::Trashed
        } else {
            TrashState::Present
        }
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
            #[cfg(feature = "trash")]
            NodeAction::PutBack => {
                if let Err(err) = self.app.put_back(node) {
                    self.app.message = Some(format!("put back failed: {err}"));
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
        let show = self.show;
        let total_fixed = columns.tree_width(show) + columns.extensions_width(show);
        let map_width = (full.width() - total_fixed).max(Columns::MIN_MAP);
        // Hidden columns take zero width and draw no title or divider.
        let on = |shown: bool, w: f32| if shown { w } else { 0.0 };
        let widths = [
            columns.name,
            on(show.bar, columns.bar),
            on(show.share, columns.share),
            on(show.size, columns.size),
            on(show.items, columns.items),
            on(show.files, columns.files),
            on(show.dirs, columns.dirs),
            on(show.modified, columns.modified),
            map_width,
            columns.ext_name,
            on(show.ext_share, columns.ext_share),
            on(show.ext_size, columns.ext_size),
        ];
        let titles = ["Name", "", "%", "Size", "Items", "Files", "Dirs", "Modified", "Treemap", "Extension", "%", "Size"];
        let right_aligned = [false, false, true, true, true, true, true, true, false, false, true, true];
        const MAP: usize = 8;
        let mut x = full.min.x;
        let mut starts = [0.0; 12];
        for (i, &w) in widths.iter().enumerate() {
            starts[i] = x;
            if w <= 0.0 {
                continue;
            }
            let cell = egui::Rect::from_min_size(egui::pos2(x, header.min.y), egui::vec2(w, header.height()));
            // Column picker at the left edge of the treemap header, eating into its space.
            let cell = if i == MAP { self.column_picker(ui, cell) } else { cell };
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
                let range = if i == MAP - 1 || i == MAP { full.y_range() } else { header.y_range() };
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
                        4 => (&mut columns.items, min.items, 1.0),
                        5 => (&mut columns.files, min.files, 1.0),
                        6 => (&mut columns.dirs, min.dirs, 1.0),
                        7 => (&mut columns.modified, min.modified, 1.0),
                        8 => (&mut columns.ext_name, min.ext_name, -1.0),
                        9 => (&mut columns.ext_name, min.ext_name, 1.0),
                        _ => (&mut columns.ext_share, min.ext_share, 1.0),
                    };
                    *col = (*col + sign * delta).max(min_w);
                    // Keep the treemap from being squeezed out.
                    let overflow = columns.tree_width(show) + columns.extensions_width(show) + Columns::MIN_MAP - full.width();
                    if overflow > 0.0 {
                        let col = match i {
                            0 => &mut columns.name,
                            1 => &mut columns.bar,
                            2 => &mut columns.share,
                            3 => &mut columns.size,
                            4 => &mut columns.items,
                            5 => &mut columns.files,
                            6 => &mut columns.dirs,
                            7 => &mut columns.modified,
                            8 | 9 => &mut columns.ext_name,
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
        let tree_rect = region(0, MAP - 1);
        let map_rect = region(MAP, MAP);
        let ext_rect = region(MAP + 1, 11);

        let mut tree_ui = ui.new_child(egui::UiBuilder::new().max_rect(tree_rect).id_salt("tree"));
        tree_ui.set_clip_rect(tree_rect);
        let mut tree_columns = [0.0; 9];
        tree_columns.copy_from_slice(&starts[..=MAP]);
        self.entry_list(&mut tree_ui, tree_columns, row_height);

        let mut map_ui = ui.new_child(egui::UiBuilder::new().max_rect(map_rect).id_salt("map"));
        map_ui.set_clip_rect(map_rect);
        self.treemap(&mut map_ui);

        let mut ext_ui = ui.new_child(egui::UiBuilder::new().max_rect(ext_rect).id_salt("extensions"));
        ext_ui.set_clip_rect(ext_rect);
        let ext_edges = [starts[9], starts[10], starts[11], full.max.x];
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
    /// Icon button at the left of `cell` opening a checklist of optional
    /// columns; returns the part of `cell` left for the title.
    fn column_picker(&mut self, ui: &mut egui::Ui, cell: egui::Rect) -> egui::Rect {
        let size = cell.height();
        let button = egui::Rect::from_min_size(cell.min, egui::vec2(size, size));
        let response = ui.interact(button, ui.id().with("column-picker"), Sense::click()).on_hover_text("Columns");
        let visuals = ui.style().interact(&response);
        if response.hovered() || response.is_pointer_button_down_on() {
            ui.painter().rect_filled(button.shrink(2.0), 4.0, visuals.weak_bg_fill);
        }
        icons::paint(ui.painter(), button.shrink(5.0), icons::Glyph::ViewColumn, visuals.text_color());
        let mut show = self.show;
        egui::Popup::menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(150.0);
            ui.label(egui::RichText::new("Tree").weak().small());
            ui.checkbox(&mut show.bar, "Bar");
            ui.checkbox(&mut show.share, "%");
            ui.checkbox(&mut show.size, "Size");
            ui.checkbox(&mut show.items, "Items");
            ui.checkbox(&mut show.files, "Files");
            ui.checkbox(&mut show.dirs, "Dirs");
            ui.checkbox(&mut show.modified, "Modified");
            ui.separator();
            ui.label(egui::RichText::new("Extensions").weak().small());
            ui.checkbox(&mut show.ext_share, "%");
            ui.checkbox(&mut show.ext_size, "Size");
        });
        self.show = show;
        egui::Rect::from_min_max(egui::pos2(button.max.x, cell.min.y), cell.max)
    }

    /// Toolbar: back, breadcrumbs, totals; layout toggle and rescan on the right.
    fn header(&mut self, ui: &mut egui::Ui) {
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
                    if icon_button(ui, icons::Glyph::ChevronLeft, can_back, "Back (Backspace)").clicked() {
                        zoom_target = Some(None);
                    }
                    let crumbs = self.app.breadcrumbs();
                    for (i, &id) in crumbs.iter().enumerate() {
                        if i > 0 {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                            icons::paint(ui.painter(), rect, icons::Glyph::ChevronRight, ui.visuals().weak_text_color());
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
    /// bar, share, size, items, files, dirs and modified columns and the
    /// right edge of modified, straight from the header, so cells always
    /// line up with it.
    fn entry_list(&mut self, ui: &mut egui::Ui, edges: [f32; 9], row_height: f32) {
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

        // Computed before the row closure, which cannot borrow the app mutably.
        let trashed_rows: Vec<bool> = rows.iter().map(|&(id, _)| self.app.is_trashed(id)).collect();
        let trash_states: Vec<TrashState> = rows.iter().map(|&(id, _)| self.trash_state(id)).collect();
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
            for (row_index, &(id, depth)) in rows.iter().enumerate().take(range.end).skip(range.start) {
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
                    ui.painter().rect_filled(row_rect, 0.0, hover_fill(ui.visuals()));
                }
                let trashed = trashed_rows[row_index];
                let text = if is_selected {
                    ui.visuals().selection.stroke.color
                } else if trashed {
                    ui.visuals().weak_text_color()
                } else {
                    ui.visuals().text_color()
                };
                let (top, bottom) = (row_rect.min.y, row_rect.max.y);
                let cell = |from: f32, to: f32| egui::Rect::from_min_max(egui::pos2(from, top), egui::pos2(to, bottom));

                // Name column: indent, expander, then a truncating label clipped to the column.
                let name_cell = cell(edges[0], edges[1]);
                let expander_rect = egui::Rect::from_min_size(
                    egui::pos2(edges[0] + pad + indent * depth as f32, top),
                    egui::vec2(18.0, row_height),
                );
                if is_dir {
                    let glyph = if self.app.expanded.contains(&id) { icons::Glyph::ExpandMore } else { icons::Glyph::ChevronRight };
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
                    let mut rich = egui::RichText::new(name).color(text);
                    if trashed {
                        rich = rich.strikethrough();
                    }
                    name_ui.add(egui::Label::new(rich).truncate().selectable(false));
                }

                // Bar column.
                let bar = cell(edges[1], edges[2]).shrink2(egui::vec2(pad, 5.0));
                if edges[2] - edges[1] > 0.0 && bar.width() > 0.0 {
                    ui.painter().rect_filled(bar, 2.0, ui.visuals().faint_bg_color);
                    let mut filled = bar;
                    filled.set_width(bar.width() * (share / 100.0) as f32);
                    ui.painter().rect_filled(filled, 2.0, ui.visuals().weak_text_color());
                }

                // Share and size: right-aligned monospace, clipped to their cells.
                // Counts and dates are shown for directories; files get blanks
                // there, as WinDirStat does.
                let count = |n: u64| if is_dir { n.to_string() } else { String::new() };
                let figures = [
                    (edges[2], edges[3], format!("{share:.1}")),
                    (edges[3], edges[4], format::size(size)),
                    (edges[4], edges[5], count(node.file_count + node.dir_count)),
                    (edges[5], edges[6], count(node.file_count)),
                    (edges[6], edges[7], count(node.dir_count)),
                    (edges[7], edges[8], node.modified.map(format_time).unwrap_or_default()),
                ];
                for (from, to, value) in figures {
                    let c = cell(from, to);
                    if c.width() <= 0.0 {
                        continue;
                    }
                    let galley = ui.painter().with_clip_rect(c).text(
                        egui::pos2(c.max.x - pad, c.center().y),
                        egui::Align2::RIGHT_CENTER,
                        value,
                        mono.clone(),
                        text,
                    );
                    if trashed {
                        ui.painter().with_clip_rect(c).hline(galley.x_range(), galley.center().y, egui::Stroke::new(1.0_f32, text));
                    }
                }

                if row.clicked() || row.secondary_clicked() {
                    select = Some(id);
                }
                let path = tree.path(id);
                let trash_state = trash_states[row_index];
                row.context_menu(|ui| {
                    if let Some(action) = node_menu(ui, &path, is_dir, trash_state) {
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
        let origin = response.rect.min;
        let painter = ui.painter_at(response.rect);
        let to_screen = |r: dirstats_treemap::Rect| {
            egui::Rect::from_min_max(
                origin + egui::vec2(r.left as f32, r.top as f32),
                origin + egui::vec2(r.right as f32, r.bottom as f32),
            )
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
        // Trashed boxes are hollowed out: panel fill with a faint outline, so the
        // space they took is visible but empty until the next rescan.
        if !self.app.trashed.is_empty() {
            let fill = ui.visuals().panel_fill;
            let edge = egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color);
            for item in map.items.iter().filter(|item| item.leaf) {
                if self.app.is_trashed(item.node) {
                    let r = to_screen(item.rect);
                    painter.rect_filled(r, 0.0, fill);
                    painter.rect_stroke(r, 0.0, edge, egui::StrokeKind::Inside);
                }
            }
        }
        let outline = |item: &dirstats_treemap::render::VisibleItem, color: Color32, width: f32| {
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
                        if node.kind != dirstats_app::scan::Kind::Directory && ExtensionColors::extension(node) == *ext {
                            outline(item, Color32::WHITE, 2.0);
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
            let trash_state = self.trash_state(node);
            let mut action = None;
            let menu = response.context_menu(|ui| action = node_menu(ui, &path, is_dir, trash_state));
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

