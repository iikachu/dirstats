# dirstats marketing kit

Ready-to-use copy for announcing and describing dirstats. Everything here
matches what the app does today. If a feature changes, update this file along
with the main [README](../README.md). Don't promise anything from the
roadmap as if it has shipped.

Screenshots are not kept in the repository, because `*.png` is git-ignored:
they show scanned paths. Take them on a demo folder with no personal names in
it (see [Screenshot checklist](#screenshot-checklist)).

## Name and taglines

- **Name:** dirstats. Always lowercase, including at the start of a sentence
  where you can avoid it.
- **Primary tagline:** See what is filling your disk.
- **Alternates:**
  - Every file a rectangle. The big ones are impossible to miss.
  - A fast, visual disk usage map for Linux, macOS and Windows.
  - Find the space hogs in seconds, in a window or over SSH.

## Short descriptions

**One line (up to 80 characters):**

> dirstats: a fast treemap disk usage viewer for Linux, macOS and Windows.

**One sentence:**

> dirstats scans a folder or a whole drive in parallel and draws it as a
> treemap, where every file is a rectangle sized by the space it takes and
> coloured by type, so the big things are impossible to miss.

**One paragraph:**

> dirstats shows you what is filling your disk. It scans a folder or a whole
> drive on all your cores and draws the result as a softly shaded treemap
> next to a size-sorted list. The largest file types each get their own
> colour, so a wall of videos or build artefacts stands out right away. From
> there you can zoom into any folder, open files, or move them to the trash.
> It runs on Linux, macOS and Windows, as a desktop app or in your terminal,
> and it is free and open source.

## Key messages

1. **Fast.** Parallel scanning on every core. On Windows, running as
   administrator reads whole NTFS drives straight from the master file table.
2. **Visual.** A cushion-shaded treemap beside a sorted list. Hover, click,
   zoom.
3. **Colour by file type.** The top file types get their own hue and a legend.
4. **Act on it.** Open a file or folder, or move it to the trash. Deletion is
   never permanent by default.
5. **Honest numbers.** Real on-disk size, each hard link counted once, other
   mounted disks left out unless you ask.
6. **Anywhere.** The same view in a terminal, so it works over SSH.
7. **Open source.** The app is GPL-3.0-or-later. The scanning library,
   `dirstats-scan`, is Apache-2.0 for use in other projects.

## Launch announcement (blog / forum post)

> ### dirstats: see what is filling your disk
>
> Disks fill up quietly. The usual tools give you a long list of numbers.
> dirstats gives you a picture.
>
> Point it at a folder or a whole drive. It scans on all your cores and draws
> a treemap. Every file is a rectangle, sized by the space it uses and
> coloured by its type. The folder of forgotten videos, the stale build
> directory, the 40 GB of old VM images: you see them at a glance.
>
> **What it does**
>
> - Scans in parallel. On Windows, as administrator, it reads NTFS drives
>   directly from the master file table.
> - Shows a shaded treemap next to a size-sorted list. Hover to identify a
>   block, click to select it, zoom into any folder.
> - Gives the biggest file types their own colours, with a legend.
> - Lets you open files and folders, or move them to the trash, from inside
>   the app.
> - Reports real disk usage, counts hard links once, and stays on one disk
>   unless you tell it otherwise.
> - Offers the same view in the terminal (`dirstats --tui`), a quick text
>   summary (`--summary`), and PNG export (`--png`).
>
> dirstats runs on Linux, macOS and Windows. It is written in Rust and is
> free and open source. The scanner is also available as a separate
> Apache-2.0 crate.
>
> **Try it**
>
> ```bash
> cargo install --path crates/dirstats --features tui,png
> dirstats ~/Downloads
> ```

## Social posts

**Short (fits in about 280 characters):**

> dirstats: see what is filling your disk. A fast treemap disk usage viewer
> for Linux, macOS and Windows. Every file is a rectangle, coloured by type.
> Desktop app or terminal. Free and open source, written in Rust.

**Thread:**

1. Where did all my disk space go? dirstats shows you as a picture instead
   of a list. 🧵
2. It scans on all your cores and draws a treemap. Every file is a rectangle
   sized by the space it uses. The big ones are impossible to miss.
3. The largest file types get their own colours, so a pile of videos, VM
   images or build output jumps out.
4. Found something? Open it, or send it to the trash, without leaving the
   app.
5. On a server? `dirstats --tui` gives the same view in your terminal, over
   SSH.
6. Linux, macOS, Windows. GPL-3.0 app, Apache-2.0 scanning library. Written
   in Rust.

**For developers (Rust community):**

> dirstats is a set of layered Rust crates: a parallel scanner (Apache-2.0),
> a cushion treemap renderer, and shared app state behind both an egui and a
> ratatui front end. There's an NTFS master file table fast path on Windows.
> Feedback and PRs welcome.

## Store / package listing

**Title:** dirstats: disk usage treemap

**Summary:** See what is filling your disk, as a treemap.

**Description:**

> dirstats scans a folder or a whole drive and draws it as a treemap. Every
> file is a rectangle, sized by its disk usage and coloured by its type, so
> the biggest space users stand out right away.
>
> - Fast, parallel scanning (NTFS master file table reading on Windows)
> - Shaded treemap beside a size-sorted list
> - Colour by file type, with a legend
> - Zoom into folders, open files, move items to the trash
> - Accurate on-disk sizes; hard links counted once
> - Terminal interface for SSH sessions
>
> Free and open source.

**Keywords:** disk usage, disk space, treemap, storage analyzer, du,
WinDirStat alternative, cleanup, file size, TUI

## Comparisons

When comparing dirstats with other tools, be fair and specific. dirstats
builds on ideas from WinDirStat, Disk Inventory X, dust and dua-cli (see
[CREDITS.md](../CREDITS.md)). Frame it by what it adds, not by what others
lack:

- *Like WinDirStat and Disk Inventory X:* a treemap coloured by file type,
  plus Linux support and a terminal mode.
- *Like dust and dua-cli:* fast parallel scanning from the terminal, plus a
  graphical treemap.

## Screenshot checklist

- Scan a demo folder built for the purpose, not a real home directory. Make
  sure no username, machine name or private file name is visible.
- Show a mix of file types so the colour legend has something to show.
- Take one GUI shot (list and treemap), one TUI shot, and one `--png` export.
- Take both light and dark system themes if the GUI follows them.
- Don't commit the images here. Attach them to the release or website.
