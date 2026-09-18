// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// Port of `FinderNtfsContext::LoadRoot` in WinDirStat's `FinderNtfs.cpp`
// (GPL-2.0-or-later, by WinDirStat Team, https://windirstat.net).

//! Locating and reading the master file table of a mounted volume.

use crate::mft::Table;
use dirstats_scan::{Progress, ScanOptions, Tree};
use std::alloc::Layout;
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{FileExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, Prefix};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, GENERIC_READ};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_NO_BUFFERING, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, SYNCHRONIZE,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{FSCTL_GET_NTFS_VOLUME_DATA, FSCTL_GET_RETRIEVAL_POINTERS, NTFS_VOLUME_DATA_BUFFER};

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
/// Bytes per read. A multiple of every cluster and record size.
const CHUNK: u64 = 4 << 20;

/// `\\.\C:` for a root like `C:\`; `None` when `root` is not a drive's root.
pub fn device_path(root: &Path) -> Option<String> {
    let mut components = root.components();
    let Component::Prefix(prefix) = components.next()? else { return None };
    let (Prefix::Disk(letter) | Prefix::VerbatimDisk(letter)) = prefix.kind() else { return None };
    matches!((components.next(), components.next()), (Some(Component::RootDir) | None, None))
        .then(|| format!(r"\\.\{}:", char::from(letter)))
}

/// A stretch of the table: where it lies on the volume and which record it starts with.
struct Piece {
    volume_offset: u64,
    first_record: u64,
    length: usize,
}

pub fn scan(
    root: &Path,
    device: &str,
    options: &ScanOptions,
    cancel: &AtomicBool,
    progress: &Progress,
) -> io::Result<Tree> {
    let open_volume = || {
        OpenOptions::new()
            .access_mode(GENERIC_READ | SYNCHRONIZE)
            .share_mode(SHARE_ALL)
            .custom_flags(FILE_FLAG_NO_BUFFERING)
            .open(device)
    };
    let volume = open_volume()?;
    // SAFETY: plain data, fully written by the call below before use.
    let mut info: NTFS_VOLUME_DATA_BUFFER = unsafe { std::mem::zeroed() };
    control(&volume, FSCTL_GET_NTFS_VOLUME_DATA, &[], (&raw mut info).cast(), size_of::<NTFS_VOLUME_DATA_BUFFER>())?;
    let cluster = u64::from(info.BytesPerCluster);
    let record_size = info.BytesPerFileRecordSegment as usize;
    if cluster == 0 || record_size == 0 || !CHUNK.is_multiple_of(cluster) || !(CHUNK as usize).is_multiple_of(record_size) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected NTFS geometry"));
    }

    let pieces = pieces(&table_extents(device)?, cluster, record_size as u64);
    let next = AtomicUsize::new(0);
    let failure = std::sync::Mutex::new(None);
    let threads = options.threads.clamp(1, pieces.len().max(1));
    let mut table = Table::default();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut table = Table::default();
                    let result = (|| {
                        // A handle each, so that reads run side by side.
                        let volume = open_volume()?;
                        let mut buffer = AlignedBuffer::new(CHUNK as usize);
                        while let Some(piece) = pieces.get(next.fetch_add(1, Ordering::Relaxed)) {
                            if cancel.load(Ordering::Relaxed) || failure.lock().unwrap().is_some() {
                                break;
                            }
                            let bytes = &mut buffer.bytes()[..piece.length];
                            read_exact_at(&volume, bytes, piece.volume_offset)?;
                            let names = table.names.len();
                            table.parse_records(bytes, record_size, piece.first_record);
                            progress.entries.fetch_add((table.names.len() - names) as u64, Ordering::Relaxed);
                        }
                        Ok(())
                    })();
                    if let Err(err) = result {
                        failure.lock().unwrap().get_or_insert(err);
                    }
                    table
                })
            })
            .collect();
        for worker in workers {
            table.merge(worker.join().expect("reader thread panicked"));
        }
    });
    drop(volume);

    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"));
    }
    if let Some(err) = failure.into_inner().unwrap() {
        return Err(err);
    }
    table
        .into_tree(root.as_os_str().to_owned(), options)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no root directory in the master file table"))
}

