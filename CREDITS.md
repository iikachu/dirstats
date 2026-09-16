# Credits

dirstats is a workspace of two crates with different licenses:

| Crate | License | Files |
|---|---|---|
| `crates/dirstats-scan` | Apache-2.0 (`LICENSE-APACHE`) | `tree.rs`, `scan.rs` |
| `crates/dirstats-treemap` | GPL-3.0-or-later (`LICENSE-GPL-3.0`) | `layout.rs`, `render.rs`, examples |
| `crates/dirstats-app`, `crates/dirstats-tui`, `crates/dirstats` | GPL-3.0-or-later (`LICENSE-GPL-3.0`) | app state, terminal UI, binary |

`dirstats-scan` contains no GPL-derived code and must stay that way: code
ported from WinDirStat or Disk Inventory X belongs in `dirstats-treemap` (or
another GPL crate). License texts for upstream projects are in `LICENSES/`.

## WinDirStat — treemap layout and cushion rendering

- Project: https://github.com/windirstat/windirstat
- By WinDirStat Team (https://windirstat.net). Original author
  Bernhard Seifert; major contributors Bryan Berns and Oliver Schneider;
  treemap rendering improvements by Falco Peijnenburg and Morten Asscheman.
  See upstream `CONTRIBUTORS.md`.
- License: GPL-2.0-or-later (`LICENSES/GPL-2.0-windirstat.md`), used in
  `dirstats-treemap` under GPL-3.0-or-later.
- Used in:
  - `crates/dirstats-treemap/src/layout.rs`: port of `Controls/TreeMapLayout.cpp` (rows and
    squarified layouts).
  - `crates/dirstats-treemap/src/render.rs`: port of `Controls/TreeMap.cpp` / `TreeMap.h`
    (`DrawTreeMap`, `DrawCushion`, `AddRidge`, `CColorSpace`, default
    palette and "Classic" options).

## dua-cli / dua-core — parallel traversal and tree design

- Project: https://github.com/Byron/dua-cli
- By Sebastian Thiel
- License: MIT (`LICENSES/MIT-dua-cli.txt`)
- Used in:
  - Dependency `dua-core`: parallel, work-stealing directory traversal
    (`getattrlistbulk` on macOS, `FileIdBothDirectoryInfo` on Windows).
  - `crates/dirstats-scan/src/tree.rs`: arena-based tree design follows
    dua-cli's `Tree`.
  - `crates/dirstats-scan/src/scan.rs`: hard-link accounting approach
    follows `inodefilter.rs`.
  - `crates/dirstats-tui`: keyboard model and feature-flag layout follow
    dua-cli's interactive mode.

## Material Symbols — icons

- Project: https://github.com/google/material-design-icons
- By Google
- License: Apache-2.0 (`crates/dirstats-gui/assets/material-symbols/LICENSE`),
  used in `dirstats-gui` under GPL-3.0-or-later.
- Glyphs used: `chevron_right` (also mirrored for back), `expand_more`,
  `content_copy`, `open_in_new`, `delete` and `undo`, all from Material
  Symbols Outlined at 24px. Their SVG path data is inlined in the `icons`
  module of `crates/dirstats-gui/src/lib.rs` and rasterised at run time;
  the module header carries the attribution. The first two SVGs are also
  vendored in `crates/dirstats-gui/assets/material-symbols/`.

## egui / eframe — GUI toolkit

- By Emil Ernerfeldt and contributors, MIT OR Apache-2.0. Used as a
  dependency of `crates/dirstats-gui`.

## ratatui, crossterm, clap, open, trash, rayon — runtime dependencies

- ratatui (MIT, by the ratatui developers), crossterm (MIT, by Timon and
  contributors), clap (MIT OR Apache-2.0, by the clap developers), open
  (MIT, by Sebastian Thiel), trash (MIT OR Apache-2.0, by Artur Kovacs).
  Used as ordinary crate dependencies.

## dust — Unix size accounting idea

- Project: https://github.com/bootandy/dust
- By bootandy, nebkor and dust contributors
- License: Apache-2.0
- Credited in `crates/dirstats-scan/src/scan.rs`: the idea of capping
  inflated `st_blocks` values on Linux NTFS mounts (dust issue #295). The
  code was written independently, so no dust code is included.

## Disk Inventory X — macOS reference

- Project: Disk Inventory X by Tjark Derlien
- By Tjark Derlien
- License: GPL-3.0-or-later (same as `dirstats-treemap`)
- Used as a behavioural reference for macOS scanning (firmlinks, volume
  boundaries, allocated size via `NSURLTotalFileAllocatedSizeKey`). No code
  has been ported yet.

## OKLab — perceptual colour

- By Björn Ottosson, https://bottosson.github.io/posts/oklab/ (public domain / MIT).
- Used in `crates/dirstats-treemap/src/color.rs`: sRGB ⇄ OKLab/OKLCH
  conversion matrices. All palette generation and shading is done in OKLCH.

## Research

- B. Shneiderman, "Tree visualization with tree-maps: 2-d space-filling
  approach", ACM Transactions on Graphics, 1992.
- J. J. van Wijk, H. van de Wetering, "Cushion Treemaps: Visualization of
  Hierarchical Information", IEEE InfoVis 1999.
- M. Bruls, K. Huizing, J. J. van Wijk, "Squarified Treemaps", EuroVis 2000.
