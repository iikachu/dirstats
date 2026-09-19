// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.


//! Material Symbols Outlined glyphs (Apache-2.0, by Google), each the
//! `d` attribute of its 24px SVG in the `0 -960 960 960` viewBox, so y runs
//! from -960 at the top to 0 at the bottom.
//! A glyph is rasterised once with an even-odd scanline fill into a
//! cached alpha texture, then drawn tinted, so paths with holes (the
//! copy sheets, the can) render exactly as designed.

use eframe::egui::{self, Color32, Rect, TextureHandle, TextureOptions};

/// An icon; each variant is the Material symbol of the same name unless
/// its doc says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)] // Open and the trash glyphs are only reachable with their features.
pub enum Glyph {
    /// `chevron_right`, a collapsed tree row.
    ChevronRight,
    /// `chevron_right` mirrored; Material's `chevron_left` is the same shape.
    ChevronLeft,
    /// `keyboard_arrow_down`, the same shape as `expand_more`.
    ExpandMore,
    /// `content_copy`, two sheets.
    ContentCopy,
    /// `open_in_new`.
    OpenInNew,
    /// `delete`, the can.
    Delete,
    /// `undo`, for putting an item back.
    Undo,
    /// Column picker.
    ViewColumn,
    /// `home`.
    Home,
    /// `storage`, a stack of drives.
    Storage,
    /// `folder`.
    Folder,
    /// Open folder, for browsing to one.
    FolderOpen,
    /// `cloud_off`.
    CloudOff,
    /// `cloud`.
    Cloud,
}

impl Glyph {
    /// SVG path data, verbatim from the vendored upstream file (a test checks).
    fn path(self) -> &'static str {
        match self {
            Glyph::ChevronRight | Glyph::ChevronLeft => "M504-480 320-664l56-56 240 240-240 240-56-56 184-184Z",
            Glyph::ExpandMore => "M480-344 240-584l56-56 184 184 184-184 56 56-240 240Z",
            Glyph::ContentCopy => "M360-240q-33 0-56.5-23.5T280-320v-480q0-33 23.5-56.5T360-880h360q33 0 56.5 23.5T800-800v480q0 33-23.5 56.5T720-240H360Zm0-80h360v-480H360v480ZM200-80q-33 0-56.5-23.5T120-160v-560h80v560h440v80H200Zm160-240v-480 480Z",
            Glyph::OpenInNew => "M200-120q-33 0-56.5-23.5T120-200v-560q0-33 23.5-56.5T200-840h280v80H200v560h560v-280h80v280q0 33-23.5 56.5T760-120H200Zm188-212-56-56 372-372H560v-80h280v280h-80v-144L388-332Z",
            Glyph::Delete => "M280-120q-33 0-56.5-23.5T200-200v-520h-40v-80h200v-40h240v40h200v80h-40v520q0 33-23.5 56.5T680-120H280Zm400-600H280v520h400v-520ZM360-280h80v-360h-80v360Zm160 0h80v-360h-80v360ZM280-720v520-520Z",
            Glyph::ViewColumn => "M121-280v-400q0-33 23.5-56.5T201-760h559q33 0 56.5 23.5T840-680v400q0 33-23.5 56.5T760-200H201q-33 0-56.5-23.5T121-280Zm79 0h133v-400H200v400Zm213 0h133v-400H413v400Zm213 0h133v-400H626v400Z",
            Glyph::Home => "M240-200h120v-240h240v240h120v-360L480-740 240-560v360Zm-80 80v-480l320-240 320 240v480H520v-240h-80v240H160Zm320-350Z",
            Glyph::Storage => "M120-160v-160h720v160H120Zm80-40h80v-80h-80v80Zm-80-440v-160h720v160H120Zm80-40h80v-80h-80v80Zm-80 280v-160h720v160H120Zm80-40h80v-80h-80v80Z",
            Glyph::FolderOpen => "M160-160q-33 0-56.5-23.5T80-240v-480q0-33 23.5-56.5T160-800h240l80 80h320q33 0 56.5 23.5T880-640H447l-80-80H160v480l96-320h684L837-217q-8 26-29.5 41.5T760-160H160Zm84-80h516l72-240H316l-72 240Zm0 0 72-240-72 240Zm-84-400v-80 80Z",
            Glyph::Folder => "M160-160q-33 0-56.5-23.5T80-240v-480q0-33 23.5-56.5T160-800h240l80 80h320q33 0 56.5 23.5T880-640v400q0 33-23.5 56.5T800-160H160Zm0-80h640v-400H447l-80-80H160v480Zm0 0v-480 480Z",
            Glyph::CloudOff => "M792-56 686-160H260q-92 0-156-64T40-380q0-77 47.5-137T210-594q3-8 6-15.5t6-16.5L56-792l56-56 736 736-56 56ZM260-240h346L284-562q-2 11-3 21t-1 21h-20q-58 0-99 41t-41 99q0 58 41 99t99 41Zm185-161Zm419 191-58-56q17-14 25.5-32.5T840-340q0-42-29-71t-71-29h-60v-80q0-83-58.5-141.5T480-720q-27 0-52 6.5T380-693l-58-58q35-24 74.5-36.5T480-800q117 0 198.5 81.5T760-520q69 8 114.5 59.5T920-340q0 39-15 72.5T864-210ZM593-479Z",
            Glyph::Cloud => "M260-160q-91 0-155.5-63T40-377q0-78 47-139t123-78q25-92 100-149t170-57q117 0 198.5 81.5T760-520q69 8 114.5 59.5T920-340q0 75-52.5 127.5T740-160H260Zm0-80h480q42 0 71-29t29-71q0-42-29-71t-71-29h-60v-80q0-83-58.5-141.5T480-720q-83 0-141.5 58.5T280-520h-20q-58 0-99 41t-41 99q0 58 41 99t99 41Zm220-240Z",
            Glyph::Undo => "M280-200v-80h284q63 0 109.5-40T720-420q0-60-46.5-100T564-560H312l104 104-56 56-200-200 200-200 56 56-104 104h252q97 0 166.5 63T800-420q0 94-69.5 157T564-200H280Z",
        }
    }

    /// Whether the path is flipped left to right when rasterised.
    fn mirrored(self) -> bool {
        self == Glyph::ChevronLeft
    }
}

