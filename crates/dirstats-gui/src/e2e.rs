// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! End-to-end tests: the whole GUI run headlessly, scanning real
//! directories and rendered offscreen with wgpu. Behind the `e2e` feature
//! because they pull in wgpu and are meant for CI:
//!
//! ```text
//! cargo test -p dirstats-gui --features e2e,egui-fonts
//! cargo test -p dirstats-gui --features e2e,egui-fonts -- --ignored   # scans the whole disk
//! ```
//!
//! Each test saves its last frame as a PNG under `DIRSTATS_E2E_OUT`
//! (default `target/e2e`), which CI uploads for a person to look over.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dirstats_app::App;
use eframe::egui::{self, Key};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::{Gui, configure};

/// A window-sized harness around the real [`Gui`], scanning `root`.
fn harness(root: &Path) -> Harness<'static, Gui> {
    let mut app = App::default();
    app.start_scan(root);
    Harness::builder().with_size(egui::vec2(1280.0, 800.0)).wgpu().build_eframe(move |cc| {
        configure(&cc.egui_ctx);
        Gui::new(app)
    })
}

/// A fresh fixture directory. On Unix it goes under `/tmp` rather than the
/// per-user temp dir (`/var/folders/..` on macOS), so the path in the
/// toolbar says nothing about the machine and screenshots can be shared.
fn fixture_dir() -> tempfile::TempDir {
    #[cfg(unix)]
    return tempfile::Builder::new().prefix("dirstats-e2e-").tempdir_in("/tmp").unwrap();
    #[cfg(not(unix))]
    return tempfile::Builder::new().prefix("dirstats-e2e-").tempdir().unwrap();
}

/// Step frames until the scan is adopted, then a few more so the treemap
/// and its texture are built.
fn wait_for_scan(harness: &mut Harness<'_, Gui>, limit: Duration) {
    let start = Instant::now();
    harness.step();
    while harness.state().app.is_scanning() {
        assert!(start.elapsed() < limit, "scan still running after {limit:?}");
        std::thread::sleep(Duration::from_millis(50));
        harness.step();
    }
    for _ in 0..3 {
        harness.step();
    }
    eprintln!("scan took {:?}", start.elapsed());
}

/// Save the current frame where CI picks it up.
fn screenshot(harness: &mut Harness<'_, Gui>, name: &str) {
    let dir = std::env::var_os("DIRSTATS_E2E_OUT")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/e2e"), PathBuf::from);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}-{}.png", std::env::consts::OS));
    harness.render().expect("offscreen render").save(&path).unwrap();
    eprintln!("wrote {}", path.display());
}

/// A real pointer click on the labelled widget: rows take clicks on their
/// whole rect, which an accessibility click on the label would miss.
fn click(harness: &mut Harness<'_, Gui>, label: &str) {
    let pos = harness.get_by_label(label).rect().center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        harness.step();
    }
}

/// What every finished scan must leave behind, whatever was scanned.
fn assert_scanned(harness: &Harness<'_, Gui>) {
    let gui = harness.state();
    assert!(!gui.app.message.as_deref().is_some_and(|m| m.contains("failed")), "{:?}", gui.app.message);
    let tree = gui.app.tree.as_ref().expect("a tree after the scan");
    let dir = gui.app.dir().expect("a current directory");
    assert!(tree.size(dir) > 0, "root has no size");
    assert!(tree.node(dir).file_count > 0, "root has no files");
    assert!(!gui.app.entries().is_empty(), "no entries listed");
    assert!(gui.map.is_some() && gui.texture.is_some(), "treemap was not rendered");
    // The toolbar is out of its scanning state.
    harness.get_by_label("Rescan");
}

