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
    let root = tempfile::tempdir().unwrap();
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

/// A real secondary click on the labelled widget, opening its context menu.
fn open_menu_on(harness: &mut Harness<'_, Gui>, label: &str) {
    let pos = harness.get_by_label(label).rect().center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        harness.step();
    }
    harness.step();
}

/// Inside a Time Machine backup the menu carries a note. On macOS the trash
/// item is disabled and the note says where backups are managed; elsewhere
/// the backup is just files, so the note only names it and the trash stays.
#[cfg(feature = "trash")]
#[test]
fn time_machine_backup_offers_no_trash() {
    let root = tempfile::tempdir().unwrap();
    let snapshot = root.path().join("Backups.backupdb/Mac/2026-09-01-120000/Macintosh HD/Users/me");
    std::fs::create_dir_all(&snapshot).unwrap();
    std::fs::write(snapshot.join("photos.zip"), vec![0_u8; 300_000]).unwrap();
    std::fs::create_dir(root.path().join("Documents")).unwrap();
    std::fs::write(root.path().join("Documents/report.pdf"), vec![0_u8; 100_000]).unwrap();

    let mut harness = harness(root.path());
    wait_for_scan(&mut harness, Duration::from_secs(30));
    assert_scanned(&harness);

    // An ordinary folder beside the backup keeps its trash item.
    open_menu_on(&mut harness, "Documents/");
    assert!(harness.query_by_label(dirstats_app::backup::NOTE).is_none());
    harness.key_press(Key::Escape);
    harness.step();

    open_menu_on(&mut harness, "Backups.backupdb/");
    harness.get_by_label(dirstats_app::backup::NOTE);
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
