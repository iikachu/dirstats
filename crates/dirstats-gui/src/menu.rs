// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.


//! The context menu shared by the tree and the treemap, and the rows it is built from.

use dirstats_app::NodeId;
use eframe::egui::{self, Sense};

use crate::icons;

/// What a node's context menu asked for; applied after the menu closes.
#[derive(Clone, Copy, Debug)]
pub(super) enum NodeAction {
    /// Zoom into the node, or into its folder when it is a file.
    Zoom,
    /// Put the node's full path on the clipboard.
    CopyPath,
    /// Open with the desktop's default handler.
    #[cfg(feature = "open")]
    Open,
    /// Move to the platform trash ([`TRASH_NAME`]).
    #[cfg(feature = "trash")]
    Trash,
    /// Move back from the trash to where it was scanned.
    #[cfg(feature = "trash")]
    PutBack,
    /// macOS: drop the local copy of an iCloud item.
    #[cfg(feature = "icloud")]
    Evict,
    /// Windows and Linux: delete without the trash, after the gate and a confirmation.
    #[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
    DeletePermanently,
    /// Windows and Linux: open the gate dialog, then delete if it is accepted.
    #[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
    EnablePermanentDelete,
}

/// What the platform calls its trash in menu labels.
/// Permanent delete names it too, to say it is skipped.
#[cfg(any(feature = "trash", all(any(windows, target_os = "linux"), feature = "delete"), test))]
pub(super) const TRASH_NAME: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };

/// Whether the permanent-delete item is offered and how it reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Permanent {
    /// Not on this platform: only the trash is offered.
    Unavailable,
    /// Offered as "Enable Permanent Delete…", which opens the gate dialog.
    Locked,
    /// Gate passed: offered as "Delete Permanently".
    Enabled,
}

/// Label of the zoom item for `node`: a folder zooms into itself, a file
/// into the folder holding it, and nothing is offered when that is where
/// the view already is.
pub(super) fn zoom_label(tree: &dirstats_app::Tree, current: Option<NodeId>, node: NodeId) -> Option<&'static str> {
    if !tree.children(node).is_empty() {
        return Some("Zoom in");
    }
    let parent = tree.node(node).parent?;
    (Some(parent) != current).then_some("Zoom in to containing folder")
}

/// Menu items for a node: the same in the tree and the treemap. `zoom` is
/// the label of the zoom item, if one is offered; `trashed`, `permanent` and
/// `cloud` pick the trash, permanent-delete and iCloud rows, each shown only
/// when its feature is on. Returns the chosen action and closes the menu.
pub(super) fn node_menu(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    zoom: Option<&str>,
    trashed: TrashState,
    permanent: Permanent,
    cloud: dirstats_app::cloud::CloudStatus,
) -> Option<NodeAction> {
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
    if let Some(label) = zoom
        && menu_item(ui, None, label, false).clicked()
    {
        action = Some(NodeAction::Zoom);
    }
    if menu_item(ui, Some(icons::Glyph::ContentCopy), "Copy path", false).clicked() {
        action = Some(NodeAction::CopyPath);
    }
    #[cfg(feature = "open")]
    if menu_item(ui, Some(icons::Glyph::OpenInNew), "Open", false).clicked() {
        action = Some(NodeAction::Open);
    }
    #[cfg(feature = "icloud")]
    match cloud {
        dirstats_app::cloud::CloudStatus::Local => {}
        dirstats_app::cloud::CloudStatus::Downloaded => {
            if menu_item(ui, Some(icons::Glyph::CloudOff), "Remove Download", false).clicked() {
                action = Some(NodeAction::Evict);
            }
        }
        dirstats_app::cloud::CloudStatus::Evicted => {
            ui.add_enabled_ui(false, |ui| menu_item(ui, Some(icons::Glyph::CloudOff), "Not Downloaded", false));
        }
    }
    #[cfg(not(feature = "icloud"))]
    let _ = cloud;
    // Trash and permanent delete are separate features; the section shows
    // when either can offer something here.
    #[cfg(any(feature = "trash", all(any(windows, target_os = "linux"), feature = "delete")))]
    {
        menu_separator(ui);
        match trashed {
            TrashState::Present | TrashState::TimeMachine => {
                let backup = trashed == TrashState::TimeMachine;
                if backup && dirstats_app::backup::BLOCKS_TRASH {
                    // Shown but disabled, with the note saying where to go
                    // instead, so the missing action does not read as a bug.
                    #[cfg(feature = "trash")]
                    ui.add_enabled_ui(false, |ui| menu_item(ui, Some(icons::Glyph::Delete), &format!("Move to {TRASH_NAME}"), false));
                } else {
                    #[cfg(feature = "trash")]
                    if menu_item(ui, Some(icons::Glyph::Delete), &format!("Move to {TRASH_NAME}"), true).clicked() {
                        action = Some(NodeAction::Trash);
                    }
                    #[cfg(all(any(windows, target_os = "linux"), feature = "delete"))]
                    match permanent {
                        Permanent::Unavailable => {}
                        Permanent::Locked => {
                            if menu_item(ui, None, "Enable Permanent Delete…", false).clicked() {
                                action = Some(NodeAction::EnablePermanentDelete);
                            }
                        }
                        Permanent::Enabled => {
                            if menu_item(ui, None, "Delete Permanently", true).clicked() {
                                action = Some(NodeAction::DeletePermanently);
                            }
                        }
                    }
                }
                if backup {
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.add(egui::Label::new(egui::RichText::new(dirstats_app::backup::NOTE).weak().small()).wrap().selectable(false));
                    });
                    ui.add_space(4.0);
                }
            }
            // Only reachable with the trash feature: nothing else trashes.
            TrashState::CanPutBack => {
                #[cfg(feature = "trash")]
                if menu_item(ui, Some(icons::Glyph::Undo), "Put Back", false).clicked() {
                    action = Some(NodeAction::PutBack);
                }
            }
            TrashState::Trashed => {
                #[cfg(feature = "trash")]
                ui.add_enabled_ui(false, |ui| menu_item(ui, Some(icons::Glyph::Delete), &format!("In {TRASH_NAME}"), false));
            }
            TrashState::Deleted => {
                ui.add_enabled_ui(false, |ui| menu_item(ui, Some(icons::Glyph::Delete), "Deleted", false));
            }
        }
    }
    #[cfg(not(all(any(windows, target_os = "linux"), feature = "delete")))]
    let _ = permanent;
    #[cfg(not(any(feature = "trash", all(any(windows, target_os = "linux"), feature = "delete"))))]
    let _ = trashed;
    if action.is_some() {
        ui.close();
    }
    action
}

