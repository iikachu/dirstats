# dirstats-core

The library behind [dirstats](https://github.com/iikachu/dirstats), a disk usage viewer with treemaps:
application state shared by every front end, scans on a worker thread, and
file actions (open, move to trash and put back, permanent delete, iCloud
"Remove Download"), each behind a feature flag. It re-exports the scanner
and the treemap, so it is the only dirstats crate another program needs.

`examples/largest.rs` scans a folder and lists its largest entries without
an `App`.

## License

GPL-3.0-or-later. See `CREDITS.md` for the projects this crate builds on and
`UPSTREAM-CONTRIBUTORS.md` for the people behind them.
