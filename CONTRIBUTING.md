# Contributing to dirstats

Contributions are welcome, and the bar to send one is low on purpose.

## The short version

- Open a pull request. You do not need to open an issue first, ask
  permission, or sign anything.
- Small, rough or partial is fine. A failing test that shows a bug, a fix for
  one platform only, or a draft you want an opinion on are all useful. Say
  what state it is in and we will take it from there.
- Use whatever tools you like, including AI assistants and coding agents. You
  do not have to disclose it. You are responsible for what you send: read it,
  run it, and be able to explain it.
- Bug reports, screenshots of odd treemaps, timings from unusual filesystems
  and "this confused me" notes about the docs count as contributions too.

## What we do ask

**1. Licences.** This is the one firm rule. There is no CLA; your
contribution is licensed under the licence of the crate it lands in:

| Crate | Licence |
|---|---|
| `crates/dirstats-scan` | Apache-2.0 |
| every other crate | GPL-3.0-or-later |

`dirstats-scan` must stay free of GPL-derived code. If your change is ported
from, or written while reading, GPL source (WinDirStat, Disk Inventory X and
the like), it belongs in one of the GPL crates. Only send code you have the
right to contribute, say where ported code came from, and credit it in the
file header and in [CREDITS.md](CREDITS.md). Credit lines are written
"by <who>". If you are unsure where something may go, open the PR anyway and
ask in it.

**2. It builds and the tests pass.**

```bash
cargo build --workspace --all-targets
```

```bash
cargo test --workspace
```

CI runs both on Linux, macOS and Windows, so you do not need all three
machines. If you can only test on one, say which. `rustfmt` is not enforced;
match the code around your change rather than reformatting files. Clippy
warnings in code you touched are worth fixing; existing ones are not your
problem.

**3. Keep the layers.** Front ends (`dirstats-gui`, `dirstats-tui`) only
present and take input; state and actions live in `dirstats-core`; scanning
and layout live below that. [ARCHITECTURE.md](ARCHITECTURE.md) has the map.
A change that works but sits in the wrong layer will still be welcomed, and
probably moved.

**4. Be careful with anything that deletes.** Trash and permanent delete act
on real files. Tests for them must only ever touch a temporary directory they
created.

## Nice to have, never required

- A commit subject in the house style, `area: a sentence saying what is now
  true`, for example `gui: the toolbar shows the app's name before a scan`.
  Maintainers will reword on merge if needed.
- A test. GUI behaviour can be tested headlessly; see
  `crates/dirstats-gui/src/e2e.rs` once it is merged.
- A before and after screenshot for visual changes. Check that it does not
  show your home directory or anything else you would rather keep private.

## Platforms you do not have

Much of the code is per-platform. You can type-check the other targets
without owning the machine:

```bash
rustup target add x86_64-pc-windows-msvc x86_64-unknown-linux-gnu
```

```bash
cargo check --workspace --all-targets --target x86_64-pc-windows-msvc
```

That proves it compiles, not that it works; CI and reviewers will help with
the rest.

## Review

Expect a friendly review aimed at getting your change in, not at finding
reasons to decline it. Maintainers may push small fixes to your branch to
save a round trip; say so in the PR if you would rather they did not.

Be kind to each other. That is the whole code of conduct.
