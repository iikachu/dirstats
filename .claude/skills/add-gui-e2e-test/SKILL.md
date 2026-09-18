---
name: add-gui-e2e-test
description: Add or change a headless end-to-end test of the dirstats GUI (crates/dirstats-gui/src/e2e.rs, egui_kittest + offscreen wgpu, run by the "gui e2e" CI job). Use when asked to test GUI behaviour, cover a GUI bug with a regression test, or extend the CI screenshots.
model: opus
---

# Adding a GUI end-to-end test

The tests live in one file, `crates/dirstats-gui/src/e2e.rs`, compiled only with
`--features e2e` under `cfg(test)`. They build the real `Gui`, scan a real
directory, step frames without a window, and save PNGs that CI uploads for a
person to look over. Read that file first; reuse its helpers rather than
writing new ones.

## Run

```bash
cargo test -p dirstats-gui --features e2e,egui-fonts
```

```bash
DIRSTATS_E2E_ROOT=some/midsize/dir cargo test -p dirstats-gui --features e2e,egui-fonts -- --ignored --nocapture
```

Always pass `egui-fonts`: without it the app loads system fonts and labels,
sizes and screenshots differ per OS. Never run the ignored `real_disk_scan`
without `DIRSTATS_E2E_ROOT` on a developer machine; it scans the whole disk.
PNGs land in `target/e2e/` (or `DIRSTATS_E2E_OUT`).

## Helpers already there

- `harness(root)`: a 1280x800 `Harness<Gui>` with the scan started and
  `configure` applied, as `run` does.
- `wait_for_scan(&mut harness, limit)`: steps until the tree is adopted and
  the treemap texture exists.
- `assert_scanned(&harness)`: the invariants every finished scan must hold.
- `click(&mut harness, label)`: a real pointer click at the widget's centre.
- `screenshot(&mut harness, name)`: writes `<name>-<os>.png`.
- `right_click(&mut harness, label)` / `right_click_at(&mut harness, pos)`:
  opens a context menu. Menu rows are labelled buttons (`"Zoom in"`,
  `"Copy path"`); click them with `click`.
- `treemap_point(&harness, name)`: screen centre of a file's treemap box.

## Writing a test

1. Build a fixture in `fixture_dir()` (under `/tmp` on Unix, so screenshots
   show no personal path) with files of distinct, well
   separated sizes (entries sort largest first; near-equal sizes make order
   depend on allocation rounding). Use `vec![0_u8; n]` contents.
2. `harness(root.path())`, `wait_for_scan`, `assert_scanned`.
3. Assert on **state** through `harness.state()` (`gui.app.tree`,
   `gui.app.entries()`, `gui.app.dir()`, `gui.selection`, ...) for exact
   facts, and on **the accessibility tree** through `harness.get_by_label(..)`
   for what is shown. Do both where it matters: state proves the logic, the
   label proves it reached the screen.
4. Drive input, then `harness.step()` at least once before asserting; a zoom
   or a new treemap needs two steps.
5. Call `screenshot` after each state worth a person's glance. Give each a
   distinct name; the OS suffix is added for you.

## Things that will trip you

- Directory rows are labelled with a trailing slash: `"big/"`, not `"big"`.
  Files are plain: `"readme.md"`. Extensions in the legend carry the dot:
  `".mkv"`.
- `get_by_label` panics on zero **or several** matches. Header titles such as
  `"Size"` and `"%"` occur twice; use `get_all_by_label` or `query_by_label`.
  A file name that also appears as a breadcrumb will match twice too.
- Do not use kittest's `.click()` on row labels. It sends an accessibility
  action to the label, which does not sense clicks; rows take pointer clicks
  on their whole rect. Use the `click` helper. `.click()` is fine for real
  buttons (`"Rescan"`, `"Rows"`, `"Squarified"`, `"Cancel"`).
- Never call `harness.run()` while a scan is running or a footer message is
  fresh: the GUI keeps requesting repaints and `run` panics after its step
  limit. Use `step()` in a loop with your own deadline.
- Keys: `harness.key_press(Key::Enter)` zooms into the selection,
  `Key::Backspace` goes back. Keyboard navigation switches hover pulsing off
  until the pointer moves.
- The temp root's path is different on every run and appears in the toolbar,
  so never assert on it and never compare screenshots to a golden image.
  Software renderers (WARP, lavapipe, Metal) also differ by a few pixels.
- Trash, permanent delete, open and iCloud eviction act on the real system.
  Only exercise them on files inside the test's own tempdir, and gate
  platform-specific ones with the same `cfg` the GUI uses
  (`cfg(all(windows, feature = "trash"))` for the dialogs).
- Tests run in parallel in one process. Do not set environment variables or
  the current directory inside a test.
- Anything that scans outside a tempdir must be `#[ignore = "..."]`; CI runs
  ignored tests with `--include-ignored`, ordinary runs skip them.

## Boundaries

- Keep everything behind the `e2e` feature. `egui_kittest` and `tempfile` are
  optional dependencies of `dirstats-gui`; a default build must not contain
  wgpu (`cargo tree -p dirstats -e normal | grep ' wgpu v'` prints nothing).
- Prefer reading private `Gui` fields from the test module over making them
  `pub`. If the GUI needs a change to be testable, keep it a pure refactor,
  as `configure` was.
- `crates/dirstats-gui` is shared with other sessions: tell the "Main agent"
  session before editing it if one is listed, and work on a branch.
- If `Cargo.lock` changes, `gpu-allocator` must keep depending on
  `windows 0.58.0` (the version `wgpu-hal` uses), not the 0.56 that `trash`
  uses, or the Windows job stops compiling. Check with:

```bash
cargo check -p dirstats-gui --features e2e,egui-fonts --tests --locked --target x86_64-pc-windows-msvc
```

## Before handing over

Run the first command above (add `trash` to run the trash test, which
is ignored by default), then `cargo test --workspace --locked` to show
ordinary tests are untouched. Open one of the PNGs and look at it. On a PR,
the `gui e2e` job's `e2e-screenshots-<os>` artifacts are the evidence for
Linux and Windows; say plainly which platforms you ran yourself.
