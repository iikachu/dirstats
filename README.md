# dirstats

**See what is filling your disk.** dirstats scans a folder or a whole drive
and draws it as a treemap: every file is a rectangle, sized by the space it
takes and coloured by its type. The big things are impossible to miss.

It runs on Linux, macOS and Windows, as a desktop app or right in your
terminal.

## Why dirstats

- **Fast.** Scans in parallel on all your cores. On Windows, run it as
  administrator and whole NTFS drives are read straight from the master file
  table.
- **Visual.** A softly shaded treemap beside a size-sorted list. Hover to see
  what a block is, click to select it, zoom into any folder.
- **Colour by file type.** The largest file types get their own hue, with a
  legend, so a wall of videos or build artefacts stands out at once.
- **Act on what you find.** Open a file or folder, or move it to the trash
  (not a permanent delete), straight from the app.
- **Honest numbers.** Sizes are the space really used on disk, hard links are
  counted once, and other mounted disks are left out unless you ask.
- **Works over SSH too.** The same view, in a terminal.

## Install

You need [Rust](https://rustup.rs). Then, from this folder:

```bash
cargo install --path crates/dirstats
```

To also get PNG export:

```bash
cargo install --path crates/dirstats --features png
```

## Use

Open the window and pick your home folder, a disk or the root to scan:

```bash
dirstats
```

Or go straight to a folder:

```bash
dirstats ~/Downloads
```

In the window, the list on the left and the treemap on the right show the
same folder. Click a block to select it, and right-click for zoom, open and trash.

### In the terminal

```bash
dirstats --tui ~/Downloads
```

dirstats uses the terminal on its own when there is no desktop to open a
window on: a Linux console, or an SSH session without X forwarding. Use
`--gui` to open a window anyway.

| Key | Action |
|---|---|
| `↑` `↓` or `k` `j` | Move selection |
| `PgUp` `PgDn` or `u` `d` | Move ten entries |
| `Home` `End` or `g` `G` | First / last entry |
| `Enter`, `→` or `l` | Go into folder |
| `Backspace`, `←` or `h` | Go back up |
| `o` | Open with the default app |
| `Ctrl+x` | Move to trash |
| `q` or `Esc` | Quit |

### Just the numbers

Print the largest entries and exit:

```bash
dirstats --summary ~/Downloads
```

### Save a treemap image

```bash
dirstats --png map.png ~/Downloads
```

Add `--layout rows` or `--layout squarified` (default), and `--shading glow`
(default) or `--shading flat`.

### Options

| Option | Meaning |
|---|---|
| `-m`, `--metric allocated\|apparent` | Space used on disk (default) or file length |
| `-x`, `--cross-filesystems` | Also scan other disks mounted inside the folder |
| `--count-hard-links` | Count each hard link again instead of once |
| `-j`, `--threads N` | Number of scanning threads (default: all cores) |

Run `dirstats --help` for the full list.

## Roadmap

Even faster scans on Linux, APFS clone accounting, Hilbert/Moore
layouts, saving and loading scans, and ready-to-run app downloads.

## For developers

dirstats is also a set of Rust crates: a scanner, a treemap renderer and the
app state behind both interfaces. See [ARCHITECTURE.md](ARCHITECTURE.md) for
the layout and feature flags. A taste of the library:

```rust
use dirstats_scan::{scan, ScanOptions};
use dirstats_treemap::{render, ExtensionColors, TreemapOptions};

let tree = scan("/some/dir", &ScanOptions::default())?;
let colors = ExtensionColors::rank(&tree);
let map = render(&tree, tree.root(), 1600, 1000, &TreemapOptions::default(),
    |t, id| colors.color(t, id));
// map.pixels is RGBA8; map.hit_test(x, y) finds the node under a pixel.
```

## License

The application is GPL-3.0-or-later; the scanning library `dirstats-scan` is
Apache-2.0. See [CREDITS.md](CREDITS.md) for the projects dirstats builds on.
