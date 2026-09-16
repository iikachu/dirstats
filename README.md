# dirstats

Cross-platform (Linux, macOS, Windows) disk usage scanning and treemaps, in
Rust. It collects the best parts of WinDirStat, dua-cli, dust and Disk
Inventory X behind one layered workspace; see [ARCHITECTURE.md](ARCHITECTURE.md).

| Crate | What it does | License |
|---|---|---|
| [`dirstats-scan`](crates/dirstats-scan) | Parallel scan into a compact, size-sorted tree | Apache-2.0 |
| [`dirstats-treemap`](crates/dirstats-treemap) | Rows/squarified layouts, OKLCH glow rendering, hit testing | GPL-3.0-or-later |
| [`dirstats-app`](crates/dirstats-app) | Front-end-agnostic state: background scan, navigation, actions | GPL-3.0-or-later |
| [`dirstats-tui`](crates/dirstats-tui) | Terminal interface (ratatui): entry list and cell treemap | GPL-3.0-or-later |
| [`dirstats-gui`](crates/dirstats-gui) | Graphical interface (egui): entry list, glow treemap, legend | GPL-3.0-or-later |
| [`dirstats`](crates/dirstats) | Library + binary; picks a front end by feature flag | GPL-3.0-or-later |

```sh
cargo install --path crates/dirstats     # TUI with open and trash actions
dirstats ~/Downloads                     # interactive
dirstats --summary ~/Downloads           # print the largest entries
cargo run -p dirstats --features gui -- --gui ~/Downloads   # window
cargo run -p dirstats --features png -- --png map.png ~/Downloads   # --shading glow|flat
```

Features of the `dirstats` crate: `tui` (default), `open` (default), `trash`
(default), `png`, and `gui`. With both front ends built, `--gui` opens the
window; a `gui`-only build always does. `--no-default-features` builds the
library only.

## Library use

```rust
use dirstats_scan::{scan, ScanOptions};
use dirstats_treemap::{render, Style, TreemapOptions};
use dirstats_treemap::ExtensionColors;

let tree = scan("/some/dir", &ScanOptions::default())?;
for &child in tree.children(tree.root()) {
    println!("{:>12} {}", tree.size(child), tree.path(child).display());
}
let colors = ExtensionColors::rank(&tree); // hues by extension size rank
let options = TreemapOptions { style: Style::Squarified, ..Default::default() };
let map = render(&tree, tree.root(), 1600, 1000, &options, |t, id| colors.color(t, id));
// colours are OKLCH throughout; map.pixels is RGBA8; map.hit_test(x, y) finds the node under a pixel.
```

## Status

Done:
- Parallel scan via `dua-core`, compact arena tree, sorted children
- Allocated or apparent size, hard links counted once, same-filesystem limit (Unix)
- Rows and squarified layouts; glow shading in OKLCH; hit testing
- Terminal front end with background scanning, keyboard navigation, open and trash
- Graphical front end (egui): hover and click on the treemap, zoom, context
  menu, extension legend
- Grid hit-test index and parallel treemap rasterisation

Planned:
- Hilbert/Moore layouts (from the MIT-licensed HPI prototype)
- NTFS MFT fast path (WinDirStat `FinderNtfs.cpp`; GPL, so a separate crate)
- Linux `statx`/`getdents64` fast path
- APFS clone accounting (`dua-core` `apfs_clone_metadata`)
- Volume boundaries on Windows, saving and loading scans
- CI across all three platforms, app bundles

## License

Each crate states its own license; see the table above and
[CREDITS.md](CREDITS.md). Code derived from GPL projects must not be added
to `dirstats-scan`.
