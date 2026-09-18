// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors

//! Is dua-core's directory listing worth it where the NTFS master file table
//! reader does not apply? On Windows dua-core lists each directory with
//! `GetFileInformationByHandleEx(FileIdBothDirectoryInfo)`, which returns
//! names, sizes, times and file ids for many entries per call. The baseline
//! is `std::fs::read_dir`, which on Windows uses `FindFirstFileExW` /
//! `FindNextFileW`; its `DirEntry::metadata` comes from the find data, so it
//! costs no extra call per entry either.
//!
//! Three scanners run over each root, all with the same thread count:
//!
//! - `dua-core`: `dua_core::walk`, counting files and summing lengths.
//! - `std`: `std::fs::read_dir` on a rayon pool, doing the same.
//! - `scan`: `dirstats_scan::scan`, the whole scan including the tree.
//!
//! Each root is timed warm (a rescan) and, on Windows, cold (standby list
//! emptied before every scan, which needs administrator rights).
//!
//! ```text
//! set DIRSTATS_BENCH_ROOTS=ntfs-folder=D:\bench;fat32=F:\;exfat=G:\;refs=R:\
//! set DIRSTATS_BENCH_FIXTURE_FILES=50000
//! cargo bench -p dirstats-scan --bench listing
//! ```
//!
//! `DIRSTATS_BENCH_ROOTS` is a `;`-separated list of `label=path` (default:
//! one generated tree in a temporary directory). `DIRSTATS_BENCH_FIXTURE_FILES`
//! first adds a generated tree of that many files below each root.
//! `DIRSTATS_BENCH_COLD=0` skips the cold runs.

use criterion::{BenchmarkId, Criterion, SamplingMode, criterion_group, criterion_main};
use rayon::prelude::*;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Files and summed file lengths below a root; the walkers must agree on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Totals {
    files: u64,
    bytes: u64,
}

fn dua_core_walk(root: &Path, threads: usize) -> Totals {
    let walk = dua_core::walk(
        root,
        threads,
        dua_core::Order::ParentFirst,
        dua_core::Options::default(),
        |_| true,
    );
    let mut totals = Totals { files: 0, bytes: 0 };
    for entry in walk.flatten() {
        if entry.file_type.is_file() {
            totals.files += 1;
            if let Some(Ok(metadata)) = &entry.metadata {
                totals.bytes += metadata.len();
            }
        }
    }
    totals
}

fn std_walk(root: &Path, pool: &rayon::ThreadPool) -> Totals {
    fn visit(dir: &Path, files: &AtomicU64, bytes: &AtomicU64) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut subdirs = Vec::new();
        let (mut local_files, mut local_bytes) = (0, 0);
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                subdirs.push(entry.path());
            } else if file_type.is_file() {
                local_files += 1;
                if let Ok(metadata) = entry.metadata() {
                    local_bytes += metadata.len();
                }
            }
        }
        files.fetch_add(local_files, Ordering::Relaxed);
        bytes.fetch_add(local_bytes, Ordering::Relaxed);
        subdirs.par_iter().for_each(|sub| visit(sub, files, bytes));
    }
    let (files, bytes) = (AtomicU64::new(0), AtomicU64::new(0));
    pool.install(|| visit(root, &files, &bytes));
    Totals {
        files: files.into_inner(),
        bytes: bytes.into_inner(),
    }
}

fn dirstats_scan(root: &Path, options: &dirstats_scan::ScanOptions) -> Totals {
    let tree = dirstats_scan::scan(root, options).unwrap();
    let node = tree.node(tree.root());
    Totals {
        files: node.file_count,
        bytes: node.apparent_size,
    }
}

/// `files` files below `root/dirstats-listing-fixture`, 100 per directory,
/// three levels deep, unless a previous run already made them.
fn fixture(root: &Path, files: usize) {
    let dir = root.join("dirstats-listing-fixture");
    let stamp = dir.join(format!("complete-{files}"));
    if stamp.exists() {
        return;
    }
    let _ = fs::remove_dir_all(&dir);
    const PER_DIR: usize = 100;
    (0..files.div_ceil(PER_DIR)).into_par_iter().for_each(|i| {
        let sub = dir
            .join(format!("a{}", i % 10))
            .join(format!("b{}", i / 10 % 10))
            .join(format!("c{i}"));
        fs::create_dir_all(&sub).unwrap();
        for j in 0..PER_DIR.min(files - i * PER_DIR) {
            fs::write(
                sub.join(format!("f{j}")),
                vec![b'x'; [0, 100, 700, 5_000, 20_000][j % 5]],
            )
            .unwrap();
        }
    });
    fs::write(&stamp, b"").unwrap();
}

