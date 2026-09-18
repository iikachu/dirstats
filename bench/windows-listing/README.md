# Windows directory listing bench (nightly)

Everything on Windows that isn't a whole NTFS drive scanned elevated (a
folder, FAT32, exFAT, ReFS, network shares, NTFS without admin rights) is
walked by dua-core, which lists each directory with
`GetFileInformationByHandleEx(FileIdBothDirectoryInfo)`. This bench, from
#16, checks whether that is actually faster than the alternatives.

This directory is its own Cargo workspace, so the main workspace and PR CI
never build it. The `bench windows listing` job in
`.github/workflows/nightly.yml` runs it every night on NTFS, FAT32, exFAT and ReFS virtual disks and uploads
the criterion report. The walkers must agree before anything is timed, so a
walker that finds something different fails the run.

## What it compares

All on the same tree, with the same thread count:

- `dua-core`: `dua_core::walk`, counting files and summing lengths.
- `std`: `std::fs::read_dir` (`FindFirstFileExW`/`FindNextFileW`) on a rayon
  pool.
- `raw` (Windows only): the same rayon walk, listing with
  `FileIdBothDirectoryInfo` directly into one buffer per thread.
- `scan`: the full `dirstats_scan::scan`, tree included.

The walkers must agree on file count and total bytes before anything is
timed. Cold samples dismount the volume (clearing the filesystem's own
caches, which ReFS needs) and empty the standby list first; this needs an
elevated process and a volume other than the system drive.

## Results

`windows-latest`, 50k files per root, each on its own virtual disk, median:

| Root | warm dua-core | warm std | warm raw | cold dua-core | cold std | cold raw |
|---|---:|---:|---:|---:|---:|---:|
| NTFS folder | 33.7 ms | 13.5 ms | **9.9 ms** | 633 ms | 698 ms | 688 ms |
| FAT32 | 48.7 ms | **30.9 ms** | 31.9 ms | 248 ms | 244 ms | 246 ms |
| exFAT | 52.5 ms | **31.1 ms** | 31.3 ms | 548 ms | 540 ms | 551 ms |
| ReFS | 42.3 ms | 26.3 ms | **21.1 ms** | 1.54 s | 1.53 s | 1.56 s |

- **Cold, all walkers tie.** Disk reads dominate.
- **Warm, the Windows call is fine, dua-core's walker isn't.** `raw` matches
  or beats `std`; dua-core, making the same call, is 1.6–3.4× slower. From
  its source (4.1.0), likely causes: per directory, a freshly zeroed 64 KB
  buffer, a `\\?\` path rebuild and an extra `GetFileInformationByHandle`;
  per entry, results sent through a channel in chunks of 4.
- An earlier run that only emptied the standby list showed ReFS "cold" equal
  to warm (44 ms): that is not cold. Dismounting fixed it.
- The Azure host may cache the VM's disk below Windows; only real hardware
  rules that out.

## When to pick it up

- To make rescans faster on Windows: either a dirstats walker built around
  the `raw` listing, or fixing the costs above in dua-core upstream. Rerun
  this bench to confirm.
- After a dua-core upgrade, to see whether the gap closed. Bump `dua-core`
  in this crate's `Cargo.toml` too; it has its own lock file.

## Running it

```bash
cd bench/windows-listing && cargo bench --bench listing
```

On Windows, elevated, set `DIRSTATS_BENCH_ROOTS` (`label=path;...`) and
`DIRSTATS_BENCH_FIXTURE_FILES`; see the top of `benches/listing.rs`. Set
`DIRSTATS_BENCH_COLD=0` to skip cold runs. To run it in CI outside the
schedule, start the `Nightly` workflow from the Actions tab.
