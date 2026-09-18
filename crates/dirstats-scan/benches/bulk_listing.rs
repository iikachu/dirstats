// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors

//! Whether macOS bulk listing (`getattrlistbulk`, which dua-core uses) earns
//! its keep against the plain approach: `readdir` plus one `lstat` per entry.
//!
//! ```text
//! cargo bench -p dirstats-scan --bench bulk_listing
//! DIRSTATS_BENCH_ROOT=~/code cargo bench -p dirstats-scan --bench bulk_listing
//! sudo -E DIRSTATS_BENCH_COLD=1 cargo bench -p dirstats-scan --bench bulk_listing
//! ```
//!
//! Two groups, both collecting the same metadata:
//!
//! - `flat`: one directory of `DIRSTATS_BENCH_FLAT_FILES` files (default
//!   20 000) on one thread, which isolates the syscall pattern.
//! - `tree`: a parallel walk of `DIRSTATS_BENCH_ROOT`, or a generated tree of
//!   `DIRSTATS_BENCH_TREE_FILES` files (default 100 000). `scan` is the full
//!   dirstats scan for scale.
//!
//! `DIRSTATS_BENCH_COLD=1` runs `purge` before every sample to empty the
//! file cache, which needs root. dua-core has no switch to turn bulk listing
//! off, so the baseline is written here. Elsewhere the bench does nothing.

#[cfg(not(target_os = "macos"))]
use criterion::Criterion;
use criterion::{criterion_group, criterion_main};

#[cfg(target_os = "macos")]
mod macos {
    use criterion::{Criterion, SamplingMode};
    use std::fs;
    use std::hint::black_box;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    fn env_usize(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    /// `files` files spread over directories of at most 100, nested 3 deep.
    fn fixture(root: &Path, files: usize) {
        for i in 0..files {
            let dir = root.join(format!(
                "{}/{}/{}",
                i / 100_000,
                (i / 1000) % 100,
                (i / 100) % 10
            ));
            if i % 100 == 0 {
                fs::create_dir_all(&dir).unwrap();
            }
            fs::write(dir.join(format!("f{i}")), [0u8; 16]).unwrap();
        }
    }

    /// Entries seen and bytes summed, so neither side can skip the metadata.
    #[derive(Default)]
    struct Tally {
        entries: AtomicU64,
        bytes: AtomicU64,
    }

    impl Tally {
        fn add(&self, len: u64) {
            self.entries.fetch_add(1, Ordering::Relaxed);
            self.bytes.fetch_add(len, Ordering::Relaxed);
        }
        fn get(&self) -> (u64, u64) {
            (
                self.entries.load(Ordering::Relaxed),
                self.bytes.load(Ordering::Relaxed),
            )
        }
    }

    fn bulk_dir(dir: &Path) -> (u64, u64) {
        let tally = Tally::default();
        for entry in dua_core::read_dir(dir, dua_core::Options::default()).unwrap() {
            let entry = entry.unwrap();
            tally.add(entry.metadata.unwrap().map_or(0, |m| m.len()));
        }
        tally.get()
    }

    fn plain_dir(dir: &Path) -> (u64, u64) {
        let tally = Tally::default();
        for entry in fs::read_dir(dir).unwrap() {
            tally.add(fs::symlink_metadata(entry.unwrap().path()).map_or(0, |m| m.len()));
        }
        tally.get()
    }

    fn bulk_tree(root: &Path, threads: usize) -> (u64, u64) {
        let tally = Tally::default();
        let walk = dua_core::walk(
            root,
            threads,
            dua_core::Order::Completion,
            dua_core::Options::default(),
            |_| true,
        );
        for entry in walk {
            tally.add(
                entry
                    .ok()
                    .and_then(|e| e.metadata)
                    .and_then(Result::ok)
                    .map_or(0, |m| m.len()),
            );
        }
        tally.get()
    }

    /// `readdir` then `lstat` per entry, a directory per rayon task.
    fn plain_tree(pool: &rayon::ThreadPool, root: &Path) -> (u64, u64) {
        fn visit<'s>(scope: &rayon::Scope<'s>, dir: PathBuf, tally: &'s Tally) {
            let Ok(entries) = fs::read_dir(&dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(metadata) = fs::symlink_metadata(&path) else {
                    continue;
                };
                tally.add(metadata.len());
                if metadata.is_dir() {
                    scope.spawn(move |s| visit(s, path, tally));
                }
            }
        }
        let tally = Tally::default();
        tally.add(0);
        pool.scope(|s| visit(s, root.to_owned(), &tally));
        tally.get()
    }

