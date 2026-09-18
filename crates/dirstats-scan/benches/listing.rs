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
//! These scanners run over each root, all with the same thread count:
//!
//! - `dua-core`: `dua_core::walk`, counting files and summing lengths.
//! - `std`: `std::fs::read_dir` on a rayon pool, doing the same.
//! - `raw` (Windows only): the same rayon walk, but listing each directory
//!   with `FileIdBothDirectoryInfo` directly into a per-thread buffer. If it
//!   matches `std`, dua-core's cost is in its walker, not in the Windows
//!   call.
//! - `scan`: `dirstats_scan::scan`, the whole scan including the tree.
//!
//! Each root is timed warm (a rescan) and, on Windows, cold. Before every
//! cold scan the root's volume is dismounted, which drops everything the
//! filesystem driver holds in memory for it (ReFS keeps its own metadata
//! cache that the next step alone does not touch), and then the standby list
//! is emptied, which drops cached file data, including that of a virtual
//! disk's backing file. The volume mounts again on the next access. Both need
//! administrator rights, and the dismount needs a volume nothing else has
//! open, so cold runs cannot use the system drive. A cache below Windows
//! (a virtual machine host's disk cache) is out of reach.
//!
//! ```text
//! set DIRSTATS_BENCH_ROOTS=ntfs-folder=N:\bench;fat32=F:\;exfat=G:\;refs=R:\
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

/// The `std` walk, listing with `FileIdBothDirectoryInfo` instead.
#[cfg(windows)]
fn raw_walk(root: &Path, pool: &rayon::ThreadPool) -> Totals {
    use std::cell::RefCell;
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdBothDirectoryInfo, GetFileInformationByHandleEx,
        OPEN_EXISTING,
    };

    thread_local! {
        // Same size as dua-core's, but allocated once per thread.
        static BUFFER: RefCell<Vec<u64>> = RefCell::new(vec![0; 8 * 1024]);
    }

    fn visit(dir: &Path, files: &AtomicU64, bytes: &AtomicU64) {
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: `wide` is null-terminated; the other arguments follow the
        // CreateFileW contract.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return;
        }
        let mut subdirs = Vec::new();
        let (mut local_files, mut local_bytes) = (0, 0);
        BUFFER.with_borrow_mut(|buffer| {
            loop {
                let len = (buffer.len() * 8) as u32;
                // SAFETY: the handle is open and the buffer is writable and
                // 8-byte aligned for `len` bytes.
                let ok = unsafe {
                    GetFileInformationByHandleEx(
                        handle,
                        FileIdBothDirectoryInfo,
                        buffer.as_mut_ptr().cast(),
                        len,
                    )
                };
                if ok == 0 {
                    break;
                }
                let base = buffer.as_ptr().cast::<u8>();
                let mut offset = 0;
                loop {
                    // SAFETY: the call filled the buffer with a chain of
                    // records, each starting at an aligned offset in it.
                    let info = unsafe { &*base.add(offset).cast::<FILE_ID_BOTH_DIR_INFO>() };
                    let name = unsafe {
                        std::slice::from_raw_parts(
                            info.FileName.as_ptr(),
                            info.FileNameLength as usize / 2,
                        )
                    };
                    if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                        if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
                            && name != [46]
                            && name != [46, 46]
                        {
                            subdirs.push(dir.join(OsString::from_wide(name)));
                        }
                    } else if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
                        local_files += 1;
                        local_bytes += info.EndOfFile as u64;
                    }
                    if info.NextEntryOffset == 0 {
                        break;
                    }
                    offset += info.NextEntryOffset as usize;
                }
            }
        });
        // SAFETY: the handle came from a successful CreateFileW.
        unsafe { CloseHandle(handle) };
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

type Scanner<'a> = Box<dyn Fn() -> Totals + 'a>;

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

        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut scanners: Vec<(&str, Scanner)> = vec![
            ("dua-core", Box::new(|| dua_core_walk(root, threads))),
            ("std", Box::new(|| std_walk(root, &pool))),
            ("scan", Box::new(|| dirstats_scan(root, &options))),
        ];
        #[cfg(windows)]
        scanners.insert(2, ("raw", Box::new(|| raw_walk(root, &pool))));
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
            for (name, scan) in &scanners {
                group.bench_function(BenchmarkId::new(*name, label), |b| {
                    b.iter_custom(|iters| {
                        let mut total = Duration::ZERO;
                        for _ in 0..iters {
                            if cold {
                                make_cold(root);
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

/// Dismount the volume holding `root`, then empty the standby list, so the
/// next scan reads from disk.
#[cfg(windows)]
fn make_cold(root: &Path) {
    use std::path::{Component, Prefix};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME};

    let Some(Component::Prefix(prefix)) = root.components().next() else {
        panic!(
            "cold runs need a root with a drive letter: {}",
            root.display()
        );
    };
    let letter = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter as char,
        _ => panic!(
            "cold runs need a root with a drive letter: {}",
            root.display()
        ),
    };
    let device: Vec<u16> = format!(r"\\.\{letter}:")
        .encode_utf16()
        .chain([0])
        .collect();
    // SAFETY: `device` is null-terminated; the handle is closed below, and
    // the control codes take no buffers.
    unsafe {
        let volume = CreateFileW(
            device.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        assert!(
            volume != INVALID_HANDLE_VALUE,
            "cannot open {letter}: (elevated?): {}",
            std::io::Error::last_os_error()
        );
        let mut returned = 0;
        for (code, what) in [
            (FSCTL_LOCK_VOLUME, "lock"),
            (FSCTL_DISMOUNT_VOLUME, "dismount"),
        ] {
            let ok = DeviceIoControl(
                volume,
                code,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            );
            assert!(
                ok != 0,
                "cannot {what} {letter}: (in use, or the system drive?): {}",
                std::io::Error::last_os_error()
            );
        }
        CloseHandle(volume);
    }
    purge_standby_list();
}

#[cfg(not(windows))]
fn make_cold(_: &Path) {}

/// Drop Windows' cached file data (the standby list).
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

criterion_group!(benches, listing);
criterion_main!(benches);