#[test]
fn fixture_scan_lists_and_zooms() {
    let root = fixture_dir();
    std::fs::create_dir_all(root.path().join("big/nested")).unwrap();
    std::fs::create_dir(root.path().join("small")).unwrap();
    std::fs::write(root.path().join("big/movie.mkv"), vec![0_u8; 300_000]).unwrap();
    std::fs::write(root.path().join("big/nested/archive.zip"), vec![0_u8; 200_000]).unwrap();
    std::fs::write(root.path().join("small/notes.txt"), vec![0_u8; 10_000]).unwrap();
    std::fs::write(root.path().join("readme.md"), vec![0_u8; 50_000]).unwrap();

    let mut harness = harness(root.path());
    wait_for_scan(&mut harness, Duration::from_secs(30));
    assert_scanned(&harness);

    {
        let gui = harness.state();
        let tree = gui.app.tree.as_ref().unwrap();
        let dir = gui.app.dir().unwrap();
        assert_eq!(tree.node(dir).file_count, 4);
        // Largest first.
        let names: Vec<_> = gui.app.entries().iter().map(|&id| tree.node(id).name.to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["big", "readme.md", "small"]);
    }
    // Directories are listed with a trailing slash.
    for name in ["big/", "readme.md", "small/"] {
        harness.get_by_label(name);
    }
    screenshot(&mut harness, "fixture");

    // Select the largest entry, zoom into it, and come back.
    click(&mut harness, "big/");
    harness.key_press(Key::Enter);
    harness.step();
    harness.step();
    {
        let gui = harness.state();
        let tree = gui.app.tree.as_ref().unwrap();
        assert_eq!(tree.node(gui.app.dir().unwrap()).name.to_string_lossy(), "big");
        assert!(gui.app.can_back());
    }
    harness.get_by_label("movie.mkv");
    screenshot(&mut harness, "fixture-zoomed");

    harness.key_press(Key::Backspace);
    harness.step();
    assert!(!harness.state().app.can_back());
}

/// Scans the machine's whole system disk; for throwaway CI runners.
#[test]
#[ignore = "scans the whole disk"]
fn real_disk_scan() {
    let root = std::env::var_os("DIRSTATS_E2E_ROOT").map_or_else(|| PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" }), PathBuf::from);
    let mut harness = harness(&root);
    // One frame of the scanning screen, for the record.
    harness.step();
    screenshot(&mut harness, "real-disk-scanning");

    wait_for_scan(&mut harness, Duration::from_secs(20 * 60));
    assert_scanned(&harness);
    {
        let gui = harness.state();
        let tree = gui.app.tree.as_ref().unwrap();
        let dir = gui.app.dir().unwrap();
        eprintln!("{}: {} bytes in {} files", root.display(), tree.size(dir), tree.node(dir).file_count);
        // A system disk holds far more than this.
        assert!(tree.node(dir).file_count > 10_000);
        assert!(tree.size(dir) > 1 << 30);
    }
    screenshot(&mut harness, "real-disk");
}

/// The fixture the context-menu tests share: `big/` (with `movie.mkv` and
/// `nested/archive.zip`), `readme.md` and `small/notes.txt`, sizes well apart.
fn menu_fixture() -> tempfile::TempDir {
    let root = fixture_dir();
    std::fs::create_dir_all(root.path().join("big/nested")).unwrap();
    std::fs::create_dir(root.path().join("small")).unwrap();
    std::fs::write(root.path().join("big/movie.mkv"), vec![0_u8; 600_000]).unwrap();
    std::fs::write(root.path().join("big/nested/archive.zip"), vec![0_u8; 200_000]).unwrap();
    std::fs::write(root.path().join("small/notes.txt"), vec![0_u8; 10_000]).unwrap();
    std::fs::write(root.path().join("readme.md"), vec![0_u8; 50_000]).unwrap();
    root
}

/// A real right click at `pos`, then a frame for the menu to appear.
fn right_click_at(harness: &mut Harness<'_, Gui>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        harness.step();
    }
    harness.step();
}

fn right_click(harness: &mut Harness<'_, Gui>, label: &str) {
    let pos = harness.get_by_label(label).rect().center();
    right_click_at(harness, pos);
}

