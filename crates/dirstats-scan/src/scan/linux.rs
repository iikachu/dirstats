// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors

//! Experimental Linux walker: directories are listed with `getdents64` and
//! their entries stat'ed with `statx` relative to the open directory.
//!
//! A first version (#4) measured about 2× slower than dua-core's `std::fs`
//! walker on a cold whole-disk scan, and `std::fs` already uses these same
//! calls. Every idea for beating it is kept as a switch in
//! [`LinuxWalker`](super::LinuxWalker) so that `examples/walkbench.rs` can
//! compare them on real filesystems:
//!
//! - inode order: stat each listing sorted by inode number, which is
//!   roughly on-disk order for ext4, XFS and btrfs;
//! - work stealing: rayon's pool instead of one locked job queue;
//! - io_uring: one ring submission per chunk of `statx` calls, so the
//!   kernel can have many inode reads in flight;
//! - `AT_STATX_DONT_SYNC`: network and FUSE mounts answer from cache;
//! - mount id: mount boundaries by mount id rather than device number;
//! - XFS bulkstat: read every inode of an XFS filesystem in inode order up
//!   front (needs root), then stat only directories while walking.
//!
//! Every directory is sent before anything inside it, because its contents
//! are only queued once the batch naming it has been sent. The entries of a
//! wide directory are split into chunks that other threads can stat.

use super::{LinuxWalker, ScanOptions, Stat, Walked, platform};
use crate::tree::Kind;
use foldhash::HashMap;
use std::cell::RefCell;
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
    | libc::STATX_MTIME;
/// From the kernel's UAPI headers; not in every libc crate target.
const STATX_MNT_ID: u32 = 0x1000;
const AT_STATX_DONT_SYNC: libc::c_int = 0x4000;
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
    /// Where the walk must stop when it may not leave its filesystem.
    boundary: Option<Boundary>,
    flags: libc::c_int,
    mask: u32,
    tuning: LinuxWalker,
    /// Attributes of every inode on the filesystem, read up front (XFS bulkstat).
    prefetched: Option<HashMap<u64, Statx>>,
}

#[derive(Clone, Copy)]
enum Boundary {
    Device(u64),
    Mount(u64),
}

impl Boundary {
    fn contains(self, stat: &Statx) -> bool {
        match self {
            Self::Device(device) => device == device_of(stat),
            Self::Mount(id) => stat.mask & STATX_MNT_ID == 0 || stat.mnt_id == id,
        }
    }
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

