// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.


//! The flat column header: widths, which columns show, the draggable
//! dividers and the regions laid out beneath them.

use eframe::egui::{self, Sense};

use crate::{Gui, icons};

/// Which optional columns are shown. Name and Extension are always there.
/// The counts and date start off so the plain layout is what opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ShownColumns {
    /// Share of the parent as a bar split by extension.
    pub(super) bar: bool,
    /// Share of the parent in percent.
    pub(super) share: bool,
    pub(super) size: bool,
    /// Files and folders below, together.
    pub(super) items: bool,
    pub(super) files: bool,
    /// Folders below.
    pub(super) dirs: bool,
    pub(super) modified: bool,
    /// Extension's share of the whole tree in percent.
    pub(super) ext_share: bool,
    /// Extension's total size.
    pub(super) ext_size: bool,
}

impl Default for ShownColumns {
    fn default() -> Self {
        Self { bar: true, share: true, size: true, items: false, files: false, dirs: false, modified: false, ext_share: true, ext_size: true }
    }
}

/// Widths of every column in the flat header, in points, kept for hidden
/// columns too. The treemap takes whatever is left between the tree and
/// extension columns, and at least [`Columns::MIN_MAP`].
#[derive(Clone, Copy, Debug)]
pub(super) struct Columns {
    pub(super) name: f32,
    pub(super) bar: f32,
    pub(super) share: f32,
    pub(super) size: f32,
    pub(super) items: f32,
    pub(super) files: f32,
    pub(super) dirs: f32,
    pub(super) modified: f32,
    pub(super) ext_name: f32,
    pub(super) ext_share: f32,
    pub(super) ext_size: f32,
}

impl Columns {
    /// Narrowest each column can be dragged, unless the treemap needs the room.
    pub(super) const MIN: Columns = Columns {
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
    pub(super) const MIN_MAP: f32 = 120.0;
    /// Width of the draggable divider between columns.
    pub(super) const DIVIDER: f32 = 6.0;

    /// Starting widths for a body `window` wide: the name column 22% of it,
    /// kept within 160 to 420, and figures sized in digits of `mono_char`,
    /// the width of a monospace `0`.
    pub(super) fn initial(window: f32, mono_char: f32) -> Self {
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

    /// Width of the tree columns that are shown, name included.
    pub(super) fn tree_width(&self, show: ShownColumns) -> f32 {
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

    /// Width of the extension columns that are shown, name included.
    pub(super) fn extensions_width(&self, show: ShownColumns) -> f32 {
        self.ext_name + if show.ext_share { self.ext_share } else { 0.0 } + if show.ext_size { self.ext_size } else { 0.0 }
    }
}

impl Gui {
    /// One flat header across the window, then the tree, treemap and
    /// extensions laid out under their columns, and the location picker over
    /// them before the first scan. Also commits this frame's hover highlight.
    pub(super) fn body(&mut self, ui: &mut egui::Ui) {
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
        let titles = ["Name", "", "%", "Size", "Items", "Files", "Folders", "Modified", "Treemap", "Extension", "%", "Size"];
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
                // the file list the header tick is enough. The tree's last
                // visible column may not be the one right before the map.
                let before_map = i < MAP && widths[i + 1..MAP].iter().all(|&w| w <= 0.0);
                let range = if before_map || i == MAP { full.y_range() } else { header.y_range() };
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

        // Before the first scan the location picker floats over the middle of
        // the whole body rather than sitting in the treemap column.
        if self.app.scan.is_none() && self.app.tree.is_none() {
            let mut overlay = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt("location-picker"));
            self.location_picker(&mut overlay);
        }

        // The treemap is drawn before the legend, so hover takes effect next frame.
        if self.hovered_highlight != self.next_highlight {
            self.hovered_highlight = self.next_highlight.take();
            ui.ctx().request_repaint();
        } else {
            self.next_highlight = None;
        }
    }

    /// Icon button at the left of `cell` opening a checklist of optional
    /// columns; returns the part of `cell` left for the title.
    pub(super) fn column_picker(&mut self, ui: &mut egui::Ui, cell: egui::Rect) -> egui::Rect {
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
            ui.set_min_width(190.0);
            // Menu labels are descriptive; the header keeps the short forms.
            ui.label(egui::RichText::new("Tree").weak().small());
            ui.checkbox(&mut show.bar, "Share bar");
            ui.checkbox(&mut show.share, "Share of parent (%)");
            ui.checkbox(&mut show.size, "Size");
            ui.checkbox(&mut show.items, "Item count");
            ui.checkbox(&mut show.files, "File count");
            ui.checkbox(&mut show.dirs, "Subfolder count");
            ui.checkbox(&mut show.modified, "Last modified");
            ui.separator();
            ui.label(egui::RichText::new("Extensions").weak().small());
            ui.checkbox(&mut show.ext_share, "Share of total (%)");
            ui.checkbox(&mut show.ext_size, "Total size");
        });
        self.show = show;
        egui::Rect::from_min_max(egui::pos2(button.max.x, cell.min.y), cell.max)
    }
}