/// Trash state of the node a menu is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TrashState {
    /// On disk as scanned.
    Present,
    /// Trashed, and the app knows where it went.
    CanPutBack,
    /// Trashed, location unknown.
    Trashed,
    /// Deleted permanently (Windows and Linux), or under something that was.
    Deleted,
    /// Part of a Time Machine backup: noted in the menu, and on macOS the
    /// trash row is shown disabled, since Time Machine removes its own backups.
    TimeMachine,
}

/// A menu row: optional leading icon in a fixed slot so labels line up,
/// then the label. `destructive` uses the error colour.
pub(super) fn menu_item(ui: &mut egui::Ui, glyph: Option<icons::Glyph>, label: &str, destructive: bool) -> egui::Response {
    const HEIGHT: f32 = 28.0;
    const PAD: f32 = 10.0;
    const SLOT: f32 = 20.0;
    const GAP: f32 = 10.0;
    let width = ui.available_width().max(180.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), Sense::click());
    // Painted by hand, so name the row for screen readers (and tests).
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
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
pub(super) fn menu_separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, ui.visuals().widgets.noninteractive.bg_stroke);
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use dirstats_app::cloud::CloudStatus;
    use dirstats_app::scan::{Kind, Node, TreeBuilder};
    use dirstats_app::{SizeMetric, Tree};
    use eframe::egui::{self, Event, PointerButton, Pos2, Rect};

    use super::*;

    const PATH: &str = "/Users/me/Documents/report.pdf";

    fn node(name: &str, parent: Option<NodeId>, kind: Kind, size: u64) -> Node {
        Node {
            name: std::ffi::OsStr::new(name).into(),
            parent,
            kind,
            apparent_size: size,
            allocated_size: size,
            file_count: u64::from(kind == Kind::File),
            dir_count: 0,
            modified: None,
            duplicate_link: false,
            error: false,
        }
    }

    /// `/root` holding `sub/` (with `inner.txt`) and `top.txt`.
    fn sample() -> (Tree, NodeId, NodeId, NodeId) {
        let mut b = TreeBuilder::new();
        let root = b.push(node("/root", None, Kind::Directory, 0));
        let sub = b.push(node("sub", Some(root), Kind::Directory, 0));
        let inner = b.push(node("inner.txt", Some(sub), Kind::File, 10));
        let top = b.push(node("top.txt", Some(root), Kind::File, 5));
        (b.finish(SizeMetric::Allocated), sub, inner, top)
    }

    /// Menu inputs, defaulting to a present, local item with a zoom row.
    #[derive(Clone, Copy)]
    struct Args {
        zoom: Option<&'static str>,
        trashed: TrashState,
        permanent: Permanent,
        cloud: CloudStatus,
    }

    impl Default for Args {
        fn default() -> Self {
            Self { zoom: Some("Zoom in"), trashed: TrashState::Present, permanent: Permanent::Unavailable, cloud: CloudStatus::Local }
        }
    }

    /// One painted piece of text and where it landed.
    struct Painted {
        text: String,
        rect: Rect,
    }

    /// Runs the menu headlessly, one frame per entry in `frames`, feeding
    /// each frame's events. Returns the text of the last frame and every
    /// action the menu returned.
    fn run(path: &Path, args: Args, frames: &[Vec<Event>]) -> (Vec<Painted>, Vec<NodeAction>) {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let mut painted = Vec::new();
        for events in frames {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
                events: events.clone(),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(action) = node_menu(ui, path, args.zoom, args.trashed, args.permanent, args.cloud) {
                        actions.push(action);
                    }
                });
            });
            painted = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(Painted {
                        text: text.galley.text().to_owned(),
                        rect: text.galley.rect.translate(text.pos.to_vec2()),
                    }),
                    _ => None,
                })
                .collect();
        }
        (painted, actions)
    }

    /// Every piece of text one frame of the menu paints, in order.
    fn labels(path: &Path, args: Args) -> Vec<String> {
        run(path, args, &[vec![]]).0.into_iter().map(|p| p.text).collect()
    }

    /// Clicks the row labelled `label` and returns what the menu reported.
    fn click(label: &str, args: Args) -> Vec<NodeAction> {
        let path = Path::new(PATH);
        let (painted, _) = run(path, args, &[vec![]]);
        let at = painted.iter().find(|p| p.text == label).unwrap_or_else(|| panic!("no row {label:?}")).rect.center();
        let press = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        let frames = [vec![], vec![Event::PointerMoved(at)], vec![press(true)], vec![press(false)], vec![]];
        run(path, args, &frames).1
    }

    #[test]
    fn folder_zooms_into_itself() {
        let (tree, sub, _, _) = sample();
        assert_eq!(zoom_label(&tree, Some(tree.root()), sub), Some("Zoom in"));
        assert_eq!(zoom_label(&tree, Some(sub), sub), Some("Zoom in"));
    }

    #[test]
    fn file_zooms_into_its_folder_unless_already_there() {
        let (tree, sub, inner, top) = sample();
        assert_eq!(zoom_label(&tree, Some(tree.root()), inner), Some("Zoom in to containing folder"));
        assert_eq!(zoom_label(&tree, Some(sub), inner), None);
        assert_eq!(zoom_label(&tree, Some(tree.root()), top), None);
        assert_eq!(zoom_label(&tree, None, top), Some("Zoom in to containing folder"));
    }

    #[test]
    fn header_shows_name_then_folder() {
        let text = labels(Path::new(PATH), Args::default());
        assert_eq!(text[0], "report.pdf");
        assert_eq!(text[1], "/Users/me/Documents");
    }

    #[test]
    fn header_of_a_root_has_no_folder_line() {
        let text = labels(Path::new("/"), Args::default());
        assert_eq!(text[0], "/");
        assert_eq!(text[1], "Zoom in");
    }

    #[test]
    fn zoom_row_follows_its_label() {
        let with = labels(Path::new(PATH), Args { zoom: Some("Zoom in to containing folder"), ..Args::default() });
        assert!(with.iter().any(|t| t == "Zoom in to containing folder"));
        let without = labels(Path::new(PATH), Args { zoom: None, ..Args::default() });
        assert!(!without.iter().any(|t| t.starts_with("Zoom")));
        assert!(without.iter().any(|t| t == "Copy path"));
    }

    #[test]
    fn nothing_is_chosen_without_a_click() {
        let (_, actions) = run(Path::new(PATH), Args::default(), &[vec![], vec![], vec![]]);
        assert!(actions.is_empty());
    }

    #[test]
    fn clicking_zoom_reports_zoom() {
        assert!(matches!(click("Zoom in", Args::default())[..], [NodeAction::Zoom]));
    }

    #[test]
    fn clicking_copy_path_reports_copy_path() {
        assert!(matches!(click("Copy path", Args::default())[..], [NodeAction::CopyPath]));
    }

    #[cfg(feature = "open")]
    #[test]
    fn clicking_open_reports_open() {
        assert!(matches!(click("Open", Args::default())[..], [NodeAction::Open]));
    }

    #[cfg(not(feature = "trash"))]
    #[test]
    fn no_trash_rows_without_the_feature() {
        let text = labels(Path::new(PATH), Args::default());
        assert!(!text.iter().any(|t| t.contains(TRASH_NAME) || t == "Put Back" || t == "Deleted"));
    }

    #[cfg(feature = "trash")]
    mod trash {
        use super::*;

        fn with(trashed: TrashState) -> Args {
            Args { trashed, ..Args::default() }
        }

        #[test]
        fn present_item_offers_the_trash() {
            let move_to = format!("Move to {TRASH_NAME}");
            assert!(labels(Path::new(PATH), with(TrashState::Present)).contains(&move_to));
            assert!(matches!(click(&move_to, with(TrashState::Present))[..], [NodeAction::Trash]));
        }

        #[test]
        fn trashed_item_with_known_location_offers_put_back() {
            let text = labels(Path::new(PATH), with(TrashState::CanPutBack));
            assert!(!text.iter().any(|t| t.starts_with("Move to")));
            assert!(matches!(click("Put Back", with(TrashState::CanPutBack))[..], [NodeAction::PutBack]));
        }

        #[test]
        fn trashed_item_shows_a_disabled_row() {
            let in_trash = format!("In {TRASH_NAME}");
            assert!(labels(Path::new(PATH), with(TrashState::Trashed)).contains(&in_trash));
            assert!(click(&in_trash, with(TrashState::Trashed)).is_empty());
        }

        #[test]
        fn deleted_item_shows_a_disabled_row() {
            assert!(labels(Path::new(PATH), with(TrashState::Deleted)).iter().any(|t| t == "Deleted"));
            assert!(click("Deleted", with(TrashState::Deleted)).is_empty());
        }
    }

    #[cfg(feature = "delete")]
    mod permanent {
        use super::*;

        #[cfg(target_os = "macos")]
        #[test]
        fn no_permanent_delete_on_macos() {
            for permanent in [Permanent::Locked, Permanent::Enabled] {
                let text = labels(Path::new(PATH), Args { permanent, ..Args::default() });
                assert!(!text.iter().any(|t| t.contains("Permanent")), "{permanent:?}");
            }
        }

        #[cfg(any(windows, target_os = "linux"))]
        #[test]
        fn locked_permanent_delete_opens_the_gate() {
            let args = Args { permanent: Permanent::Locked, ..Args::default() };
            assert!(!labels(Path::new(PATH), args).iter().any(|t| t == "Delete Permanently"));
            assert!(matches!(click("Enable Permanent Delete…", args)[..], [NodeAction::EnablePermanentDelete]));
        }

        #[cfg(any(windows, target_os = "linux"))]
        #[test]
        fn enabled_permanent_delete_deletes() {
            let args = Args { permanent: Permanent::Enabled, ..Args::default() };
            assert!(!labels(Path::new(PATH), args).iter().any(|t| t == "Enable Permanent Delete…"));
            assert!(matches!(click("Delete Permanently", args)[..], [NodeAction::DeletePermanently]));
        }

        #[cfg(any(windows, target_os = "linux"))]
        #[test]
        fn permanent_delete_only_for_present_items() {
            let args = Args { permanent: Permanent::Enabled, trashed: TrashState::Deleted, ..Args::default() };
            assert!(!labels(Path::new(PATH), args).iter().any(|t| t == "Delete Permanently"));
        }

        /// Permanent delete does not need the trash feature.
        #[cfg(all(any(windows, target_os = "linux"), not(feature = "trash")))]
        #[test]
        fn offered_without_the_trash() {
            let text = labels(Path::new(PATH), Args { permanent: Permanent::Enabled, ..Args::default() });
            assert!(text.iter().any(|t| t == "Delete Permanently"));
            assert!(!text.iter().any(|t| t.contains(TRASH_NAME)));
        }
    }

    #[cfg(feature = "icloud")]
    mod icloud {
        use super::*;

        fn with(cloud: CloudStatus) -> Args {
            Args { cloud, ..Args::default() }
        }

        #[test]
        fn local_item_has_no_cloud_row() {
            let text = labels(Path::new(PATH), with(CloudStatus::Local));
            assert!(!text.iter().any(|t| t == "Remove Download" || t == "Not Downloaded"));
        }

        #[test]
        fn downloaded_item_can_be_evicted() {
            assert!(matches!(click("Remove Download", with(CloudStatus::Downloaded))[..], [NodeAction::Evict]));
        }

        #[test]
        fn evicted_item_shows_a_disabled_row() {
            assert!(click("Not Downloaded", with(CloudStatus::Evicted)).is_empty());
        }
    }
}