fn roots() -> (Vec<(String, PathBuf)>, Option<tempfile::TempDir>) {
    let files = std::env::var("DIRSTATS_BENCH_FIXTURE_FILES")
        .ok()
        .and_then(|n| n.parse().ok());
    match std::env::var("DIRSTATS_BENCH_ROOTS") {
        Ok(list) => {
            let roots: Vec<_> = list
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|item| {
                    let (label, path) = item
                        .split_once('=')
                        .expect("DIRSTATS_BENCH_ROOTS items are label=path");
                    (label.to_owned(), PathBuf::from(path))
                })
                .collect();
            if let Some(files) = files {
                for (_, path) in &roots {
                    fs::create_dir_all(path).unwrap();
                    fixture(path, files);
                }
            }
            (roots, None)
        }
        Err(_) => {
            let dir = tempfile::tempdir().unwrap();
            fixture(dir.path(), files.unwrap_or(20_000));
            (
                vec![("tempdir".to_owned(), dir.path().to_owned())],
                Some(dir),
            )
        }
    }
}

fn listing(c: &mut Criterion) {
    let (roots, _keep) = roots();
    let options = dirstats_scan::ScanOptions {
        size_metric: dirstats_scan::SizeMetric::Apparent,
        ..Default::default()
    };
    let threads = options.threads;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    let cold_runs = cfg!(windows) && std::env::var("DIRSTATS_BENCH_COLD").as_deref() != Ok("0");

    for (label, root) in &roots {
        // Timing walkers that disagree would compare different work.
        let expected = dua_core_walk(root, threads);
        assert_eq!(
            std_walk(root, &pool),
            expected,
            "std and dua-core disagree on {label}"
        );
        // The tree also counts directories' own lengths, so only files compare.
        assert_eq!(
            dirstats_scan(root, &options).files,
            expected.files,
            "scan and dua-core disagree on {label}"
        );
        eprintln!(
            "{label} ({}): {} files, {} bytes",
            root.display(),
            expected.files,
            expected.bytes
        );

        let scanners: [(&str, &dyn Fn() -> Totals); 3] = [
            ("dua-core", &|| dua_core_walk(root, threads)),
            ("std", &|| std_walk(root, &pool)),
            ("scan", &|| dirstats_scan(root, &options)),
        ];
        for cold in [false, true] {
            if cold && !cold_runs {
                continue;
            }
            let mut group =
                c.benchmark_group(format!("listing/{}", if cold { "cold" } else { "warm" }));
            if cold {
                // Every sample starts from an emptied cache, so warming up
                // only costs time.
                group
                    .sample_size(10)
                    .sampling_mode(SamplingMode::Flat)
                    .warm_up_time(Duration::from_secs(1));
            } else {
                group.sample_size(20);
            }
            for (name, scan) in scanners {
                group.bench_function(BenchmarkId::new(name, label), |b| {
                    b.iter_custom(|iters| {
                        let mut total = Duration::ZERO;
                        for _ in 0..iters {
                            if cold {
                                purge_standby_list();
                            }
                            let start = Instant::now();
                            black_box(scan());
                            total += start.elapsed();
                        }
                        total
                    });
                });
            }
            group.finish();
        }
    }
}

/// Drop Windows' cached file data and metadata (the standby list), so the
/// next scan reads from disk.
#[cfg(windows)]
fn purge_standby_list() {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSetSystemInformation(class: i32, info: *const std::ffi::c_void, len: u32) -> i32;
    }
    // SystemMemoryListInformation, and its flush-modified and purge-standby
    // commands.
    const MEMORY_LIST_INFORMATION: i32 = 80;
    const COMMANDS: [i32; 2] = [3, 4];

    // SAFETY: Win32 calls given valid pointers to correctly sized values.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        assert!(OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut token) != 0);
        let name: Vec<u16> = "SeProfileSingleProcessPrivilege\0".encode_utf16().collect();
        let mut luid = LUID {
            LowPart: 0,
            HighPart: 0,
        };
        assert!(LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) != 0);
        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let ok = AdjustTokenPrivileges(
            token,
            0,
            &privileges,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        CloseHandle(token);
        assert!(
            ok != 0,
            "cannot enable SeProfileSingleProcessPrivilege; run elevated or set DIRSTATS_BENCH_COLD=0"
        );
        for command in COMMANDS {
            let status =
                NtSetSystemInformation(MEMORY_LIST_INFORMATION, (&raw const command).cast(), 4);
            assert!(status >= 0, "emptying the standby list failed: {status:#x}");
        }
    }
}

#[cfg(not(windows))]
fn purge_standby_list() {}

criterion_group!(benches, listing);
criterion_main!(benches);
