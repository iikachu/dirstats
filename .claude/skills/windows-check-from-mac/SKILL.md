---
name: windows-check-from-mac
description: Type-check Windows-only (or Linux-only) dirstats code from a Mac or any other host without that OS, and know what that does and does not prove. Use after touching cfg(windows) or cfg(target_os = "linux") code, a target-specific dependency, or Cargo.lock.
---

# Checking another platform's code from here

Much of dirstats is per-platform: the NTFS reader, permanent delete and its
dialogs, the Recycle Bin, volume handling. A host build never compiles the
other platforms' branches, so a change can look finished and fail in CI
minutes later. A cross **check** closes most of that gap in seconds.

## Commands

Once per machine:

```bash
rustup target add x86_64-pc-windows-msvc x86_64-unknown-linux-gnu
```

Then:

```bash
cargo check --workspace --all-targets --locked --target x86_64-pc-windows-msvc
```

```bash
cargo check --workspace --all-targets --locked --target x86_64-unknown-linux-gnu
```

```bash
cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc
```

When the GUI end-to-end harness or `Cargo.lock` changed, also:

```bash
cargo check -p dirstats-gui --features e2e,egui-fonts --tests --locked --target x86_64-pc-windows-msvc
```

`check` needs no linker or Windows SDK. `build` and `test` for the Windows
target do, and will fail at link time on a Mac; that failure says nothing
about the code. A crate with a C build script may also refuse to cross-check
for lack of a cross C compiler; note it and rely on CI for that crate.

## What it proves

| Proves | Does not prove |
|---|---|
| `cfg(windows)` code type-checks | it links |
| target-specific dependencies resolve and compile under `--locked` | it runs, or the Win32 calls succeed |
| feature and `cfg` combinations are consistent | behaviour that needs administrator rights or a real NTFS volume |

Behaviour is proven by the CI run on `windows-latest`, including the
`disk scan` job that scans `C:\` through the real entry point. Say in the
hand-over which of the two you have: "type-checks for Windows" is not
"works on Windows".

## The `windows` crate version trap

Several dependencies use Microsoft's `windows` crate and exchange its types,
so they must agree on one version. Cargo does not know that. This applies
once the GUI end-to-end harness (the `e2e` feature, which brings in wgpu) is
in the tree; if `Cargo.lock` has no `gpu-allocator`, skip this section. In
`Cargo.lock`, `gpu-allocator` accepts `windows` 0.53 to 0.58 and must be
locked to **0.58.0**, the version `wgpu-hal` uses. `trash` needs 0.56, so
that version stays in the lockfile too and Cargo will happily reuse it for
`gpu-allocator`, after which `wgpu-hal` fails with dozens of mismatched
`ID3D12...` type errors. It only ever shows on Windows.

After any `cargo update` or dependency change:

```bash
awk '/^name = "gpu-allocator"/,/^$/' Cargo.lock | grep windows
```

If it shows `windows 0.56.0`, edit that one line in `Cargo.lock` to
`"windows 0.58.0"` and re-run the e2e check above with `--locked`.

## Reading a failed Windows job

```bash
gh run view RUN_ID --json jobs -q '.jobs[] | "\(.databaseId) \(.name): \(.conclusion)"'
```

```bash
gh api --allow-escape-sequences repos/OWNER/REPO/actions/jobs/JOB_ID/logs | sed 's/\x1b\[[0-9;]*m//g' | grep -nE 'panicked|^.{29}error|test result' -A6
```

`gh run view --log-failed` only works once the whole run has finished; the
API call works as soon as the job has.
