# dirstats

Cross-platform (Linux, macOS, Windows) disk usage scanning and cushion
treemap rendering, in Rust. It combines WinDirStat's treemap algorithms with
dua-cli's parallel native traversal.

| Crate | What it does | License |
|---|---|---|
| [`dirstats-scan`](crates/dirstats-scan) | Parallel scan into a compact, size-sorted tree | Apache-2.0 |
| [`dirstats-treemap`](crates/dirstats-treemap) | Rows/squarified layouts, cushion rendering, hit testing | GPL-3.0-or-later |

```rust
use dirstats_scan::{scan, ScanOptions};
use dirstats_treemap::{render, Style, TreemapOptions};
use dirstats_treemap::render::{color_by_extension, default_palette};

let tree = scan("/some/dir", &ScanOptions::default())?;
for &child in tree.children(tree.root()) {
    println!("{:>12} {}", tree.size(child), tree.path(child).display());
}
let palette = default_palette();
let options = TreemapOptions { style: Style::Squarified, ..Default::default() };
let map = render(&tree, tree.root(), 1600, 1000, &options, color_by_extension(&palette));
// map.pixels is RGBA8; map.hit_test(x, y) finds the node under a pixel.
```

Try it:

```sh
cargo run --release -p dirstats-treemap --example treemap -- ~/Downloads treemap.png squarified
```

## Status (first draft)

Done:
- Parallel scan via `dua-core`, compact arena tree, sorted children
- Allocated or apparent size, hard links counted once, same-filesystem limit (Unix)
- Rows and squarified layouts, cushion shading, hit testing

Planned:
- Hilbert/Moore layouts (from the MIT-licensed HPI prototype)
- NTFS MFT fast path (WinDirStat `FinderNtfs.cpp`; GPL, so a separate crate)
- Linux `statx`/`getdents64` fast path
- APFS clone accounting (`dua-core` `apfs_clone_metadata`)
- Volume boundaries on Windows, parallel cushion rendering, grid hit-test
  index, extension colors ranked by size, saving and loading scans

## License

Each crate states its own license; see the table above and
[CREDITS.md](CREDITS.md). Code derived from GPL projects must not be added
to `dirstats-scan`.
