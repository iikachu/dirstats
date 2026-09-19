# dirstats-ntfs

The NTFS fast path of [dirstats](https://github.com/iikachu/dirstats): on Windows it scans a whole NTFS
volume by reading its master file table instead of walking directories,
which takes administrator rights. Elsewhere, and whenever it cannot read
the table, it walks directories with
[`dirstats-scan`](https://crates.io/crates/dirstats-scan).

The master file table reader is ported from WinDirStat.

## License

GPL-3.0-or-later. See `CREDITS.md` for the projects this crate builds on and
`UPSTREAM-CONTRIBUTORS.md` for the people behind them.
