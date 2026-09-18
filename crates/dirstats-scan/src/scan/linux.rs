// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors

//! Linux walker: directories are listed with `getdents64` and their entries
//! stat'ed with `statx` relative to the open directory, in inode order.
//!
//! dua-core walks Linux through `std::fs`, which lists a directory in the
//! order the filesystem returns names and asks `statx` for every basic
//! attribute plus the birth time. ext4 returns names in hash order, so
//! stat'ing in that order jumps about the inode table; sorting each
//! listing by inode number first turns that into a mostly forward sweep.
//! Only the attributes a scan uses are requested, which spares filesystems
//! where some (birth time on network mounts, for instance) cost extra.
//!
//! Work is shared between threads a directory at a time; the entries of a
//! wide directory are split into chunks so that other threads can stat
//! them too. Every directory is sent before anything inside it, because
//! its contents are only queued once the batch naming it has been sent.

use super::{ScanOptions, Stat, Walked, platform};
use crate::tree::Kind;
use std::ffi::{CStr, CString, OsStr};
use std::fs::File;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

/// Entries stat'ed by one job; larger directories are shared out in chunks.
const CHUNK: usize = 256;
/// Bytes of directory entries fetched per `getdents64` call.
const LISTING_BUFFER: usize = 64 * 1024;
/// The attributes a scan uses.
const MASK: u32 = libc::STATX_TYPE
    | libc::STATX_MODE
    | libc::STATX_NLINK
    | libc::STATX_INO
    | libc::STATX_SIZE
    | libc::STATX_BLOCKS
    | libc::STATX_MTIME
    | libc::STATX_MNT_ID;
/// Never follow the last component, and never trigger an automount: an
/// unmounted autofs point is reported as it is, as `stat` would.
const FLAGS: libc::c_int = libc::AT_SYMLINK_NOFOLLOW | libc::AT_NO_AUTOMOUNT;

type Batch = Vec<io::Result<Walked>>;

/// A running walk. Yields the root, then every entry below it with each
/// directory before its contents; dropping it stops the workers.
pub(super) struct Walk {
    events: Receiver<Batch>,
    batch: std::vec::IntoIter<io::Result<Walked>>,
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
    stop: AtomicBool,
    next_directory: AtomicUsize,
    /// Where the root is mounted, when the walk must not leave it.
    boundary: Option<Mount>,
}

impl Shared {
    /// Make every worker return. The flag is set under the queue lock so
    /// that a worker about to wait cannot miss the wake-up.
    fn halt(&self) {
        let queue = self.queue.lock().unwrap();
        self.stop.store(true, Ordering::Relaxed);
        drop(queue);
        self.ready.notify_all();
    }
}

struct Queue {
    jobs: Vec<Job>,
    /// Jobs queued or running; the walk is over when this reaches zero.
    outstanding: usize,
}

enum Job {
    /// List a directory and stat its entries.
    Read { path: Arc<Path>, directory: usize },
    /// Stat a chunk of a wide directory's entries.
    Stat { dir: Arc<File>, path: Arc<Path>, directory: usize, names: Vec<Name> },
}

/// A directory entry as listed: inode, `d_type` and name.
struct Name {
    inode: u64,
    kind: u8,
    name: CString,
}

