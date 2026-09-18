# Archived: Linux getdents64 + statx walker

A Linux walker that lists directories with `getdents64` and stats each entry
with `statx` relative to the open directory. Every idea for making it beat
dua-core's `std::fs` walker is kept as a switch. It was tried in
#4–#6 and #8, measured in #18, and shelved because nothing won on local disks.

This directory is its own Cargo workspace, so the main workspace and CI never
build it. `ci/linux-walker-bench.yml` is the benchmark workflow, kept outside
`.github/workflows` so it doesn't run.

## Why it lost

`std::fs` already lists with `getdents64` and stats with `statx`, so the only
room left is in scheduling and I/O pattern. Measured on `ubuntu-latest`
(4 threads) in run 35385854301, cold cache, 3 rounds, compared with dua-core
(above 1× is faster):

| Variant | ext4 `/usr` (625k files) | XFS (151k) | NFS loopback (156k) |
|---|---:|---:|---:|
| dua-core (baseline) | 10.6 s | 3.3 s | 16.4 s |
| queue + inode order (#4) | 0.84× | 0.54× | 0.97× |
| queue | 0.88× | 0.56× | 1.64× |
| stealing | 1.00× | 0.82× | 1.35× |
| stealing + inode order | 0.95× | 0.94× | 1.27× |
| stealing + io_uring | 0.91× | 0.68× | 1.47× |
| stealing + dont_sync | 1.00× | 1.14× | 1.54× |
| stealing + mount id | 1.01× | 0.62× | 1.46× |
| stealing + xfs bulkstat | — | 1.11× | — |

- **Work stealing** fixes #4's slowdown but only reaches parity.
- **Inode order** didn't help on cloud disks.
- **io_uring** made warm scans about 30% slower: batching cost more than it
  saved.
- **Everything else is inside the noise.** On local disks, `dont_sync` and
  mount id should change nothing, yet they swung ±40% on XFS cold. On NFS,
  dua-core itself ranged from 10.9 s to 16.4 s, so the NFS gains are
  unconfirmed.
- **Warm cache:** every variant without io_uring was about 5% faster than
  dua-core.

## When to try again

- io_uring gets cheaper per operation, or gains batched directory listing
  (`IORING_OP_GETDENTS`) so listing and stat'ing can share one submission.
- A kernel call returns the listing and its attributes together, as
  `getattrlistbulk` does on macOS.
- NFS or other high-latency mounts become a target. Rerun with 10+ rounds
  first to see whether the NFS gains are real.

## Running it

Linux only. The walker is compiled only for Linux, and elsewhere `scan`
returns `Unsupported`.

```bash
cd archive/linux-walker && cargo test --release
```

```bash
cd archive/linux-walker && cargo build --release --example walkbench && sudo target/release/examples/walkbench --root /usr --reps 10 --cold
```

To run it in CI, copy `ci/linux-walker-bench.yml` to `.github/workflows/`
and start it from the Actions tab. The default is 10 cold rounds.

The crate depends on `dirstats-scan` by path and builds its trees with the
public `TreeBuilder`, so as long as that API holds it should still compile.
If it doesn't, fix the crate before trusting any numbers.
