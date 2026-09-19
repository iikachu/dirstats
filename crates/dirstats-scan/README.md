# dirstats-scan

Parallel, cross-platform disk usage scanning into a compact tree, whose
children are sorted by size. Sizes are the space used on disk or the file
length, hard links are counted once, and other filesystems are skipped
unless asked for.

This is the scanner of [dirstats](https://github.com/iikachu/dirstats), a disk usage viewer with treemaps.
It depends on no GPL code and can be used on its own.

## License

Apache-2.0. See `CREDITS.md` for the projects this crate builds on and
`UPSTREAM-CONTRIBUTORS.md` for the people behind them.
