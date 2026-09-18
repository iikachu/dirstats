// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.


//! The entry list: the zoom directory's row, the expandable tree under it and keyboard navigation.

use dirstats_app::{NodeId, format};
use eframe::egui::{self, Color32, Key, Sense};

use crate::menu::{NodeAction, TrashState, node_menu, zoom_label};
use crate::theme::hover_fill;
use crate::{Gui, Highlight, Selection, format_time, icons};

impl Gui {
    /// Keyboard navigation in the tree. Up and down move through the visible
    /// rows; Home and End (or Cmd+Up/Down on macOS, Ctrl+Home/End elsewhere)
    /// jump to the ends; Page Up and Page Down (or Option+Up/Down on macOS)
    /// move by a screenful; right expands a directory or steps into its first
    /// child; left collapses it or steps to the parent. `rows` is refreshed
    /// when the expansion changes.
    pub(super) fn keyboard_navigation(&mut self, ui: &egui::Ui, rows: &mut Vec<(NodeId, u32)>, row_step: f32) {
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
    /// The current directory as a fixed row above the list: its figures in
    /// the usual columns, and the name cell holding the path as crumbs.
    /// Ancestors are clickable and return the directory to zoom to; the row
    /// itself takes no selection, hover, keyboard or menu.
    pub(super) fn current_dir_row(&self, ui: &mut egui::Ui, edges: [f32; 9], row_height: f32) -> Option<NodeId> {
        let tree = self.app.tree.as_ref()?;
        let dir = self.app.dir()?;
        let node = tree.node(dir);
        let size = tree.size(dir);
        let parent_size = node.parent.map_or(size, |p| tree.size(p));
        let share = format::percent(size, parent_size);
        let pad = 6.0;
        let mono = egui::TextStyle::Monospace.resolve(ui.style());
        // The background is painted once the wrapped crumbs have said how
        // tall the row is, but has to sit beneath them.
        let background = ui.painter().add(egui::Shape::Noop);
        let origin = ui.cursor().min;
        let row_width = ui.available_width();
        let text = ui.visuals().text_color();

        // Crumbs from the left, wrapping onto more lines when the path is
        // longer than the name column. The cell is one line tall: a wrapping
        // layout takes its rect's height as the height of the first line and
        // grows downward from there.
        let inset = 3.0;
        let name_cell = egui::Rect::from_min_max(egui::pos2(edges[0] + pad, origin.y + inset), egui::pos2(edges[1] - pad, origin.y + row_height - inset));
        let mut target = None;
        let mut crumbs_height = 0.0;
        if name_cell.width() > 4.0 {
            let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
            let mut crumb_ui = ui.new_child(egui::UiBuilder::new().max_rect(name_cell).layout(layout));
            let clip = ui.clip_rect();
            // Clip to the column, but leave the cell's padding for the hover
            // pill, which reaches a little past the text on either side.
            let clip_x = name_cell.x_range().expand(pad - 1.0).intersection(clip.x_range());
            crumb_ui.set_clip_rect(egui::Rect::from_x_y_ranges(clip_x, clip.y_range()));
            crumb_ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            let crumbs = self.app.breadcrumbs();
            let body_font = egui::TextStyle::Body.resolve(crumb_ui.style());
            for (i, &id) in crumbs.iter().enumerate() {
                let mut name = tree.node(id).name.to_string_lossy().into_owned();
                if i + 1 == crumbs.len() {
                    name.push('/');
                }
                // Start a new line here rather than let the label wrap itself:
                // a label pushed whole onto the next line still claims an
                // empty piece of this one, and its rect spans both.
                // An ancestor's width includes the marker after it, so the two
                // move to the next line together and no line starts with one.
                const MARKER: f32 = 12.0;
                let marker_room = if i + 1 == crumbs.len() { 0.0 } else { crumb_ui.spacing().item_spacing.x + MARKER };
                let width = crumb_ui.painter().layout_no_wrap(name.clone(), body_font.clone(), text).size().x + marker_room;
                let at_line_start = crumb_ui.cursor().min.x <= name_cell.min.x + 0.5;
                if !at_line_start && width > crumb_ui.available_size_before_wrap().x {
                    crumb_ui.end_row();
                }
                if i + 1 == crumbs.len() {
                    crumb_ui.add(egui::Label::new(egui::RichText::new(name).strong().color(text)).wrap().selectable(false));
                } else {
                    // Held back so the hover fill can go beneath the text.
                    let fill = crumb_ui.painter().add(egui::Shape::Noop);
                    let link = crumb_ui.add(egui::Label::new(egui::RichText::new(name).color(text)).wrap().sense(Sense::click()).selectable(false));
                    if link.hovered() {
                        crumb_ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        // A pill behind the crumb, darker while pressed, and
                        // an underline as on a link.
                        let visuals = crumb_ui.visuals();
                        let strength = if link.is_pointer_button_down_on() { 0.28 } else { 0.14 };
                        let color = visuals.panel_fill.lerp_to_gamma(visuals.text_color(), strength);
                        let pill = link.rect.expand2(egui::vec2(3.0, 1.0));
                        crumb_ui.painter().set(fill, egui::Shape::rect_filled(pill, 4.0, color));
                        crumb_ui.painter().hline(link.rect.x_range(), link.rect.max.y - 1.0, egui::Stroke::new(1.0_f32, text));
                    }
                    if link.clicked() {
                        target = Some(id);
                    }
                    let (rect, _) = crumb_ui.allocate_exact_size(egui::vec2(MARKER, MARKER), Sense::hover());
                    icons::paint(crumb_ui.painter(), rect, icons::Glyph::ChevronRight, crumb_ui.visuals().weak_text_color());
                }
            }
            crumbs_height = crumb_ui.min_rect().height();
        }
        // Never so tall that the list below is squeezed out.
        let limit = (ui.available_height() * 0.5).max(row_height);
        let height = row_height.max(crumbs_height + 2.0 * inset).min(limit);
        let row_rect = egui::Rect::from_min_size(origin, egui::vec2(row_width, height));
        ui.allocate_rect(row_rect, Sense::hover());
        ui.painter().set(background, egui::Shape::rect_filled(row_rect, 0.0, ui.visuals().faint_bg_color));
        ui.painter().hline(row_rect.x_range(), row_rect.max.y, ui.visuals().widgets.noninteractive.bg_stroke);
        // Figures sit on the first line, level with the start of the path.
        let (top, bottom) = (row_rect.min.y, row_rect.min.y + row_height);
        let cell = |from: f32, to: f32| egui::Rect::from_min_max(egui::pos2(from, top), egui::pos2(to, bottom));

        let bar = cell(edges[1], edges[2]).shrink2(egui::vec2(pad, 5.0));
        if edges[2] - edges[1] > 0.0 && bar.width() > 0.0 {
            ui.painter().rect_filled(bar, 2.0, ui.visuals().extreme_bg_color);
            let mut filled = bar;
            filled.set_width(bar.width() * (share / 100.0) as f32);
            ui.painter().rect_filled(filled, 2.0, ui.visuals().weak_text_color());
        }
        let figures = [
            (edges[2], edges[3], format!("{share:.1}")),
            (edges[3], edges[4], format::size(size)),
            (edges[4], edges[5], (node.file_count + node.dir_count).to_string()),
            (edges[5], edges[6], node.file_count.to_string()),
            (edges[6], edges[7], node.dir_count.to_string()),
            (edges[7], edges[8], node.modified.map(format_time).unwrap_or_default()),
        ];
        for (from, to, value) in figures {
            let c = cell(from, to);
            if c.width() <= 0.0 {
                continue;
            }
            ui.painter().with_clip_rect(c).text(egui::pos2(c.max.x - pad, c.center().y), egui::Align2::RIGHT_CENTER, value, mono.clone(), text);
        }
        target
    }

    pub(super) fn entry_list(&mut self, ui: &mut egui::Ui, edges: [f32; 9], row_height: f32) {
        if self.app.tree.is_none() {
            return;
        }
        let mut rows = self.app.tree_rows();
        self.keyboard_navigation(ui, &mut rows, row_height + ui.spacing().item_spacing.y);
        // Computed before borrowing the tree, since the lookup caches into self.
        let cloud_states: Vec<dirstats_app::cloud::CloudStatus> = rows.iter().map(|&(id, _)| self.cloud_status(id)).collect();
        let tree = self.app.tree.as_ref().expect("checked above");
        let (mix, colors) = (self.mix.as_ref(), self.colors.as_ref());
        let selected = self.selected_node();
        let indent = 16.0;
        let pad = 6.0;
        let mono = egui::TextStyle::Monospace.resolve(ui.style());

        // Computed before the row closure, which cannot borrow the app mutably.
        let trashed_rows: Vec<bool> = rows.iter().map(|&(id, _)| self.app.is_trashed(id)).collect();
        let evicted_rows: Vec<bool> = rows.iter().map(|&(id, _)| self.app.is_evicted(id)).collect();
        let trash_states: Vec<TrashState> = rows.iter().map(|&(id, _)| self.trash_state(id)).collect();
        let permanent = self.permanent();
        let current_dir = self.app.dir();
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
        // Pinned first row: the current directory, with its ancestors as
        // clickable crumbs for moving back up the tree.
        let zoom_up = self.current_dir_row(ui, edges, row_height);

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
                // A small cloud after the name for iCloud items, crossed out
                // when only a placeholder is on disk. The truncating label
                // would take the whole cell, so its room is held back first.
                let glyph = match cloud_states[row_index] {
                    dirstats_app::cloud::CloudStatus::Local => None,
                    dirstats_app::cloud::CloudStatus::Downloaded => Some(icons::Glyph::Cloud),
                    dirstats_app::cloud::CloudStatus::Evicted => Some(icons::Glyph::CloudOff),
                };
                let icon_room = if glyph.is_some() { 18.0 } else { 0.0 };
                let text_rect = egui::Rect::from_min_max(label_rect.min, egui::pos2(label_rect.max.x - icon_room, bottom));
                if text_rect.width() > 4.0 {
                    let mut name_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    name_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
                    let mut rich = egui::RichText::new(name).color(text);
                    if trashed {
                        rich = rich.strikethrough();
                    }
                    let label = name_ui.add(egui::Label::new(rich).truncate().selectable(false));
                    if let Some(glyph) = glyph {
                        let x = (label.rect.max.x + 4.0).min(label_rect.max.x - 14.0);
                        let icon = egui::Rect::from_min_size(egui::pos2(x, row_rect.center().y - 7.0), egui::vec2(14.0, 14.0));
                        // Lighter than text, a hint beside the name; but an
                        // eviction done here is a change worth noticing, so it
                        // takes the accent colour until the next scan.
                        let color = if evicted_rows[row_index] {
                            ui.visuals().hyperlink_color
                        } else {
                            ui.visuals().weak_text_color().gamma_multiply(0.5)
                        };
                        icons::paint(&ui.painter().with_clip_rect(name_cell), icon, glyph, color);
                    }
                }

                // Bar column: the row's share of its parent, split into the
                // largest extensions below it in their treemap colours. The
                // tail of smaller extensions stays in the plain bar colour.
                let bar = cell(edges[1], edges[2]).shrink2(egui::vec2(pad, 5.0));
                if edges[2] - edges[1] > 0.0 && bar.width() > 0.0 {
                    ui.painter().rect_filled(bar, 2.0, ui.visuals().faint_bg_color);
                    let mut filled = bar;
                    filled.set_width(bar.width() * (share / 100.0) as f32);
                    ui.painter().rect_filled(filled, 2.0, ui.visuals().weak_text_color());
                    if let (Some(mix), Some(colors)) = (mix, colors)
                        && size > 0
                    {
                        let painter = ui.painter().with_clip_rect(filled);
                        let mut x = filled.min.x;
                        for &(rank, bytes) in mix.segments(id) {
                            let width = filled.width() * (bytes as f64 / size as f64) as f32;
                            let segment = egui::Rect::from_min_max(egui::pos2(x, filled.min.y), egui::pos2(x + width, filled.max.y));
                            let [r, g, b] = colors.color_at(rank as usize).to_srgb();
                            painter.rect_filled(segment, 0.0, Color32::from_rgb(r, g, b));
                            x += width;
                        }
                    }
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
                let cloud = cloud_states[row_index];
                let zoom = zoom_label(tree, current_dir, id);
                row.context_menu(|ui| {
                    if let Some(action) = node_menu(ui, &path, zoom, trash_state, permanent, cloud) {
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
        if let Some(id) = zoom_up
            && self.app.zoom_to(id)
        {
            self.map_key = None;
        }
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
}
