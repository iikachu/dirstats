# AGENTS.md

Guidance for coding agents working in this repository. Human contributors:
see [CONTRIBUTING.md](CONTRIBUTING.md); everything there applies to you too.

## What this is

dirstats is a disk usage scanner with treemaps, in a window (egui) or the
terminal (ratatui). It is a Rust workspace (edition 2024, Rust 1.88+) of
layered crates; [ARCHITECTURE.md](ARCHITECTURE.md) is the map and is worth
reading before a non-trivial change.

| Crate | Role | Licence |
|---|---|---|
| `dirstats-scan` | tree model and parallel scan | **Apache-2.0** |
| `dirstats-treemap` | layout and cushion rendering | GPL-3.0-or-later |
| `dirstats-ntfs` | NTFS master file table fast path | GPL-3.0-or-later |
| `dirstats-core` | the library: state, worker threads, file actions; re-exports scan and treemap | GPL-3.0-or-later |
| `dirstats-tui`, `dirstats-gui` | presentation and input only | GPL-3.0-or-later |
| `dirstats` | CLI and binary; picks a front end | GPL-3.0-or-later |

`bench/` holds benches that are their own Cargo workspaces, outside the one
above: the shelved Linux walker and the Windows listing bench. PR CI doesn't
build them. `.github/workflows/nightly.yml` runs them, and the NTFS MFT bench,
every night, and on PRs that change them.

## Commands

```bash
cargo build --workspace --all-targets
```

```bash
cargo test --workspace
```

```bash
cargo run -- PATH
```

```bash
cargo run -- --tui PATH
```

```bash
cargo check --workspace --all-targets --target x86_64-pc-windows-msvc
```

`rustfmt` is enforced, with the settings in `rustfmt.toml`: run
`cargo fmt --all` before committing. CI fails on anything
`cargo fmt --all --check` would change.

## You are free to

This project is permissive about how work gets done. Without asking, you may:

- add, change or refactor code in any crate, add tests, add dependencies that
  earn their place, and update docs to match;
- create branches and worktrees, and commit on them;
- make a reasonable call where the request is ambiguous, and say which call
  you made;
- leave a task partly done if you hit a real limit, as long as you say
  exactly what is and is not done.

## The few hard rules

1. **Licence boundary.** No GPL-derived code in `crates/dirstats-scan`, ever.
   Ported code goes in a GPL crate, with its origin in the file header and in
   `CREDITS.md`. Headers start with the SPDX line, then `// by dirstats
   contributors`. Write credits as "by <who>"; never "Copyright" or ©. Leave
   texts under `LICENSES/` verbatim.
2. **Layers.** Front ends never scan, lay out or touch the filesystem; they
   call `dirstats-core`, and depend on no other dirstats library crate. Scans run on a worker thread and never block a front
   end.
3. **Real files.** Trash, permanent delete, open and iCloud eviction act on
   the user's system. Exercise them only on paths inside a temp directory the
   test created. Never run a whole-disk scan test on a developer machine
   without pointing it at a smaller root.
4. **Privacy.** The repository is public. Nothing committed may contain a
   home directory path, username, machine name or personal email; `*.png` is
   git-ignored because screenshots show scanned paths. Check the staged diff
   before each commit.
5. **Git.** Never commit to `main` directly, force-push, rewrite published
   history, merge a PR or enable auto-merge unless asked. The working tree
   may be shared with other sessions: prefer your own `git worktree`, stage
   by path, leave files you did not touch alone, and never use bare
   `git stash`.
6. **Honest reports.** Say what you ran and where. "Type-checks for Windows"
   is not "works on Windows". If a test fails or a step was skipped, say so.

## Conventions

- Commit subjects: `area: a sentence saying what is now true`, lower case, no
  full stop, e.g. `gui, tui: the scan status says "skipped", not "errors"`.
  Areas are crate names without the `dirstats-` prefix, or `ci`, `readme`,
  `credits`, `skills`.
- Comments explain why, at about the density of the surrounding code. Doc
  comments on public items.
- Optional capabilities are feature flags owned by `dirstats-core` and
  forwarded by the binary with `?` syntax. Test-only harnesses stay behind
  their own non-default feature.
- OS-specific code is `cfg`-gated so every crate builds on every platform and
  falls back to the portable path.

## Skills

Step-by-step routines live in `.claude/skills/`. Read the matching one before
starting:

| Task | Skill |
|---|---|
| Porting or adapting someone else's code | `port-upstream-code` |
| Committing, branching, opening a PR | `commit-style` |
| A faster scan for one OS or filesystem | `add-fs-fast-path` |
| A new optional capability | `add-feature-flag` |
| Touching `cfg(windows)` / `cfg(linux)` code or `Cargo.lock` | `windows-check-from-mac` |
| Testing GUI behaviour headlessly | `add-gui-e2e-test` |
| Screenshots on a PR that changes the GUI | `gui-pr-screenshots` |

They are plain Markdown; any agent can follow them.