/// Screen position of the middle of `node`'s box in the treemap.
fn treemap_point(harness: &Harness<'_, Gui>, name: &str) -> egui::Pos2 {
    let gui = harness.state();
    let tree = gui.app.tree.as_ref().unwrap();
    let map = gui.map.as_ref().expect("a treemap");
    let item = map.items.iter().find(|item| item.leaf && *tree.node(item.node).name == *std::ffi::OsStr::new(name)).unwrap_or_else(|| panic!("no box for {name}"));
    // The map is the only image the size of its layout.
    let size = egui::vec2(map.width as f32, map.height as f32);
    let image = harness
        .get_all_by_role(egui::accesskit::Role::Image)
        .map(|node| node.rect())
        .find(|rect| (rect.size() - size).length() < 2.0)
        .expect("the treemap image");
    let r = item.rect;
    image.min + egui::vec2((r.left + r.right) as f32 / 2.0, (r.top + r.bottom) as f32 / 2.0)
}

fn dir_name(harness: &Harness<'_, Gui>) -> String {
    let gui = harness.state();
    gui.app.tree.as_ref().unwrap().node(gui.app.dir().unwrap()).name.to_string_lossy().into_owned()
}

#[test]
fn context_menu_on_a_row_zooms_into_the_folder() {
    let root = menu_fixture();
    let mut harness = harness(root.path());
    wait_for_scan(&mut harness, Duration::from_secs(30));
    assert_scanned(&harness);

    right_click(&mut harness, "big/");
    // The menu is open, headed by the folder's name.
    harness.get_by_label("Zoom in");
    harness.get_by_label("Copy path");
    assert!(harness.query_by_label("Zoom in to containing folder").is_none());
    screenshot(&mut harness, "context-menu-row");

    click(&mut harness, "Zoom in");
    harness.step();
    harness.step();
    assert_eq!(dir_name(&harness), "big");
    assert!(harness.state().app.can_back());
    assert!(harness.query_by_label("Copy path").is_none(), "menu still open");
    harness.get_by_label("movie.mkv");
    screenshot(&mut harness, "context-menu-row-zoomed");
}

#[test]
fn context_menu_copies_a_path_and_closes_on_escape() {
    let root = menu_fixture();
    let mut harness = harness(root.path());
    wait_for_scan(&mut harness, Duration::from_secs(30));
    assert_scanned(&harness);

    // A file in the folder already shown has nothing to zoom into.
    right_click(&mut harness, "readme.md");
    harness.get_by_label("Copy path");
    assert!(harness.query_by_label("Zoom in").is_none());
    assert!(harness.query_by_label("Zoom in to containing folder").is_none());
    screenshot(&mut harness, "context-menu-file");

    harness.key_press(Key::Escape);
    harness.step();
    assert!(harness.query_by_label("Copy path").is_none(), "Escape left the menu open");

    right_click(&mut harness, "readme.md");
    click(&mut harness, "Copy path");
    harness.step();
    let message = harness.state().app.message.clone().unwrap_or_default();
    assert!(message.starts_with("copied ") && message.ends_with("readme.md"), "{message:?}");
    assert!(harness.query_by_label("Copy path").is_none(), "menu still open");
}

#[test]
fn context_menu_on_the_treemap_zooms_to_the_containing_folder() {
    let root = menu_fixture();
    let mut harness = harness(root.path());
    wait_for_scan(&mut harness, Duration::from_secs(30));
    assert_scanned(&harness);

    let pos = treemap_point(&harness, "movie.mkv");
    right_click_at(&mut harness, pos);
    // A file below the shown folder zooms to the folder holding it.
    harness.get_by_label("Zoom in to containing folder");
    {
        let gui = harness.state();
        let tree = gui.app.tree.as_ref().unwrap();
        let Some(crate::Selection::Node(selected)) = gui.selection else { panic!("right click did not select a box") };
        assert_eq!(tree.node(selected).name.to_string_lossy(), "movie.mkv");
    }
    screenshot(&mut harness, "context-menu-treemap");

    click(&mut harness, "Zoom in to containing folder");
    harness.step();
    harness.step();
    assert_eq!(dir_name(&harness), "big");
    screenshot(&mut harness, "context-menu-treemap-zoomed");
}