    fn purge() {
        let status = std::process::Command::new("purge")
            .status()
            .expect("run purge");
        assert!(
            status.success(),
            "purge failed (run the bench as root for cold samples)"
        );
    }

    fn time(cold: bool, iters: u64, mut f: impl FnMut()) -> Duration {
        let mut total = Duration::ZERO;
        for _ in 0..iters {
            if cold {
                purge();
            }
            let start = Instant::now();
            f();
            total += start.elapsed();
        }
        total
    }

    pub fn bench(c: &mut Criterion) {
        let cold = std::env::var_os("DIRSTATS_BENCH_COLD").is_some();
        let threads = dirstats_scan::ScanOptions::default().threads;
        let tmp = tempfile::tempdir().unwrap();

        let flat = tmp.path().join("flat");
        fs::create_dir(&flat).unwrap();
        for i in 0..env_usize("DIRSTATS_BENCH_FLAT_FILES", 20_000) {
            fs::write(flat.join(format!("f{i}")), [0u8; 16]).unwrap();
        }
        let tree = std::env::var_os("DIRSTATS_BENCH_ROOT").map_or_else(
            || {
                let tree = tmp.path().join("tree");
                fixture(&tree, env_usize("DIRSTATS_BENCH_TREE_FILES", 100_000));
                tree
            },
            PathBuf::from,
        );

        // Both sides must see the same entries, or the timings compare nothing.
        assert_eq!(bulk_dir(&flat), plain_dir(&flat));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let (bulk_n, plain_n) = (bulk_tree(&tree, threads).0, plain_tree(&pool, &tree).0);
        assert_eq!(
            bulk_n,
            plain_n,
            "entry counts differ under {}",
            tree.display()
        );
        eprintln!(
            "tree: {} entries under {}, {threads} threads",
            bulk_n,
            tree.display()
        );

        let cache = if cold { "cold" } else { "warm" };
        let configure = |group: &mut criterion::BenchmarkGroup<'_, _>| {
            if cold {
                group
                    .sample_size(10)
                    .sampling_mode(SamplingMode::Flat)
                    .warm_up_time(Duration::from_secs(1));
            }
        };

        let mut group = c.benchmark_group(format!("bulk_listing/flat/{cache}"));
        configure(&mut group);
        group.bench_function("getattrlistbulk", |b| {
            b.iter_custom(|n| time(cold, n, || _ = black_box(bulk_dir(&flat))))
        });
        group.bench_function("readdir+lstat", |b| {
            b.iter_custom(|n| time(cold, n, || _ = black_box(plain_dir(&flat))))
        });
        group.finish();

        let mut group = c.benchmark_group(format!("bulk_listing/tree/{cache}"));
        configure(&mut group);
        group.sample_size(10);
        group.bench_function("getattrlistbulk", |b| {
            b.iter_custom(|n| time(cold, n, || _ = black_box(bulk_tree(&tree, threads))))
        });
        group.bench_function("readdir+lstat", |b| {
            b.iter_custom(|n| time(cold, n, || _ = black_box(plain_tree(&pool, &tree))))
        });
        let options = dirstats_scan::ScanOptions::default();
        group.bench_function("scan", |b| {
            b.iter_custom(|n| {
                time(cold, n, || {
                    _ = black_box(dirstats_scan::scan(&tree, &options).unwrap())
                })
            })
        });
        group.finish();
    }
}

#[cfg(target_os = "macos")]
use macos::bench;

#[cfg(not(target_os = "macos"))]
fn bench(_: &mut Criterion) {
    eprintln!("the bulk listing bench only runs on macOS");
}

criterion_group!(benches, bench);
criterion_main!(benches);
