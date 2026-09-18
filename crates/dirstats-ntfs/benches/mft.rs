// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Reading the master file table against walking the same NTFS volume.
//!
//! ```text
//! # Elevated prompt on Windows; the root defaults to C:\.
//! set DIRSTATS_BENCH_ROOT=D:\
//! cargo bench -p dirstats-ntfs --bench mft
//! ```
//!
//! The table is read with `FILE_FLAG_NO_BUFFERING`, so every MFT scan goes to
//! disk, while the walker hits a warm cache after the first scan: these are
//! cold-table against warm-walk numbers. Elsewhere the bench does nothing.

use criterion::{Criterion, criterion_group, criterion_main};

#[cfg(windows)]
fn mft_vs_walk(c: &mut Criterion) {
    use dirstats_scan::ScanOptions;
    use std::hint::black_box;
    use std::path::PathBuf;

    let root = std::env::var_os("DIRSTATS_BENCH_ROOT").map_or_else(|| PathBuf::from(r"C:\"), PathBuf::from);
    let options = ScanOptions::default();
    // Fail up front rather than time a walk under the table's name.
    if let Err(err) = dirstats_ntfs::scan_mft(&root, &options) {
        panic!("cannot read the MFT of {} (NTFS drive root, elevated?): {err}", root.display());
    }

    let mut group = c.benchmark_group(format!("ntfs/{}", root.display()));
    group.sample_size(10);
    group.bench_function("mft", |b| b.iter(|| black_box(dirstats_ntfs::scan_mft(&root, &options).unwrap())));
    group.bench_function("walk", |b| b.iter(|| black_box(dirstats_scan::scan(&root, &options).unwrap())));
    group.finish();
}

#[cfg(not(windows))]
fn mft_vs_walk(_: &mut Criterion) {
    eprintln!("the MFT bench only runs on Windows");
}

criterion_group!(benches, mft_vs_walk);
criterion_main!(benches);
