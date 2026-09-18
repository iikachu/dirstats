// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Graphical front end: an entry list, a glow treemap and an extension
//! legend, driven entirely by [`dirstats_app::App`].

use dirstats_app::{App, NodeId};
use dirstats_treemap::render::{ExtensionColors, ExtensionMix};
use dirstats_treemap::{Style, Treemap};
use eframe::egui::{self, Key, TextureHandle};

mod actions;
mod chrome;
mod columns;
#[cfg(all(any(windows, target_os = "linux"), feature = "trash"))]
mod dialogs;
#[cfg(all(test, feature = "e2e"))]
mod e2e;
mod entries;
mod icons;
mod legend;
mod menu;
mod picker;
mod theme;
mod treemap;

use columns::{Columns, ShownColumns};
#[cfg(all(any(windows, target_os = "linux"), feature = "trash"))]
use dialogs::Dialog;
use theme::{MONO_STEP, apply_theme, system_fonts, system_text_sizes};

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

/// Local date and time, minute precision.
fn format_time(time: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time).format("%Y-%m-%d %H:%M").to_string()
}

/// Extensions drawn as coloured segments in a share bar; the rest is the plain bar.
const BAR_SEGMENTS: usize = 6;

/// The name the app goes by: the window title, and the toolbar before a scan.
const APP_NAME: &str = "dirstats";

/// Open the window and run until it is closed.
pub fn run(app: App) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_title(APP_NAME),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| {
            configure(&cc.egui_ctx);
            Ok(Box::new(Gui::new(app)))
        }),
    )
}

/// Fonts, text sizes and theme, set once before the first frame.
fn configure(ctx: &egui::Context) {
    // The egui-fonts feature keeps egui's bundled fonts and sizes for comparison.
    if !cfg!(feature = "egui-fonts") {
        let (fonts, system) = system_fonts();
        ctx.set_fonts(fonts);
        if system {
            ctx.all_styles_mut(|style| style.text_styles = system_text_sizes());
        }
    }
    // Monospace faces (Hack, SF Mono, Cascadia) all carry a taller
    // x-height than their proportional partners, so the numeric
    // columns sit a step below body text whichever fonts are in use.
    ctx.all_styles_mut(|style| {
        let body = style.text_styles[&egui::TextStyle::Body].size;
        style.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(body - MONO_STEP));
    });
    apply_theme(ctx);
}

struct Gui {
    app: App,
    colors: Option<ExtensionColors>,
    /// Largest extensions below every node, for the share bar segments.
    mix: Option<ExtensionMix>,
    /// Places offered when nothing is being scanned, listed on first show.
    locations: Option<Vec<dirstats_app::locations::Location>>,
    /// Probed with the locations; `Some(false)` earns a hint in the picker
    /// and on the scanning screen.
    full_disk_access: Option<bool>,
    /// iCloud status of nodes drawn so far; cleared with the tree and after
    /// an eviction.
    cloud: std::collections::HashMap<NodeId, dirstats_app::cloud::CloudStatus>,
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
    /// Modal in front of everything, if any.
    #[cfg(all(any(windows, target_os = "linux"), feature = "trash"))]
    dialog: Option<Dialog>,
}

impl Gui {
    fn new(app: App) -> Self {
        Self {
            app,
            colors: None,
            mix: None,
            locations: None,
            full_disk_access: dirstats_app::locations::full_disk_access(),
            cloud: std::collections::HashMap::new(),
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
            #[cfg(all(any(windows, target_os = "linux"), feature = "trash"))]
            dialog: None,
        }
    }

    fn tree_changed(&mut self) {
        self.tree_version += 1;
        self.selection = None;
        self.cloud.clear();
        self.colors = self.app.tree.as_ref().map(ExtensionColors::rank);
        self.mix = match (&self.app.tree, &self.colors) {
            (Some(tree), Some(colors)) => Some(colors.mix(tree, BAR_SEGMENTS)),
            _ => None,
        };
        self.map = None;
        self.map_key = None;
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
        #[cfg(all(any(windows, target_os = "linux"), feature = "trash"))]
        self.dialogs(ctx);
        if let Some(text) = self.pending_copy.take() {
            ctx.copy_text(text);
        }
    }
}
