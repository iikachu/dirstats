---
name: add-fs-fast-path
description: Add a platform- or filesystem-specific fast scan path to dirstats (for example Linux getdents64/statx, APFS clone accounting, btrfs subvolumes, another whole-volume reader like the NTFS master file table one). Use when asked to make scanning faster or more correct on one OS or filesystem.
---

# Adding a filesystem fast path

Read `ARCHITECTURE.md` ("Layers" and "Platform and filesystem support") and
`crates/dirstats-ntfs/src/lib.rs` first. The NTFS reader set the pattern.

## 1. Where it goes

- Written independently, or from Apache/MIT sources: inside
  `crates/dirstats-scan` under a `platform` module, behind a per-platform
  feature that is on by default on its platform (`linux-fast`, ...).
- Derived from GPL code (WinDirStat, Disk Inventory X): **never** in
  `dirstats-scan`. Make a separate GPL-3.0-or-later crate. See the
  `port-upstream-code` skill before writing a line.

## 2. The contract

A fast path is a drop-in for `dirstats_scan::scan_with`:

```rust
pub fn scan_with(root: impl AsRef<Path>, options: &ScanOptions, cancel: &AtomicBool, progress: &Progress) -> io::Result<Tree>
```

- Build the result with `dirstats_scan::TreeBuilder`, so sizes, hard links
  and extension stats mean the same thing as in a walked tree.
- Honour every `ScanOptions` field (`same_filesystem`,
  `count_hard_links_once`, `size_metric`, `threads`). If the fast path cannot
  honour one, do not take it for that call: the NTFS reader only runs when
  `same_filesystem` is set, because a file table cannot follow mount points.
- Update `progress.entries` and `progress.errors` as you go; front ends show
  them live. Check `cancel` often and return `io::ErrorKind::Interrupted`.
- **Always fall back.** On any failure other than `Interrupted` (wrong
  filesystem, no privilege, unreadable structure), reset
  `progress.entries` to 0 and call `dirstats_scan::scan_with`. A fast path
  must never turn a scan that used to work into an error.
- Detect the filesystem once per volume, not per entry.

## 3. Wiring

Front ends never call a scanner. They go through
`crates/dirstats-app/src/scanner.rs` (`RunningScan::spawn`), and the CLI's
`--summary` and `--png` paths in `crates/dirstats/src/lib.rs` call `scan`
directly. A separate-crate fast path is chained in front of the current
entry point (today `dirstats_ntfs::scan_with`, which itself falls through to
`dirstats_scan`), so GUI, TUI and CLI all get it with no front-end change.
Add the crate to `[workspace.dependencies]` in the root `Cargo.toml`.

Gate OS-specific code and dependencies with `cfg` and
`[target.'cfg(...)'.dependencies]`, so every crate still builds everywhere
and simply walks where the fast path does not apply.

## 4. Tests

- Unit-test the parser on captured or hand-built raw structures, in a module
  that compiles on every OS (`mft.rs` is not `cfg(windows)`; only the volume
  handle code is). That way the logic is tested on all three CI runners.
- Compare against the walker: scan the same fixture both ways and assert
  equal file counts and sizes under each `SizeMetric`.
- The nightly `disk scan` jobs scan each runner's whole system disk through
  the real entry point; the log prints scan time, file count and total size.
  Start the `Nightly` workflow on your branch from the Actions tab and quote
  before and after figures from it in the PR. Windows runners are elevated,
  so privileged paths do run there.
- From a Mac, at least type-check the Windows side (see the
  `windows-check-from-mac` skill).

## 5. Documents

Update the support table in `ARCHITECTURE.md` (move the cell from "planned"
to "done"), the layer table if a crate was added, and `CREDITS.md` if
anything was ported. Commit as `scan:` or under the new crate's area.
