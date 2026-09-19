# Architecture

dirstats collects the best parts of the reference projects (WinDirStat,
dua-cli, dust, Disk Inventory X, the HPI treemap prototype) into one Rust
workspace with a clear separation of concerns, feature-flagged build
targets, and per-platform fast paths for every well-known filesystem.

## Layers

Each layer is a crate. A crate depends only on layers below it and never on
a front end.

| Layer | Crate | Licence | Responsibility |
|---|---|---|---|
| Model | `dirstats-scan` (`tree` module) | Apache-2.0 | Arena tree, node kinds, size metrics, sorting, extension stats |
| Scan | `dirstats-scan` (`scan` + `platform::*`) | Apache-2.0 | Parallel traversal, hard links, volume boundaries, per-filesystem fast paths |
| Persist | `dirstats-scan` (feature `serde`) | Apache-2.0 | Save and load scans |
| Layout | `dirstats-treemap` (`layout`) | GPL-3.0-or-later | Rows, squarified, Hilbert, Moore |
| Render | `dirstats-treemap` (`render`) | GPL-3.0-or-later | Cushion shading, colour schemes, hit testing, frames and labels |
| NTFS | `dirstats-ntfs` | GPL-3.0-or-later | Whole-volume scan from the master file table (Windows, needs administrator rights); walks with `dirstats-scan` otherwise |
| App | `dirstats-app` | GPL-3.0-or-later | The library front ends and other programs use: state (current scan, selection, zoom, sort), the worker threads for scans and permanent deletes, and file actions (open, reveal, trash, put back, iCloud evict, gated permanent delete). Re-exports `dirstats-scan` as `scan` and `dirstats-treemap` as `treemap` |
| Front end | `dirstats-tui`, `dirstats-gui` | GPL-3.0-or-later | Presentation and input only; no scanning or layout logic |
| Binary | `dirstats` (`src/main.rs`) | GPL-3.0-or-later | CLI parsing, picks a front end by feature flag |

```
dirstats ─┬─ dirstats-gui ─┐
          ├─ dirstats-tui ─┤
          └────────────────┴─ dirstats-app ─┬─ dirstats-treemap ─ dirstats-scan
                                            ├─ dirstats-scan
                                            └─ dirstats-ntfs (Windows) ─ dirstats-scan
```

Rules:
- Front ends and the binary depend on `dirstats-app` only, not on
  `dirstats-scan` or `dirstats-treemap`; they reach those through
  `dirstats_app::scan` and `dirstats_app::treemap`. TUI and GUI must be
  swappable without touching scan or treemap code.
- Scanning never blocks a front end: `scan_with` runs on a worker thread and
  reports through `Progress` and a cancel flag.
- `dirstats-scan` stays free of GPL-derived code (see
  `CREDITS.md`). GPL-derived fast paths (for example an NTFS MFT reader from
  WinDirStat) go in a separate GPL crate that offers the same `scan_with`
  and builds its result with `dirstats_scan::TreeBuilder`.

## Build targets and feature flags

The top-level `dirstats` crate is both a library and a binary. Features
follow dua-cli's convention: one flag per front end, capability flags kept
separate, defaults chosen for the common case.

```toml
[features]
default    = ["gui", "tui", "open", "trash", "delete", "icloud", "ntfs-mft"]
gui        = ["dep:dirstats-gui"]  # egui window
tui        = ["dep:dirstats-tui"]  # ratatui + crossterm; --tui, or chosen when there is no desktop
open       = [...]                 # open and reveal via the desktop
trash      = [...]                 # move-to-trash action
delete     = [...]                 # gated permanent delete (Windows, Linux); independent of trash
ntfs-mft   = [...]                 # NTFS volumes from the master file table (Windows, admin)
icloud     = [...]                 # iCloud "Remove Download" (macOS; no-op elsewhere)
png        = ["dep:png"]           # --png writes a cushion treemap image
egui-fonts = [...]                 # egui's bundled fonts, for comparison
```

`open`, `trash`, `delete`, `icloud` and `ntfs-mft` are owned by
`dirstats-app`; the actions among them are forwarded to whichever front
ends are built, with `dep?/feature`.

With both front ends built the binary picks one at run time
(`dirstats::session`): `--tui` or `--gui` decide outright; otherwise it
opens a window when the session looks graphical (`WAYLAND_DISPLAY` or
`DISPLAY` on Linux and the BSDs, not an SSH session on macOS, always on
Windows),
uses the terminal when it does not, and falls back to the terminal if the
window fails to open (except on Windows, where the error is reported). With no terminal either, it prints the summary.

Per-crate features:
- `dirstats-app`: `open`, `trash`, `delete`, `icloud`, `ntfs-mft`; none by
  default. `ntfs-mft` pulls in `dirstats-ntfs` on Windows only; without it Windows walks
  directories like every other platform.
  `trash` and `delete` are independent. `App`'s permanent delete exists
  on Windows, Linux and macOS, and `delete::delete_permanently` on every
  platform, but the GUI only offers it on Windows and Linux, so the macOS
  GUI and TUI without `trash` cannot remove files.