/// Moves a file from the test's own tempdir to the system trash, and puts it
/// back.
#[cfg(feature = "trash")]
#[test]
#[ignore = "moves a file to the system trash"]
fn context_menu_moves_a_file_to_the_trash() {
    use crate::menu::TRASH_NAME;

    let root = menu_fixture();
    let file = root.path().join("readme.md");
    let mut harness = harness(root.path());
    wait_for_scan(&mut harness, Duration::from_secs(30));
    assert_scanned(&harness);

    right_click(&mut harness, "readme.md");
    screenshot(&mut harness, "context-menu-trash");
    click(&mut harness, &format!("Move to {TRASH_NAME}"));
    harness.step();
    assert!(!file.exists(), "file still on disk: {:?}", harness.state().app.message);
    {
        let gui = harness.state();
        let tree = gui.app.tree.as_ref().unwrap();
        let node = gui.app.entries().iter().copied().find(|&id| *tree.node(id).name == *std::ffi::OsStr::new("readme.md")).unwrap();
        assert!(gui.app.is_trashed(node));
    }

    // Opened again, the menu no longer offers the trash.
    right_click(&mut harness, "readme.md");
    assert!(harness.query_by_label(&format!("Move to {TRASH_NAME}")).is_none());
    screenshot(&mut harness, "context-menu-trashed");
    click(&mut harness, "Put Back");
    harness.step();
    assert!(file.exists(), "not put back: {:?}", harness.state().app.message);
    right_click(&mut harness, "readme.md");
    harness.get_by_label(&format!("Move to {TRASH_NAME}"));
    screenshot(&mut harness, "context-menu-put-back");
}

/// Inside a Time Machine backup the menu carries a note. On macOS the trash
/// item is disabled and the note says where backups are managed; elsewhere
/// the backup is just files, so the note only names it and the trash stays.
#[cfg(feature = "trash")]
#[test]
fn time_machine_backup_offers_no_trash() {
    use crate::menu::TRASH_NAME;
    use egui_kittest::kittest::NodeT;

    let root = fixture_dir();
    let snapshot = root.path().join("Backups.backupdb/Mac/2026-09-01-120000/Macintosh HD/Users/me");
    std::fs::create_dir_all(&snapshot).unwrap();
    std::fs::write(snapshot.join("photos.zip"), vec![0_u8; 300_000]).unwrap();
    std::fs::create_dir(root.path().join("Documents")).unwrap();
    std::fs::write(root.path().join("Documents/report.pdf"), vec![0_u8; 100_000]).unwrap();
    let trash = format!("Move to {TRASH_NAME}");

    let mut harness = harness(root.path());
    wait_for_scan(&mut harness, Duration::from_secs(30));
    assert_scanned(&harness);

    // An ordinary folder beside the backup keeps its trash item.
    right_click(&mut harness, "Documents/");
    assert!(!harness.get_by_label(&trash).accesskit_node().is_disabled());
    assert!(harness.query_by_label(dirstats_app::backup::NOTE).is_none());
    harness.key_press(Key::Escape);
    harness.step();

    right_click(&mut harness, "Backups.backupdb/");
    harness.get_by_label(dirstats_app::backup::NOTE);
    let disabled = harness.get_by_label(&trash).accesskit_node().is_disabled();
    assert_eq!(disabled, cfg!(target_os = "macos"), "trash item disabled only on macOS");
    screenshot(&mut harness, "time-machine-menu");

    // The app decides, whatever a front end offers.
    let gui = harness.state();
    let tree = gui.app.tree.as_ref().unwrap();
    let backup = tree.children(tree.root()).iter().copied().find(|&id| tree.node(id).name.to_string_lossy() == "Backups.backupdb").unwrap();
    assert!(gui.app.is_time_machine(backup));
    if cfg!(target_os = "macos") {
        let err = gui.app.check_removable(backup).unwrap_err();
        assert!(err.to_string().contains("Managed by Time Machine"), "{err}");
    } else {
        assert!(dirstats_app::backup::NOTE.contains("macOS Time Machine backup"));
        gui.app.check_removable(backup).expect("only macOS keeps backups out of the trash");
    }
}