/// Texture side in pixels; glyphs are drawn at 14–22px so this is plenty.
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

/// Even-odd scanline coverage of the path at `TEXTURE_SIDE` square: one
/// alpha byte per pixel, row by row from the top, mirrored left to right if
/// asked.
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
/// relative) into closed rings in path units. Quadratic curves become
/// eight segments; other commands are ignored.
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

    #[test]
    fn every_path_is_the_d_attribute_of_its_vendored_upstream_svg() {
        macro_rules! svg {
            ($name:literal) => {
                include_str!(concat!("../assets/material-symbols/", $name, ".svg"))
            };
        }
        let sources = [
            (Glyph::ChevronRight, svg!("chevron_right")),
            (Glyph::ChevronLeft, svg!("chevron_right")),
            (Glyph::ExpandMore, svg!("keyboard_arrow_down")),
            (Glyph::ContentCopy, svg!("content_copy")),
            (Glyph::OpenInNew, svg!("open_in_new")),
            (Glyph::Delete, svg!("delete")),
            (Glyph::Undo, svg!("undo")),
            (Glyph::ViewColumn, svg!("view_column")),
            (Glyph::Home, svg!("home")),
            (Glyph::Storage, svg!("storage")),
            (Glyph::Folder, svg!("folder")),
            (Glyph::FolderOpen, svg!("folder_open")),
            (Glyph::CloudOff, svg!("cloud_off")),
            (Glyph::Cloud, svg!("cloud")),
        ];
        for (glyph, svg) in sources {
            let wrapped = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" height="24" viewBox="0 -960 960 960" width="24"><path d="{}"/></svg>"#,
                glyph.path()
            );
            assert_eq!(svg.trim_end(), wrapped, "{glyph:?}");
        }
    }
}
