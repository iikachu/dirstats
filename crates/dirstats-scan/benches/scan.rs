// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors

//! Fast path against the generic walker, on synthetic trees and optionally a
//! real directory.
//!
//! ```text
//! cargo bench -p dirstats-scan --bench scan
//! DIRSTATS_BENCH_ROOT=/usr cargo bench -p dirstats-scan --bench scan -- real
//! # Linux, as root: drop the page, dentry and inode caches before every scan.
//! DIRSTATS_BENCH_COLD=1 DIRSTATS_BENCH_ROOT=/usr cargo bench -p dirstats-scan --bench scan -- real
//! ```
//!
//! Only Linux with the `linux-fast` feature has a separate fast path; on other
//! platforms both walkers run the same code and the pair is a noise check.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dirstats_scan::{ScanOptions, scan};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A synthetic tree: `fanout` directories per level, `depth` levels, and
/// `files` small files in every directory.
struct Shape {
    name: &'static str,
    fanout: usize,
    depth: usize,
    files: usize,
}

const SHAPES: &[Shape] = &[
    Shape { name: "wide", fanout: 1, depth: 1, files: 20_000 },
    Shape { name: "deep", fanout: 1, depth: 200, files: 20 },
    Shape { name: "bushy", fanout: 6, depth: 4, files: 12 },
];

fn populate(dir: &Path, shape: &Shape, level: usize) {
    for i in 0..shape.files {
        fs::write(dir.join(format!("f{i}")), [0u8; 64]).unwrap();
    }
    if level < shape.depth {
        for i in 0..shape.fanout {
            let sub = dir.join(format!("d{i}"));
            fs::create_dir(&sub).unwrap();
            populate(&sub, shape, level + 1);
        }
    }
}

fn walkers() -> [(&'static str, ScanOptions); 2] {
    let fast = ScanOptions::default();
    let generic = ScanOptions { fast_path: false, ..fast.clone() };
    [("fast", fast), ("generic", generic)]
}

/// Drop Linux's page, dentry and inode caches; needs root.
fn drop_caches() {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: sync takes no arguments and cannot fail.
        unsafe { libc_sync() };
        fs::write("/proc/sys/vm/drop_caches", "3").expect("DIRSTATS_BENCH_COLD needs root on Linux");
    }
    #[cfg(not(target_os = "linux"))]
    panic!("DIRSTATS_BENCH_COLD is only supported on Linux");
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    #[link_name = "sync"]
    fn libc_sync();
}

fn bench_root(c: &mut Criterion, group_name: &str, root: &Path, cold: bool) {
    let mut group = c.benchmark_group(group_name);
    if cold {
        group.sample_size(10);
    }
    for (walker, options) in walkers() {
        group.bench_with_input(BenchmarkId::from_parameter(walker), &options, |b, options| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    if cold {
                        drop_caches();
                    }
                    let start = Instant::now();
                    std::hint::black_box(scan(root, options).unwrap());
                    total += start.elapsed();
                }
                total
            });
        });
    }
    group.finish();
}

fn synthetic(c: &mut Criterion) {
    for shape in SHAPES {
        let dir = tempfile::tempdir().unwrap();
        populate(dir.path(), shape, 0);
        bench_root(c, &format!("synthetic/{}", shape.name), dir.path(), false);
    }
}

fn real(c: &mut Criterion) {
    let Some(root) = std::env::var_os("DIRSTATS_BENCH_ROOT").map(PathBuf::from) else {
        return;
    };
    let cold = std::env::var_os("DIRSTATS_BENCH_COLD").is_some();
    let name = format!("real/{}", if cold { "cold" } else { "warm" });
    bench_root(c, &name, &root, cold);
}

criterion_group!(benches, synthetic, real);
criterion_main!(benches);