/// The table's extents as (first virtual cluster, first volume cluster, clusters).
fn table_extents(device: &str) -> io::Result<Vec<(u64, u64, u64)>> {
    let table = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_NO_BUFFERING)
        .open(format!(r"{device}\$MFT::$DATA"))?;
    // RETRIEVAL_POINTERS_BUFFER as 64-bit words: extent count, first
    // virtual cluster, then a (next virtual cluster, volume cluster) pair
    // per extent.
    let mut words = vec![0u64; 2 + 2 * 32];
    let starting_vcn = 0u64.to_ne_bytes();
    loop {
        match control(&table, FSCTL_GET_RETRIEVAL_POINTERS, &starting_vcn, words.as_mut_ptr().cast(), size_of_val(&*words)) {
            Ok(()) => break,
            Err(err) if err.raw_os_error() == Some(ERROR_MORE_DATA as i32) => {
                let doubled = words.len() * 2;
                words.resize(doubled, 0);
            }
            Err(err) => return Err(err),
        }
    }
    let count = (words[0] & 0xFFFF_FFFF) as usize;
    let mut vcn = words[1];
    let mut extents = Vec::with_capacity(count);
    for &[next_vcn, lcn] in words[2..].as_chunks::<2>().0.iter().take(count) {
        // An all-ones cluster marks a hole, which the table never has.
        if lcn != u64::MAX && next_vcn > vcn {
            extents.push((vcn, lcn, next_vcn - vcn));
        }
        vcn = next_vcn;
    }
    Ok(extents)
}

/// Cut the extents into reads of at most [`CHUNK`] bytes.
fn pieces(extents: &[(u64, u64, u64)], cluster: u64, record_size: u64) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for &(vcn, lcn, clusters) in extents {
        let mut done = 0;
        let total = clusters * cluster;
        while done < total {
            let length = (total - done).min(CHUNK);
            pieces.push(Piece {
                volume_offset: lcn * cluster + done,
                first_record: (vcn * cluster + done) / record_size,
                length: length as usize,
            });
            done += length;
        }
    }
    pieces
}

fn control(file: &File, code: u32, input: &[u8], output: *mut c_void, output_size: usize) -> io::Result<()> {
    let mut returned = 0u32;
    // SAFETY: `input` and `output` are live for the call with the sizes
    // given, and the handle is open; no OVERLAPPED, so the call completes
    // before returning.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            code,
            input.as_ptr().cast(),
            input.len() as u32,
            output,
            output_size as u32,
            &raw mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

fn read_exact_at(file: &File, mut bytes: &mut [u8], mut offset: u64) -> io::Result<()> {
    while !bytes.is_empty() {
        match file.seek_read(bytes, offset)? {
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            read => {
                bytes = &mut bytes[read..];
                offset += read as u64;
            }
        }
    }
    Ok(())
}

/// Page-aligned memory, which unbuffered reads need whatever the sector size.
struct AlignedBuffer {
    pointer: *mut u8,
    layout: Layout,
}

impl AlignedBuffer {
    fn new(size: usize) -> Self {
        let layout = Layout::from_size_align(size, 4096).expect("valid layout");
        // SAFETY: the layout has a non-zero size.
        let pointer = unsafe { std::alloc::alloc_zeroed(layout) };
        if pointer.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Self { pointer, layout }
    }

    fn bytes(&mut self) -> &mut [u8] {
        // SAFETY: `pointer` owns `layout.size()` initialised bytes, borrowed
        // for as long as `self` is.
        unsafe { std::slice::from_raw_parts_mut(self.pointer, self.layout.size()) }
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        // SAFETY: allocated in `new` with this layout.
        unsafe { std::alloc::dealloc(self.pointer, self.layout) };
    }
}