impl Walk {
    /// Start walking `root`, or `None` when this kernel cannot (no `statx`
    /// before Linux 4.11, or a sandbox that refuses it) or the root cannot
    /// be stat'ed. The caller then walks the portable way, which reports
    /// any error about the root itself.
    pub(super) fn start(root: &Path, options: &ScanOptions) -> Option<Self> {
        let c_root = CString::new(root.as_os_str().as_bytes()).ok()?;
        let stat = statx(libc::AT_FDCWD, &c_root).ok()?;
        let kind = kind_of(&stat);
        let directory = (kind == Kind::Directory).then_some(0);
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue { jobs: Vec::new(), outstanding: 0 }),
            ready: Condvar::new(),
            stop: AtomicBool::new(false),
            next_directory: AtomicUsize::new(1),
            boundary: options.same_filesystem.then(|| Mount::of(&stat)),
        });
        let root_entry = Walked {
            parent: None,
            directory,
            name: root.as_os_str().into(),
            kind,
            metadata: Some(Ok(stat_of(&stat))),
        };

        let threads = options.threads.max(1);
        let (sender, events) = sync_channel(threads * 4);
        let mut workers = Vec::new();
        if directory.is_some() {
            {
                let mut queue = shared.queue.lock().unwrap();
                queue.jobs.push(Job::Read { path: Arc::from(root), directory: 0 });
                queue.outstanding = 1;
            }
            for index in 0..threads {
                let (shared, sender) = (Arc::clone(&shared), sender.clone());
                let spawned = std::thread::Builder::new()
                    .name(format!("dirstats-walk-{index}"))
                    .spawn(move || work(&shared, &sender));
                match spawned {
                    Ok(handle) => workers.push(handle),
                    // Fewer threads still finish the walk; none cannot.
                    Err(_) if !workers.is_empty() => break,
                    Err(_) => return None,
                }
            }
        }
        // Only the workers send, so the channel closes once they are done.
        drop(sender);
        Some(Self { events, batch: vec![Ok(root_entry)].into_iter(), shared, workers })
    }

    /// The next entry, or `None` once the walk is complete or `cancel` is set.
    pub(super) fn next(&mut self, cancel: &AtomicBool) -> Option<io::Result<Walked>> {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(entry) = self.batch.next() {
                return Some(entry);
            }
            // Wake now and then so a cancel is noticed while a slow
            // directory (on a network mount, say) is still being read.
            match self.events.recv_timeout(Duration::from_millis(100)) {
                Ok(batch) => self.batch = batch.into_iter(),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
    }
}

