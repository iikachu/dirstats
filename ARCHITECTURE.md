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
| App | `dirstats-app` | GPL-3.0-or-later | Front-end-agnostic state: current scan, selection, zoom, sort, actions (open, reveal, trash; on Windows also gated permanent delete), persisted settings |
| Front end | `dirstats-tui`, `dirstats-gui` | GPL-3.0-or-later | Presentation and input only; no scanning or layout logic |
| Binary | `dirstats` (`src/main.rs`) | GPL-3.0-or-later | CLI parsing, picks a front end by feature flag |

Rules:
- Front ends talk to `dirstats-app` only. TUI and GUI must be swappable
  without touching scan or treemap code.
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
default = ["gui", "trash"]
gui   = ["dep:dirstats-gui"]        # egui window
tui   = ["dep:dirstats-tui"]        # ratatui + crossterm front end; --tui at run time
trash = ["dirstats-app/trash"]      # move-to-trash action
serde = ["dirstats-scan/serde"]     # save/load scans
```

Per-crate features:
- `dirstats-scan`: `serde`; `linux-fast` (statx/getdents64), `macos-fast`
  (getattrlistbulk, already via dua-core), `windows-fast`
  (FileIdBothDirectoryInfo, already via dua-core). Fast paths are on by
  default on their platform and fall back to the generic walker.
- `dirstats-treemap`: `parallel` (rayon cushion rendering), `png`
  (example output).

`cargo build` gives the GUI binary. `cargo build --no-default-features`
gives only the library. `cargo build --features tui` adds the terminal
front end.

## Platform and filesystem support

Every platform gets the fastest supported traversal and correct size
accounting for each well-known filesystem. The scan layer exposes one
`platform` trait with these responsibilities, each implemented per OS:

| Concern | macOS | Linux | Windows |
|---|---|---|---|
| Bulk enumeration | `getattrlistbulk` (dua-core) | `getdents64` + `statx` in inode order (`linux-fast`, done) | `FileIdBothDirectoryInfo` (dua-core); NTFS master file table for whole drives (`dirstats-ntfs`, done) |
| Volume boundary | `st_dev` | `st_dev` / `statx` mount id | volume serial from `GetFileInformationByHandle` (planned) |
| Hard links | `st_nlink` + inode set | `st_nlink` + inode set | file ID set; `nlink` via handle (planned) |
| Allocated size | `st_blocks`; APFS clone accounting (planned) | `st_blocks`; cap inflated NTFS mounts (done) | allocation size from enumeration; compressed and sparse (planned) |
| Filesystem quirks | firmlinks, packages (from Disk Inventory X); Time Machine backups are not offered for trash (done) | bind mounts, btrfs subvolumes | reparse points, junctions, OneDrive placeholders |

Detection of the filesystem type is done once per volume so the scan picks
the right strategy without per-entry cost.

## Front ends

- TUI (now): ratatui + crossterm, modelled on dua-cli's interactive mode
  plus a cell-based treemap. Keyboard-first, works over SSH.
- GUI (now): eframe/egui with the same `dirstats-app` state. The pixel
  treemap from `dirstats-treemap::render` is uploaded as a texture and
  re-rendered only when the tree, directory, layout or panel size changes.
  Hover uses the grid hit-test index; click reveals, double-click zooms,
  right-click opens or trashes. On Windows the menu also offers permanent
  deletion behind a one-time gate, done by the app's own bottom-up walker
  (as WinDirStat does) rather than the shell, with its own confirmation,
  progress and failure report; the Recycle Bin path stays with the trash
  crate, which refuses rather than nukes items the bin cannot take.

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