- `dirstats-gui`: forwards those, plus `egui-fonts` and `e2e` (headless
  end-to-end tests, CI only).
- `dirstats-tui`: forwards `open`, `trash` and `ntfs-mft`.
- `dirstats-treemap`: `parallel` (rayon cushion rendering), on by default.
  `dirstats-app` keeps it on and nothing forwards it: every target dirstats
  ships for has threads, and a single-threaded or WebAssembly build is not
  planned. Library users can still depend on `dirstats-treemap` with
  `default-features = false`.
- `dirstats-scan`: none yet. Fast paths are chosen by `cfg` per platform;
  `serde` for saved scans is planned.

`cargo build` gives the binary with both front ends.
`cargo build --no-default-features` still builds a binary, which prints
the summary. CI checks every feature on its own and with defaults off.

## Platform and filesystem support

Every platform gets the fastest supported traversal and correct size
accounting for each well-known filesystem. The scan layer exposes one
`platform` trait with these responsibilities, each implemented per OS:

| Concern | macOS | Linux | Windows |
|---|---|---|---|
| Bulk enumeration | `getattrlistbulk` (dua-core) | `getdents64` + `statx` via `std::fs` (dua-core); a custom walker was tried and dropped, see #18 | `FileIdBothDirectoryInfo` (dua-core); NTFS master file table for whole drives (`dirstats-ntfs`, done) |
| Volume boundary | `st_dev` | `st_dev` / `statx` mount id | volume serial from `GetFileInformationByHandle` (planned) |
| Hard links | `st_nlink` + inode set | `st_nlink` + inode set | file ID set; `nlink` via handle (planned) |
| Allocated size | `st_blocks`; APFS clone accounting (planned) | `st_blocks`; cap inflated NTFS mounts (done) | allocation size from enumeration; compressed and sparse (planned) |
| Filesystem quirks | firmlinks, packages (from Disk Inventory X); Time Machine backups are not offered for trash (done) | bind mounts, btrfs subvolumes | reparse points, junctions, OneDrive placeholders |

Detection of the filesystem type is done once per volume so the scan picks
the right strategy without per-entry cost.

## Using dirstats-app as a library

Other programs use the same crate the front ends do. Two ways in:

- An `App`, as the GUI and TUI do: scans on a worker thread, a selection,
  actions by node id, and state that remembers what was trashed or
  deleted.
- Plain functions on paths, which `App`'s actions are built on:
  `scanner::scan` (blocking scan), `trash::move_to_trash` and
  `trash::put_back`, `delete::delete_permanently` (blocking), and
  `cloud::evict`. Each removal checks `check_removable` first: no drive
  or filesystem roots, no home folder, no Time Machine backups on macOS.
  Nobody is asked; confirming is the caller's job.

`crates/dirstats-app/examples/largest.rs` scans a folder, lists its
largest entries and, with `--trash`, moves them to the trash.

## Front ends

- TUI (now): ratatui + crossterm, modelled on dua-cli's interactive mode
  plus a cell-based treemap. Keyboard-first, works over SSH.
- GUI (now): eframe/egui with the same `dirstats-app` state. The pixel
  treemap from `dirstats_app::treemap::render` is uploaded as a texture and
  re-rendered only when the tree, directory, layout or panel size changes.
  Hover uses the grid hit-test index; click reveals, double-click zooms,
  right-click opens or trashes. On Windows and Linux the menu also offers
  permanent deletion behind a gate that lasts until the app quits (nothing
  is saved between runs), done by the app's own bottom-up
  walker (as WinDirStat does) rather than the shell, with its own
  confirmation, progress and failure report; the trash path stays with the
  trash crate, which refuses rather than nukes items the Recycle Bin or a
  Linux mount without a writable `.Trash-$UID` folder cannot take, and the
  refusal offers permanent delete as the next step. The macOS GUI
  deliberately leaves it out, though the library has it (clearing
  Finder's Locked flag where needed): the Finder trash accepts items on every local and most network
  volumes, so the escape hatch would only add a way to lose data, and Time
  Machine snapshots are guarded separately (`backup::BLOCKS_TRASH`).

## Roadmap

1. Done: `dirstats-app`, `dirstats-tui`, and the `dirstats` lib+bin crate
   with the feature flags above; `--png` replaces the treemap example.
2. Add a CI matrix (macOS, Linux, Windows) so all platform code compiles.
3. Fill the platform gaps listed above, starting with Windows volume
   boundaries and directory error marking.
4. Done: size-ranked extension colours, grid hit-test index, parallel
   rasterisation, GUI front end.
5. Hilbert and Moore layouts; frames and labels.
6. Save and load scans; benchmarks against dua, gdu, ncdu.
7. App bundles and icons.