impl Drop for Walk {
    fn drop(&mut self) {
        self.shared.halt();
        // Workers blocked on a full channel are released by closing it.
        let (_, closed) = sync_channel(0);
        drop(std::mem::replace(&mut self.events, closed));
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn work(shared: &Shared, events: &SyncSender<Batch>) {
    loop {
        let job = {
            let mut queue = shared.queue.lock().unwrap();
            loop {
                if shared.stop.load(Ordering::Relaxed) || queue.outstanding == 0 {
                    return;
                }
                if let Some(job) = queue.jobs.pop() {
                    break job;
                }
                queue = shared.ready.wait(queue).unwrap();
            }
        };
        let (batch, children) = match job {
            Job::Read { path, directory } => read(shared, path, directory),
            Job::Stat { dir, path, directory, names } => stat_all(shared, &dir, &path, directory, names),
        };
        // Sent before the directories in it are queued, so every directory
        // reaches the consumer ahead of its contents.
        if !batch.is_empty() && events.send(batch).is_err() {
            shared.halt();
            return;
        }
        let mut queue = shared.queue.lock().unwrap();
        queue.outstanding = queue.outstanding + children.len() - 1;
        queue.jobs.extend(children);
        drop(queue);
        // Wakes idle workers for the new jobs, or all of them to exit.
        shared.ready.notify_all();
    }
}

/// List the directory at `path` and stat its entries, sharing out all but
/// the first chunk of a wide one.
fn read(shared: &Shared, path: Arc<Path>, directory: usize) -> (Batch, Vec<Job>) {
    let dir = match File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(&path)
    {
        Ok(dir) => dir,
        Err(err) => return (vec![Err(err)], Vec::new()),
    };
    let mut names = Vec::new();
    let mut failure = None;
    let mut buffer = vec![0u8; LISTING_BUFFER];
    loop {
        // SAFETY: the buffer is valid for writes of its whole length.
        let read = unsafe {
            libc::syscall(libc::SYS_getdents64, dir.as_raw_fd(), buffer.as_mut_ptr(), buffer.len())
        };
        if read < 0 {
            // Keep what was listed; the rest of the directory is lost.
            failure = Some(io::Error::last_os_error());
            break;
        }
        if read == 0 {
            break;
        }
        for (inode, kind, name) in dirents(&buffer[..read as usize]) {
            let name = CString::new(name).expect("no NUL in a file name");
            names.push(Name { inode, kind, name });
        }
    }
    // Inode order is roughly on-disk order for ext4, XFS and btrfs alike.
    names.sort_unstable_by_key(|name| name.inode);

    if names.len() > CHUNK {
        let dir = Arc::new(dir);
        let mut chunks = Vec::new();
        let mut rest = names.split_off(CHUNK);
        while !rest.is_empty() {
            let tail = rest.split_off(rest.len().min(CHUNK));
            let chunk = std::mem::replace(&mut rest, tail);
            chunks.push(Job::Stat { dir: Arc::clone(&dir), path: Arc::clone(&path), directory, names: chunk });
        }
        // Idle workers take the other chunks while this one is stat'ed.
        {
            let mut queue = shared.queue.lock().unwrap();
            queue.outstanding += chunks.len();
            queue.jobs.extend(chunks);
        }
        shared.ready.notify_all();
        let (mut batch, children) = stat_all(shared, &dir, &path, directory, names);
        batch.extend(failure.map(Err));
        return (batch, children);
    }
    let (mut batch, children) = stat_all(shared, &dir, &path, directory, names);
    batch.extend(failure.map(Err));
    (batch, children)
}

/// Stat `names` inside `dir` and queue the directories among them.
fn stat_all(shared: &Shared, dir: &File, path: &Arc<Path>, directory: usize, names: Vec<Name>) -> (Batch, Vec<Job>) {
    let mut batch = Vec::with_capacity(names.len());
    let mut children = Vec::new();
    for Name { inode: _, kind, name } in names {
        let file_name = OsStr::from_bytes(name.to_bytes());
        let (kind, metadata, descend) = match statx(dir.as_raw_fd(), &name) {
            Ok(stat) => {
                let kind = kind_of(&stat);
                let inside = shared.boundary.is_none_or(|boundary| boundary.holds(Mount::of(&stat)));
                (kind, Ok(stat_of(&stat)), kind == Kind::Directory && inside)
            }
            // Gone since the listing, or not ours to see: the listing's
            // type still places it.
            Err(err) => (kind_of_dirent(kind), Err(err), false),
        };
        let index = (kind == Kind::Directory).then(|| shared.next_directory.fetch_add(1, Ordering::Relaxed));
        if descend && let Some(index) = index {
            children.push(Job::Read { path: Arc::from(path.join(file_name)), directory: index });
        }
        batch.push(Ok(Walked {
            parent: Some(directory),
            directory: index,
            name: file_name.into(),
            kind,
            metadata: Some(metadata),
        }));
    }
    (batch, children)
}

/// The entries of a `getdents64` buffer as inode, `d_type` and name,
/// without "." and "..".
fn dirents(buffer: &[u8]) -> impl Iterator<Item = (u64, u8, &[u8])> {
    // struct linux_dirent64 { u64 d_ino; s64 d_off; u16 d_reclen; u8 d_type; char d_name[]; }
    const NAME: usize = 19;
    let mut offset = 0;
    std::iter::from_fn(move || {
        loop {
            let header = buffer.get(offset..offset + NAME)?;
            let inode = u64::from_ne_bytes(header[0..8].try_into().unwrap());
            let length = usize::from(u16::from_ne_bytes(header[16..18].try_into().unwrap()));
            let kind = header[18];
            let record = buffer.get(offset..offset + length).filter(|_| length > NAME)?;
            offset += length;
            let name = &record[NAME..];
            let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
            if name != b"." && name != b".." {
                return Some((inode, kind, name));
            }
        }
    })
}

/// `struct statx` from the kernel's UAPI headers, declared here because
/// the libc crate only offers it for glibc and recent musl.
#[repr(C)]
struct Statx {
    mask: u32,
    blksize: u32,
    attributes: u64,
    nlink: u32,
    uid: u32,
    gid: u32,
    mode: u16,
    _spare0: u16,
    ino: u64,
    size: u64,
    blocks: u64,
    attributes_mask: u64,
    atime: StatxTimestamp,
    btime: StatxTimestamp,
    ctime: StatxTimestamp,
    mtime: StatxTimestamp,
    rdev_major: u32,
    rdev_minor: u32,
    dev_major: u32,
    dev_minor: u32,
    mnt_id: u64,
    _spare: [u64; 13],
}

#[repr(C)]
struct StatxTimestamp {
    sec: i64,
    nsec: u32,
    _reserved: i32,
}

const _: () = assert!(size_of::<Statx>() == 256);

fn statx(dir: RawFd, name: &CStr) -> io::Result<Statx> {
    let mut buffer = MaybeUninit::<Statx>::zeroed();
    // SAFETY: `name` is NUL-terminated and the buffer is a whole `struct statx`.
    let result = unsafe { libc::syscall(libc::SYS_statx, dir, name.as_ptr(), FLAGS, MASK, buffer.as_mut_ptr()) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: zeroed and then filled in by the kernel; every bit pattern is valid.
    Ok(unsafe { buffer.assume_init() })
}

fn kind_of(stat: &Statx) -> Kind {
    match u32::from(stat.mode) & libc::S_IFMT {
        libc::S_IFDIR => Kind::Directory,
        libc::S_IFREG => Kind::File,
        libc::S_IFLNK => Kind::Symlink,
        _ => Kind::Other,
    }
}

fn kind_of_dirent(kind: u8) -> Kind {
    match kind {
        libc::DT_DIR => Kind::Directory,
        libc::DT_REG => Kind::File,
        libc::DT_LNK => Kind::Symlink,
        // Also DT_UNKNOWN, which some filesystems always report.
        _ => Kind::Other,
    }
}

/// The mount an entry was reached through.
///
/// The mount id (Linux 5.8 and later) is what "the same filesystem" means
/// to a user: a bind mount of a directory on the same device is another
/// mount and is not entered, so its contents are not counted twice, while
/// a btrfs subvolume, which has a device number of its own but is not
/// mounted separately, is part of the scan. Older kernels leave the mount
/// id out and the device number decides, as with `stat`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mount {
    id: Option<u64>,
    device: u64,
}

impl Mount {
    fn of(stat: &Statx) -> Self {
        Self { id: (stat.mask & libc::STATX_MNT_ID != 0).then_some(stat.mnt_id), device: device(stat) }
    }

    fn holds(self, entry: Self) -> bool {
        match (self.id, entry.id) {
            (Some(root), Some(entry)) => root == entry,
            _ => self.device == entry.device,
        }
    }
}

/// The device number, packed the same way for every entry of a walk.
fn device(stat: &Statx) -> u64 {
    (u64::from(stat.dev_major) << 32) | u64::from(stat.dev_minor)
}

fn stat_of(stat: &Statx) -> Stat {
    Stat {
        apparent_size: stat.size,
        allocated_size: platform::allocated_from_blocks(stat.size, stat.blocks, u64::from(stat.blksize)),
        modified: (stat.mask & libc::STATX_MTIME != 0).then(|| time(&stat.mtime)).flatten(),
        link: (stat.nlink > 1).then(|| ((device(stat), stat.ino), Some(u64::from(stat.nlink)))),
    }
}

fn time(stamp: &StatxTimestamp) -> Option<SystemTime> {
    let nanos = Duration::from_nanos(u64::from(stamp.nsec));
    if stamp.sec >= 0 {
        SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(stamp.sec.unsigned_abs()))?.checked_add(nanos)
    } else {
        SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(stamp.sec.unsigned_abs()))?.checked_add(nanos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(inode: u64, kind: u8, name: &[u8]) -> Vec<u8> {
        // Records are padded to eight bytes, with the name NUL-terminated.
        let length = (19 + name.len() + 1).next_multiple_of(8);
        let mut bytes = vec![0u8; length];
        bytes[0..8].copy_from_slice(&inode.to_ne_bytes());
        bytes[16..18].copy_from_slice(&u16::try_from(length).unwrap().to_ne_bytes());
        bytes[18] = kind;
        bytes[19..19 + name.len()].copy_from_slice(name);
        bytes
    }

    #[test]
    fn mount_ids_decide_over_devices_when_known() {
        let mount = |id, device| Mount { id, device };
        let root = mount(Some(30), 1);
        assert!(root.holds(mount(Some(30), 2)), "a btrfs subvolume on the root's mount is inside");
        assert!(!root.holds(mount(Some(31), 1)), "a bind mount of the same device is outside");
        assert!(mount(None, 1).holds(mount(None, 1)), "without mount ids the device decides");
        assert!(!mount(None, 1).holds(mount(Some(30), 2)));
    }

    #[test]
    fn dirents_skip_dot_entries_and_keep_names() {
        let mut buffer = record(2, libc::DT_DIR, b".");
        buffer.extend(record(1, libc::DT_DIR, b".."));
        buffer.extend(record(40, libc::DT_REG, b"a file"));
        buffer.extend(record(7, libc::DT_UNKNOWN, "日本語".as_bytes()));
        let entries: Vec<_> = dirents(&buffer).collect();
        assert_eq!(entries, vec![(40, libc::DT_REG, &b"a file"[..]), (7, libc::DT_UNKNOWN, "日本語".as_bytes())]);
    }

    #[test]
    fn dirents_stop_at_a_truncated_or_empty_record() {
        let mut buffer = record(40, libc::DT_REG, b"whole");
        let cut = buffer.len();
        buffer.extend(record(41, libc::DT_REG, b"truncated"));
        assert_eq!(dirents(&buffer[..cut + 12]).count(), 1);
        let mut zero_length = record(42, libc::DT_REG, b"x");
        zero_length[16..18].copy_from_slice(&0u16.to_ne_bytes());
        assert_eq!(dirents(&zero_length).count(), 0, "a zero length must not loop");
    }
}
