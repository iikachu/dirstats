---
name: port-upstream-code
description: Port or adapt code, algorithms or ideas from WinDirStat, Disk Inventory X, dua-cli, dust or any other upstream project into dirstats. Use before writing any code that is derived from, translated from, or closely modelled on someone else's source, to pick the right crate for its licence and to credit it properly.
---

# Porting upstream code

The workspace is split by licence, and the split is the point: the scanner is
permissively licensed under one licence, everything else is GPL. Getting this
wrong once already cost a rewrite of the repository's history.

## 1. Decide where the code may live

| Upstream licence | Examples | May go in |
|---|---|---|
| GPL (any version that allows "or later", or GPL-3.0) | WinDirStat (GPL-2.0-or-later), Disk Inventory X (GPL-3.0) | GPL crates only: `dirstats-treemap`, `dirstats-ntfs`, `dirstats-core`, `dirstats-tui`, `dirstats-gui`, `dirstats` |
| Apache-2.0 | dust | any crate, including `dirstats-scan` |
| MIT, BSD, Zlib | dua-cli / dua-core (MIT) | any crate; keep the licence text in `LICENSES/` |
| GPL-2.0-only, or no licence stated | | nowhere. Stop and ask |

`crates/dirstats-scan` is **Apache-2.0 only** and must contain no GPL-derived
code. "Derived" includes a translation to Rust, a line-by-line re-expression,
and code written while reading the GPL source for that purpose. If a scanner
speed-up comes from GPL code, it goes in its own GPL crate that offers the
same `scan_with` and builds its result with `dirstats_scan::TreeBuilder`, as
`dirstats-ntfs` does.

An idea taken from an issue thread or documentation, implemented
independently, is not a port. Say so in the header, as `scan.rs` does for
dust's block-count cap, and credit the idea anyway.

## 2. File header

Every source file starts with the SPDX line for its crate, then the credit
line, then the provenance of anything ported:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// Port of WinDirStat's `FinderNtfs.cpp` (GPL-2.0-or-later, by WinDirStat
// Team, https://windirstat.net). See `CREDITS.md` in the repository.
```

Credit lines are written "by <who>". Do not use the © symbol or the word
"Copyright" in text we write ourselves. Upstream licence texts under
`LICENSES/` stay verbatim, whatever they say.

Name the upstream file and, where it helps a reader compare, the functions
(`DrawCushion`, `AddRidge`). Credit the people upstream credits: check its
CONTRIBUTORS or AUTHORS file rather than naming only the project.

## 3. CREDITS.md and LICENSES/

- Add or extend the project's section in `CREDITS.md`: project URL, "By"
  line, its licence and the licence we use it under, and a "Used in" list
  mapping each of our files to the upstream file it came from.
- Keep the crate/licence table at the top of `CREDITS.md` true. A new crate
  gets a row.
- First use of a project: add its licence text to `LICENSES/` as
  `<SPDX-id>-<project>.<ext>`, unmodified.
- A new crate's `Cargo.toml` carries the matching `license =` field.

## 4. Check before handing over

```bash
grep -rL 'SPDX-License-Identifier' crates/*/src
```

```bash
grep -rn 'SPDX-License-Identifier' crates/dirstats-scan/src | grep -v 'Apache-2.0'
```

Both should print nothing. Then read the diff of `crates/dirstats-scan` once
more and ask of each new function: could I have written this without the GPL
source open? If not, it is in the wrong crate.

Say in the commit message what was ported and from where.
