// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Fonts, text sizes and colours: the platform's own faces where they can
//! be read, and both egui themes tuned for contrast.

use eframe::egui::{self, Color32};

/// How far the monospace size sits below body text, in points.
pub(super) const MONO_STEP: f32 = 1.5;

/// egui's bundled fonts with the platform's own UI and monospace faces put
/// in front of them: SF Pro (or Helvetica Neue) and SF Mono (or Menlo) on macOS, Segoe UI and
/// Cascadia Mono (or Consolas) on Windows. The bundled fonts stay as
/// fallbacks for glyphs the system faces lack, and are used alone on other
/// platforms or when no candidate file can be read. The flag says whether
/// any system face was loaded.
pub(super) fn system_fonts() -> (egui::FontDefinitions, bool) {
    use egui::FontFamily::{Monospace, Proportional};
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded = false;
    let (proportional, monospace): (&[&str], &[&str]) = if cfg!(target_os = "macos") {
        (
            &["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/HelveticaNeue.ttc"],
            &[
                "/System/Applications/Utilities/Terminal.app/Contents/Resources/Fonts/SF-Mono-Regular.otf",
                "/System/Library/Fonts/SFNSMono.ttf",
                "/System/Library/Fonts/Menlo.ttc",
            ],
        )
    } else if cfg!(target_os = "windows") {
        (&["C:\\Windows\\Fonts\\segoeui.ttf"], &["C:\\Windows\\Fonts\\CascadiaMono.ttf", "C:\\Windows\\Fonts\\consola.ttf"])
    } else {
        (&[], &[])
    };
    for (family, candidates, name) in [(Proportional, proportional, "system-ui"), (Monospace, monospace, "system-mono")] {
        // First candidate that reads as a non-empty file wins.
        let Some(bytes) = candidates.iter().filter_map(|path| std::fs::read(path).ok()).find(|b| !b.is_empty()) else {
            continue;
        };
        fonts.font_data.insert(name.to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.entry(family).or_default().insert(0, name.to_owned());
        loaded = true;
    }
    (fonts, loaded)
}

/// Text sizes matching the platform's own UI when its fonts are in use:
/// macOS sets body text at 13pt and pairs it with 12pt SF Mono; Windows
/// sets Segoe UI at 9pt (12px) with 11px captions. egui's defaults suit
/// its bundled Ubuntu Light, which sits smaller on the line than either.
/// Only called when a system face loaded, which today means macOS or Windows.
/// The monospace entry is set to body size here; `configure` then replaces
/// it with body size less [`MONO_STEP`].
pub(super) fn system_text_sizes() -> std::collections::BTreeMap<egui::TextStyle, egui::FontId> {
    use egui::FontFamily::{Monospace, Proportional};
    use egui::{FontId, TextStyle};
    let (small, body, heading) = match std::env::consts::OS {
        "macos" => (11.0, 13.0, 17.0),
        "windows" => (11.0, 12.0, 18.0),
        // Unreachable today: no system faces are looked up elsewhere.
        _ => return egui::Style::default().text_styles,
    };
    // Monospace is derived from body once the fonts are settled; see `configure`.
    let mono = body;
    [
        (TextStyle::Small, FontId::new(small, Proportional)),
        (TextStyle::Body, FontId::new(body, Proportional)),
        (TextStyle::Button, FontId::new(body, Proportional)),
        (TextStyle::Heading, FontId::new(heading, Proportional)),
        (TextStyle::Monospace, FontId::new(mono, Monospace)),
    ]
    .into()
}

/// Row fill under the pointer: the panel colour nudged toward the text
/// colour, so it reads as a hover on either theme and keeps text and weak
/// text at or above 4.5:1.
pub(super) fn hover_fill(visuals: &egui::Visuals) -> Color32 {
    visuals.panel_fill.lerp_to_gamma(visuals.text_color(), 0.12)
}

/// Colour of a disabled icon button: about 3:1 or better against the panel on either theme.
pub(super) fn disabled_icon(visuals: &egui::Visuals) -> Color32 {
    visuals.panel_fill.lerp_to_gamma(visuals.text_color(), 0.55)
}

/// Both themes with text contrast at WCAG 2.1 AA or better. egui's dark
/// defaults give 5.1:1 for text and about 2.7:1 for weak text; the light
/// defaults are fine for text but weak text is around 3:1.
pub(super) fn apply_theme(ctx: &egui::Context) {
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
