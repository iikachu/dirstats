// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Reading the master file table against walking the same NTFS volume, each
//! cold (standby list emptied before every scan, like a first scan) and warm
//! (a rescan).
//!
//! ```text
//! # Elevated prompt on Windows.
//! set DIRSTATS_BENCH_ROOT=D:\
//! set DIRSTATS_BENCH_FIXTURE_FILES=100000
//! cargo bench -p dirstats-ntfs --bench mft
//! ```
//!
//! `DIRSTATS_BENCH_ROOT` defaults to `C:\`. `DIRSTATS_BENCH_FIXTURE_FILES`
//! first adds a generated tree of that many files to the volume, so a nearly
//! empty one still has something to scan. Elsewhere the bench does nothing.

#[cfg(windows)]
use criterion::SamplingMode;
use criterion::{Criterion, criterion_group, criterion_main};

#[cfg(windows)]
#[path = "../tests/support/mod.rs"]
mod support;

#[cfg(windows)]
fn mft_vs_walk(c: &mut Criterion) {
    use dirstats_scan::{ScanOptions, Tree};
    use std::hint::black_box;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    let root = std::env::var_os("DIRSTATS_BENCH_ROOT").map_or_else(|| PathBuf::from(r"C:\"), PathBuf::from);
    if let Some(files) = std::env::var("DIRSTATS_BENCH_FIXTURE_FILES").ok().and_then(|n| n.parse().ok()) {
        support::fixture(&root, files);
    }
    let options = ScanOptions::default();
    // Fail up front rather than time a walk under the table's name.
    if let Err(err) = dirstats_ntfs::scan_mft(&root, &options) {
        panic!("cannot read the MFT of {} (NTFS drive root, elevated?): {err}", root.display());
    }

    type Scan = fn(&Path, &ScanOptions) -> io::Result<Tree>;
    let scanners: [(&str, Scan); 2] = [("mft", |r, o| dirstats_ntfs::scan_mft(r, o)), ("walk", |r, o| dirstats_scan::scan(r, o))];

    for cold in [true, false] {
        let mut group = c.benchmark_group(format!("ntfs/{}/{}", root.display(), if cold { "cold" } else { "warm" }));
        if cold {
            // Every scan starts from an emptied cache, so warming up only
            // costs time. Flat sampling keeps each sample to a few scans
            // instead of ramping up to 10; 15 s fits 10 such samples.
            group
                .sample_size(10)
                .sampling_mode(SamplingMode::Flat)
                .warm_up_time(Duration::from_secs(1))
                .measurement_time(Duration::from_secs(15));
        } else {
            group.sample_size(20);
        }
        for (name, scan) in scanners {
            group.bench_function(name, |b| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        if cold {
                            support::purge_standby_list();
                        }
                        let start = Instant::now();
                        black_box(scan(&root, &options).unwrap());
                        total += start.elapsed();
                    }
                    total
                });
            });
        }
        group.finish();
    }
}

#[cfg(not(windows))]
fn mft_vs_walk(_: &mut Criterion) {
    eprintln!("the MFT bench only runs on Windows");
}

criterion_group!(benches, mft_vs_walk);
criterion_main!(benches);
