---
name: add-feature-flag
description: Add an optional, feature-flagged capability to dirstats (an action like trash/open/icloud, an output like png, a front end, or a test-only harness like e2e). Use when new functionality brings a dependency or platform API that not every build should carry.
---

# Adding a feature flag

Convention (from dua-cli): one flag per front end, capability flags kept
separate, defaults chosen for the common case. The logic lives in the lowest
crate that can own it; upper crates only forward.

## 1. Own it in the right crate

- An **action or capability** (trash, open, iCloud eviction) is implemented
  in `dirstats-core`, behind a feature there whose entries are `dep:` items:

```toml
trash = ["dep:trash", "dep:objc2-foundation"]
```

- A **front end's use of it** (menu entry, key binding, dialog) is behind a
  same-named feature in `dirstats-gui` / `dirstats-tui` that only forwards:

```toml
trash = ["dirstats-core/trash"]
```

- The **binary** forwards to every crate that has the feature. Front ends are
  optional dependencies, so use `?` so the flag does not switch them on:

```toml
trash = ["dirstats-core/trash", "dirstats-tui?/trash", "dirstats-gui?/trash"]
```

  Forgetting the `?` makes `--features trash` silently build the TUI too.
  Leaving a front end out of the list compiles fine and the action is just
  missing from that front end.

- A **test-only** harness (like `e2e` in `dirstats-gui`) stays in its crate,
  is not forwarded from the binary, is never in `default`, and guards its
  module with `cfg(all(test, feature = "..."))`.

Put shared dependency versions in `[workspace.dependencies]` in the root
`Cargo.toml` and refer to them with `workspace = true, optional = true`.

## 2. In the code

- Gate with `#[cfg(feature = "x")]`; combine with platform where needed, as
  `#[cfg(all(windows, feature = "trash"))]` does for the delete dialogs.
- A capability that is off should disappear from menus and help text, not
  show up disabled.
- A feature that only matters on one OS must still compile, as a no-op, on
  the others (`icloud` does).

## 3. Default or not

Add it to the binary's `default` only if nearly every user wants it and it
costs little in size, build time and new dependencies. Anything heavy or
niche stays opt-in.

## 4. Check the combinations

```bash
cargo check -p dirstats --no-default-features
```

```bash
cargo check -p dirstats --no-default-features --features tui,NEWFLAG
```

```bash
cargo check -p dirstats --features NEWFLAG
```

```bash
cargo tree -p dirstats -e normal --no-default-features --features NEWFLAG | grep -E 'dirstats-(gui|tui)'
```

The last one should print nothing: the flag alone must not pull in a front
end. For a test-only flag, confirm the default build did not grow:

```bash
cargo tree -p dirstats -e normal | grep -c ' NEWDEP v'
```

CI builds default features only, so these combinations are yours to check.

## 5. Documents

Add the flag, with a one-line comment, to the `[features]` block in
`crates/dirstats/Cargo.toml`, and keep the feature listing in
`ARCHITECTURE.md` ("Build targets and feature flags") in step with it. The
end-user README mentions a flag only if users would set it themselves.