    fn statx(&self, dir: RawFd, name: &CStr) -> io::Result<Statx> {
        statx(dir, name, self.flags, self.mask)
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
        let tuning = options.linux.clone();
        let flags = FLAGS | if tuning.dont_sync { AT_STATX_DONT_SYNC } else { 0 };
        let mask = MASK | if tuning.mount_id { STATX_MNT_ID } else { 0 };
        let c_root = CString::new(root.as_os_str().as_bytes()).ok()?;
        let stat = statx(libc::AT_FDCWD, &c_root, flags, mask).ok()?;
        let kind = kind_of(&stat);
        let directory = (kind == Kind::Directory).then_some(0);
        let boundary = options.same_filesystem.then(|| {
            if tuning.mount_id && stat.mask & STATX_MNT_ID != 0 {
                Boundary::Mount(stat.mnt_id)
            } else {
                Boundary::Device(device_of(&stat))
            }
        });
        let prefetched = (tuning.xfs_bulkstat && directory.is_some()).then(|| xfs_bulkstat(root, &stat)).flatten();
        let work_stealing = tuning.work_stealing;
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue { jobs: Vec::new(), outstanding: 0 }),
            ready: Condvar::new(),
            stop: AtomicBool::new(false),
            next_directory: AtomicUsize::new(1),
            boundary,
            flags,
            mask,
            tuning,
            prefetched,
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
            let first = Job::Read { path: Arc::from(root), directory: 0 };
            if work_stealing {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .thread_name(|index| format!("dirstats-walk-{index}"))
                    .build()
                    .ok()?;
                let (shared, sender) = (Arc::clone(&shared), sender.clone());
                let driver = std::thread::Builder::new()
                    .name("dirstats-walk".into())
                    .spawn(move || pool.scope(|scope| steal(scope, &shared, &sender, first)))
                    .ok()?;
                workers.push(driver);
            } else {
                {
                    let mut queue = shared.queue.lock().unwrap();
                    queue.jobs.push(first);
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

/// Run one job: send its entries, then hand the directories among them to
/// `spawn`. `false` once the walk has been dropped.
fn run(shared: &Shared, events: &SyncSender<Batch>, job: Job, spawn: &dyn Fn(Job)) -> bool {
    let (batch, children) = match job {
        Job::Read { path, directory } => read(shared, path, directory, spawn),
        Job::Stat { dir, path, directory, names } => stat_all(shared, &dir, &path, directory, names),
    };
    // Sent before the directories in it are queued, so every directory
    // reaches the consumer ahead of its contents.
    if !batch.is_empty() && events.send(batch).is_err() {
        shared.halt();
        return false;
    }
    children.into_iter().for_each(spawn);
    true
}

/// A worker of the locked-queue walker.
fn work(shared: &Shared, events: &SyncSender<Batch>) {
    let spawn = |job| {
        let mut queue = shared.queue.lock().unwrap();
        queue.outstanding += 1;
        queue.jobs.push(job);
        drop(queue);
        shared.ready.notify_one();
    };
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
        if !run(shared, events, job, &spawn) {
            return;
        }
        let mut queue = shared.queue.lock().unwrap();
        queue.outstanding -= 1;
        let done = queue.outstanding == 0;
        drop(queue);
        if done {
            // Wakes idle workers to exit.
            shared.ready.notify_all();
        }
    }
}

/// Run `job` on rayon's pool, spawning its children as further tasks.
fn steal<'scope>(scope: &rayon::Scope<'scope>, shared: &Arc<Shared>, events: &SyncSender<Batch>, job: Job) {
    if shared.stop.load(Ordering::Relaxed) {
        return;
    }
    run(shared, events, job, &|job| {
        let (shared, events) = (Arc::clone(shared), events.clone());
        scope.spawn(move |scope| steal(scope, &shared, &events, job));
    });
}

/// List the directory at `path` and stat its entries, sharing out all but
/// the first chunk of a wide one.
fn read(shared: &Shared, path: Arc<Path>, directory: usize, spawn: &dyn Fn(Job)) -> (Batch, Vec<Job>) {
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
    if shared.tuning.inode_order {
        names.sort_unstable_by_key(|name| name.inode);
    }

    let dir = Arc::new(dir);
    if names.len() > CHUNK {
        let mut rest = names.split_off(CHUNK);
        while !rest.is_empty() {
            let tail = rest.split_off(rest.len().min(CHUNK));
            let chunk = std::mem::replace(&mut rest, tail);
            // Idle workers take the other chunks while this one is stat'ed.
            spawn(Job::Stat { dir: Arc::clone(&dir), path: Arc::clone(&path), directory, names: chunk });
        }
    }
    let (mut batch, children) = stat_all(shared, &dir, &path, directory, names);
    batch.extend(failure.map(Err));
    (batch, children)
}

/// Stat `names` inside `dir` and queue the directories among them.
fn stat_all(shared: &Shared, dir: &File, path: &Arc<Path>, directory: usize, names: Vec<Name>) -> (Batch, Vec<Job>) {
    let stats = stat_names(shared, dir.as_raw_fd(), &names);
    let mut batch = Vec::with_capacity(names.len());
    let mut children = Vec::new();
    for (Name { inode: _, kind, name }, stat) in names.into_iter().zip(stats) {
        let file_name = OsStr::from_bytes(name.to_bytes());
        let (kind, metadata, descend) = match stat {
            Ok(stat) => {
                let kind = kind_of(&stat);
                let inside = shared.boundary.is_none_or(|boundary| boundary.contains(&stat));
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

/// Attributes of each of `names`, in order.
fn stat_names(shared: &Shared, dir: RawFd, names: &[Name]) -> Vec<io::Result<Statx>> {
    let mut stats: Vec<Option<io::Result<Statx>>> = names
        .iter()
        .map(|name| {
            // Directories are always stat'ed: they may be mount points, whose
            // listed inode is the one underneath.
            let prefetched = shared.prefetched.as_ref().filter(|_| name.kind != libc::DT_DIR && name.kind != libc::DT_UNKNOWN);
            prefetched.and_then(|map| map.get(&name.inode)).map(|stat| Ok(stat.clone()))
        })
        .collect();
    let missing: Vec<usize> = (0..names.len()).filter(|&i| stats[i].is_none()).collect();
    if shared.tuning.io_uring
        && let Some(results) = uring_statx(dir, missing.iter().map(|&i| &*names[i].name), shared.flags, shared.mask)
    {
        for (i, result) in missing.into_iter().zip(results) {
            stats[i] = Some(result);
        }
    } else {
        for i in missing {
            stats[i] = Some(shared.statx(dir, &names[i].name));
        }
    }
    stats.into_iter().map(|stat| stat.expect("every name stat'ed")).collect()
}

thread_local! {
    /// This thread's ring; `None` inside once creating one has failed.
    static RING: RefCell<Option<Option<io_uring::IoUring>>> = const { RefCell::new(None) };
}

/// Ring entries, and so `statx` calls in flight per thread.
const RING_ENTRIES: u32 = CHUNK as u32;

pub(super) fn io_uring_available() -> bool {
    io_uring::IoUring::new(8).is_ok()
}

/// `statx` every name through this thread's io_uring, or `None` when
/// there is no ring.
fn uring_statx<'a>(dir: RawFd, names: impl ExactSizeIterator<Item = &'a CStr>, flags: libc::c_int, mask: u32) -> Option<Vec<io::Result<Statx>>> {
    RING.with_borrow_mut(|slot| {
        let ring = slot.get_or_insert_with(|| io_uring::IoUring::new(RING_ENTRIES).ok()).as_mut()?;
        let names: Vec<&CStr> = names.collect();
        let mut buffers: Vec<MaybeUninit<Statx>> = (0..names.len()).map(|_| MaybeUninit::zeroed()).collect();
        let mut results: Vec<Option<io::Result<Statx>>> = (0..names.len()).map(|_| None).collect();
        for start in (0..names.len()).step_by(RING_ENTRIES as usize) {
            let end = (start + RING_ENTRIES as usize).min(names.len());
            for i in start..end {
                let entry = io_uring::opcode::Statx::new(
                    io_uring::types::Fd(dir),
                    names[i].as_ptr(),
                    buffers[i].as_mut_ptr().cast(),
                )
                .flags(flags)
                .mask(mask)
                .build()
                .user_data(i as u64);
                // SAFETY: the name and buffer outlive the submission, which
                // is waited for below; the ring has room for a whole chunk.
                unsafe { ring.submission().push(&entry) }.expect("ring has room for a chunk");
            }
            let mut pending = end - start;
            while pending > 0 {
                if let Err(err) = ring.submit_and_wait(pending) {
                    if err.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    // Entries may still be in flight and write into these
                    // buffers, so leak them and the ring rather than free
                    // them. (The kernel copies each path when it is submitted.)
                    std::mem::forget(buffers);
                    std::mem::forget(slot.replace(None));
                    return None;
                }
                for completion in ring.completion() {
                    let i = completion.user_data() as usize;
                    let result = completion.result();
                    results[i] = Some(if result < 0 {
                        Err(io::Error::from_raw_os_error(-result))
                    } else {
                        // SAFETY: filled in by the kernel; every bit pattern is valid.
                        Ok(unsafe { buffers[i].assume_init_read() })
                    });
                    pending -= 1;
                }
            }
        }
        Some(results.into_iter().map(|result| result.expect("every statx completed")).collect())
    })
}

/// Read the attributes of every inode on the XFS filesystem holding `root`
/// with `XFS_IOC_BULKSTAT`, in inode order. `None` when `root` is not on
/// XFS or the process may not (the ioctl needs `CAP_SYS_ADMIN`).
fn xfs_bulkstat(root: &Path, root_stat: &Statx) -> Option<HashMap<u64, Statx>> {
    const XFS_SUPER_MAGIC: i64 = 0x5846_5342;
    // _IOR('X', 127, struct xfs_bulk_ireq)
    const XFS_IOC_BULKSTAT: libc::c_ulong = 0x8040_587f;
    const PER_CALL: usize = 4096;

    #[repr(C)]
    struct BulkIreq {
        ino: u64,
        flags: u32,
        icount: u32,
        ocount: u32,
        agno: u32,
        reserved: [u64; 5],
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Bulkstat {
        ino: u64,
        size: u64,
        blocks: u64,
        xflags: u64,
        atime: i64,
        mtime: i64,
        ctime: i64,
        btime: i64,
        gen_: u32,
        uid: u32,
        gid: u32,
        projectid: u32,
        atime_nsec: u32,
        mtime_nsec: u32,
        ctime_nsec: u32,
        btime_nsec: u32,
        blksize: u32,
        rdev: u32,
        cowextsize_blks: u32,
        extsize_blks: u32,
        nlink: u32,
        extents: u32,
        aextents: u32,
        version: u16,
        forkoff: u16,
        sick: u16,
        checked: u16,
        mode: u16,
        pad2: u16,
        extents64: u64,
        pad: [u64; 6],
    }
    const _: () = assert!(size_of::<BulkIreq>() == 64);
    const _: () = assert!(size_of::<Bulkstat>() == 192);
    #[repr(C)]
    struct Request {
        header: BulkIreq,
        entries: [MaybeUninit<Bulkstat>; PER_CALL],
    }

    let dir = File::options().read(true).custom_flags(libc::O_DIRECTORY).open(root).ok()?;
    let mut fs = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: the buffer is a whole `struct statfs`.
    if unsafe { libc::fstatfs(dir.as_raw_fd(), fs.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: filled in by fstatfs.
    #[allow(clippy::unnecessary_cast)] // f_type's type differs between architectures
    if unsafe { fs.assume_init() }.f_type as i64 != XFS_SUPER_MAGIC {
        return None;
    }
    let mut request = Box::new(Request {
        header: BulkIreq { ino: 0, flags: 0, icount: PER_CALL as u32, ocount: 0, agno: 0, reserved: [0; 5] },
        entries: [MaybeUninit::zeroed(); PER_CALL],
    });
    let mut map = HashMap::default();
    loop {
        request.header.icount = PER_CALL as u32;
        // SAFETY: the request is a header followed by room for `icount` entries.
        if unsafe { libc::ioctl(dir.as_raw_fd(), XFS_IOC_BULKSTAT as _, &mut *request as *mut Request) } != 0 {
            return None;
        }
        let count = request.header.ocount as usize;
        if count == 0 {
            break;
        }
        for entry in &request.entries[..count] {
            // SAFETY: the kernel filled in the first `ocount` entries.
            let b = unsafe { entry.assume_init() };
            // bs_blocks counts filesystem blocks; statx counts 512-byte ones.
            let blocks = b.blocks * u64::from(b.blksize) / 512;
            let mut stat = Statx::zeroed();
            stat.mask = MASK;
            stat.blksize = b.blksize;
            stat.nlink = b.nlink;
            stat.mode = b.mode;
            stat.ino = b.ino;
            stat.size = b.size;
            stat.blocks = blocks;
            stat.mtime = StatxTimestamp { sec: b.mtime, nsec: b.mtime_nsec, _reserved: 0 };
            stat.dev_major = root_stat.dev_major;
            stat.dev_minor = root_stat.dev_minor;
            stat.mnt_id = root_stat.mnt_id;
            stat.mask |= root_stat.mask & STATX_MNT_ID;
            map.insert(b.ino, stat);
        }
    }
    Some(map)
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
#[derive(Clone)]
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
#[derive(Clone)]
struct StatxTimestamp {
    sec: i64,
    nsec: u32,
    _reserved: i32,
}

const _: () = assert!(size_of::<Statx>() == 256);

impl Statx {
    fn zeroed() -> Self {
        // SAFETY: every bit pattern is valid.
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

fn statx(dir: RawFd, name: &CStr, flags: libc::c_int, mask: u32) -> io::Result<Statx> {
    let mut buffer = MaybeUninit::<Statx>::zeroed();
    // SAFETY: `name` is NUL-terminated and the buffer is a whole `struct statx`.
    let result = unsafe { libc::syscall(libc::SYS_statx, dir, name.as_ptr(), flags, mask, buffer.as_mut_ptr()) };
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

/// The device number, packed the same way for every entry of a walk.
fn device_of(stat: &Statx) -> u64 {
    (u64::from(stat.dev_major) << 32) | u64::from(stat.dev_minor)
}

fn stat_of(stat: &Statx) -> Stat {
    Stat {
        apparent_size: stat.size,
        allocated_size: platform::allocated_from_blocks(stat.size, stat.blocks, u64::from(stat.blksize)),
        modified: (stat.mask & libc::STATX_MTIME != 0).then(|| time(&stat.mtime)).flatten(),
        link: (stat.nlink > 1).then(|| ((device_of(stat), stat.ino), Some(u64::from(stat.nlink)))),
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
