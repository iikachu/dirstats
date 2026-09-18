// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors
//
// Traversal is provided by dua-core (MIT, by Sebastian Thiel,
// https://github.com/Byron/dua-cli), which uses getattrlistbulk on macOS and
// FileIdBothDirectoryInfo enumeration on Windows. On Linux the `linux-fast`
// feature swaps in the getdents64 and statx walker in `scan/linux.rs`.
//
// Capping inflated block counts on Linux NTFS mounts is an idea credited to
// dust (https://github.com/bootandy/dust, issue #295); the code here was
// written independently.

//! Parallel filesystem scan.

use crate::tree::{Kind, Node, NodeId, SizeMetric, Tree};
use dua_core::{Entry, Order};
use foldhash::HashMap;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::SystemTime;

#[cfg(all(target_os = "linux", feature = "linux-fast"))]
mod linux;

#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Worker threads for directory reads.
    pub threads: usize,
    /// Do not descend into other mounted filesystems (Unix only for now).
    pub same_filesystem: bool,
    /// Count data reachable through several hard links only once.
    pub count_hard_links_once: bool,
    pub size_metric: SizeMetric,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            threads: std::thread::available_parallelism().map_or(4, |n| n.get()),
            same_filesystem: true,
            count_hard_links_once: true,
            size_metric: SizeMetric::default(),
        }
    }
}

/// Counters updated while a scan runs; safe to read from another thread.
#[derive(Debug, Default)]
pub struct Progress {
    pub entries: AtomicU64,
    pub errors: AtomicU64,
}

/// Scan `root` to completion.
pub fn scan(root: impl AsRef<Path>, options: &ScanOptions) -> io::Result<Tree> {
    scan_with(root, options, &AtomicBool::new(false), &Progress::default())
}

/// Scan `root`, reporting into `progress` and stopping with
/// [`io::ErrorKind::Interrupted`] once `cancel` is set.
pub fn scan_with(
    root: impl AsRef<Path>,
    options: &ScanOptions,
    cancel: &AtomicBool,
    progress: &Progress,
) -> io::Result<Tree> {
    let root = root.as_ref();
    #[cfg(all(target_os = "linux", feature = "linux-fast"))]
    if let Some(mut walk) = linux::Walk::start(root, options) {
        return build(root, options, cancel, progress, || walk.next(cancel));
    }
    walk_generic(root, options, cancel, progress)
}

/// Scan with dua-core's walker, which every platform supports.
fn walk_generic(
    root: &Path,
    options: &ScanOptions,
    cancel: &AtomicBool,
    progress: &Progress,
) -> io::Result<Tree> {
    platform::prepare_process();
    let root_device = platform::root_device(root)?;
    let same_filesystem = options.same_filesystem;
    // Directories that only repeat content reachable elsewhere under the
    // root (macOS firmlink targets), never entered.
    let duplicates = platform::duplicate_directories(root);
    let descend = move |entry: &Entry| {
        if !duplicates.is_empty()
            && entry.file_type.is_dir()
            && duplicates.iter().any(|d| d.parent() == Some(&*entry.parent_path) && d.file_name() == Some(&*entry.file_name))
        {
            return false;
        }
        match (same_filesystem, root_device, &entry.metadata) {
            (true, Some(root_device), Some(Ok(metadata))) => {
                platform::device(metadata).is_none_or(|device| device == root_device)
            }
            _ => true,
        }
    };

    let mut walk = dua_core::walk(
        root,
        options.threads,
        Order::ParentFirst,
        dua_core::Options::default(),
        descend,
    );
    build(root, options, cancel, progress, || {
        walk.next_cancellable(cancel).map(|item| item.map(walked))
    })
}

/// One entry of a walk. Walks yield every directory before its contents.
struct Walked {
    /// Walk-local index of the directory holding this entry; `None` for the root.
    parent: Option<usize>,
    /// Walk-local index of this entry when it is a directory.
    directory: Option<usize>,
    name: Box<OsStr>,
    kind: Kind,
    /// `None` when the walk did not ask for metadata.
    metadata: Option<io::Result<Stat>>,
}

/// The attributes of an entry that a scan uses.
struct Stat {
    apparent_size: u64,
    allocated_size: u64,
    modified: Option<SystemTime>,
    /// Identity of multiply-linked data and its total link count, when known.
    link: Option<((u64, u64), Option<u64>)>,
}

fn walked(entry: Entry) -> Walked {
    let kind = if entry.file_type.is_dir() {
        Kind::Directory
    } else if entry.file_type.is_file() {
        Kind::File
    } else if entry.file_type.is_symlink() {
        Kind::Symlink
    } else {
        Kind::Other
    };
    Walked {
        parent: entry.parent_directory_id.map(|id| id.index()),
        directory: entry.directory_id.map(|id| id.index()),
        name: entry.file_name.as_os_str().into(),
        kind,
        metadata: entry.metadata.map(|metadata| {
            metadata.map(|metadata| Stat {
                apparent_size: platform::len(&metadata),
                allocated_size: platform::allocated_size(&metadata),
                modified: metadata.modified().ok(),
                link: platform::link_identity(&metadata),
            })
        }),
    }
}

/// Build a tree from the entries `next` yields.
fn build(
    root: &Path,
    options: &ScanOptions,
    cancel: &AtomicBool,
    progress: &Progress,
    mut next: impl FnMut() -> Option<io::Result<Walked>>,
) -> io::Result<Tree> {
    let mut tree = Tree::new();
    // Maps the walk's dense directory indices to tree nodes.
    let mut directory_nodes: Vec<Option<NodeId>> = Vec::new();
    // Hard-linked data seen so far, with the links not yet encountered. An
    // entry is dropped once every link has been seen, so the map only holds
    // links still outstanding (as dua-cli's inode filter does). Unknown link
    // counts (Windows enumeration) are kept for the whole scan.
    let mut pending_links: HashMap<(u64, u64), u64> = HashMap::default();

    // An error before any child of the root has arrived is most likely
    // the root's own listing failing (permission denied, for instance).
    // Kept so that a scan yielding nothing but the root can fail with it.
    let mut root_error: Option<io::Error> = None;

    while let Some(item) = next() {
        let entry = match item {
            Ok(entry) => entry,
            Err(err) if tree.is_empty() => return Err(err),
            Err(err) => {
                if tree.len() == 1 && root_error.is_none() {
                    root_error = Some(err);
                }
                progress.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };

        let parent = match entry.parent {
            None => None,
            Some(index) => match directory_nodes.get(index).copied().flatten() {
                Some(parent) => Some(parent),
                None => continue,
            },
        };
        let name: Box<OsStr> = if parent.is_none() {
            root.as_os_str().into()
        } else {
            entry.name
        };
        let kind = entry.kind;

        let mut node = Node {
            name,
            parent,
            kind,
            apparent_size: 0,
            allocated_size: 0,
            file_count: u64::from(kind != Kind::Directory),
            dir_count: 0,
            modified: None,
            duplicate_link: false,
            error: false,
        };
        match entry.metadata {
            Some(Ok(stat)) => {
                node.apparent_size = stat.apparent_size;
                node.allocated_size = stat.allocated_size;
                node.modified = stat.modified;
                if options.count_hard_links_once
                    && kind == Kind::File
                    && let Some((identity, links)) = stat.link
                {
                    node.duplicate_link = match pending_links.entry(identity) {
                        std::collections::hash_map::Entry::Vacant(slot) => {
                            slot.insert(links.map_or(u64::MAX, |n| n.saturating_sub(1)));
                            false
                        }
                        std::collections::hash_map::Entry::Occupied(mut slot) => {
                            if links.is_some() {
                                let remaining = slot.get_mut();
                                *remaining -= 1;
                                if *remaining == 0 {
                                    slot.remove();
                                }
                            }
                            true
                        }
                    };
                }
            }
            Some(Err(_)) => {
                node.error = true;
                progress.errors.fetch_add(1, Ordering::Relaxed);
            }
            None => {}
        }

        let id = tree.push(node);
        if let Some(index) = entry.directory {
            if directory_nodes.len() <= index {
                directory_nodes.resize(index + 1, None);
            }
            directory_nodes[index] = Some(id);
        }
        progress.entries.fetch_add(1, Ordering::Relaxed);
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"));
    }
    if tree.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "nothing scanned"));
    }
    if tree.len() == 1
        && let Some(err) = root_error
    {
        return Err(io::Error::new(err.kind(), format!("cannot read {}: {err}", root.display())));
    }
    tree.finish(options.size_metric);
    Ok(tree)
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use dua_core::Metadata;
    use std::os::unix::fs::MetadataExt;
    use std::{io, path::Path};

    /// Nothing to prepare on this platform.
    pub fn prepare_process() {}

    /// No directory repeats another on this platform.
    pub fn duplicate_directories(_root: &Path) -> Vec<std::path::PathBuf> {
        Vec::new()
    }

    pub fn root_device(root: &Path) -> io::Result<Option<u64>> {
        Ok(Some(std::fs::symlink_metadata(root)?.dev()))
    }

    pub fn device(metadata: &Metadata) -> Option<u64> {
        Some(metadata.dev())
    }

    pub fn len(metadata: &Metadata) -> u64 {
        metadata.len()
    }

    /// Blocks beyond the file's length that we still believe (preallocation).
    const PLAUSIBLE_EXTRA_BLOCKS: u64 = 1 << 16;

    pub fn allocated_size(metadata: &Metadata) -> u64 {
        allocated_from_blocks(metadata.len(), metadata.blocks(), metadata.blksize())
    }

    /// `blocks` in 512-byte units, unless it is implausibly larger than the
    /// file (seen on NTFS mounts), in which case the length rounded up to whole
    /// I/O blocks is used instead.
    pub fn allocated_from_blocks(len: u64, blocks: u64, io_block: u64) -> u64 {
        let reported = blocks.saturating_mul(512);
        let io_block = io_block.max(1);
        let rounded_len = len.next_multiple_of(io_block);
        let plausible_max =
            rounded_len.saturating_add(io_block.saturating_mul(PLAUSIBLE_EXTRA_BLOCKS));
        if reported <= plausible_max {
            reported
        } else {
            rounded_len
        }
    }

    /// Identity of multiply-linked data and its total link count.
    pub fn link_identity(metadata: &Metadata) -> Option<((u64, u64), Option<u64>)> {
        (metadata.nlink() > 1).then(|| ((metadata.dev(), metadata.ino()), Some(metadata.nlink())))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use dua_core::Metadata;
    use std::os::unix::fs::MetadataExt;
    use std::{io, path::Path};

    // From <sys/resource.h>; not in the libc crate this project pins.
    const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: std::ffi::c_int = 3;
    const IOPOL_SCOPE_PROCESS: std::ffi::c_int = 0;
    const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: std::ffi::c_int = 1;

    unsafe extern "C" {
        fn setiopolicy_np(iotype: std::ffi::c_int, scope: std::ffi::c_int, policy: std::ffi::c_int) -> std::ffi::c_int;
    }

    /// Never download evicted iCloud Drive (or other file-provider) items.
    ///
    /// By default macOS materialises a "dataless" file or folder when
    /// something reads it, so a scan that walks into an evicted folder
    /// blocks for the download and the download itself inflates the disk
    /// usage being measured. With materialisation off, such reads fail
    /// with `EDEADLK` and are counted as errors; the entry's attributes
    /// still come back, with zero blocks allocated, which is the honest
    /// on-disk figure. Process scope, so every worker thread is covered.
    pub fn prepare_process() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| unsafe {
            // Failure only means the policy is unsupported; nothing to do.
            let _ = setiopolicy_np(
                IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
                IOPOL_SCOPE_PROCESS,
                IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
            );
        });
    }

    pub fn root_device(root: &Path) -> io::Result<Option<u64>> {
        Ok(Some(std::fs::symlink_metadata(root)?.dev()))
    }

    pub fn device(metadata: &Metadata) -> Option<u64> {
        Some(metadata.dev())
    }

    /// Firmlink targets on the Data volume that a scan above them would
    /// count a second time.
    ///
    /// Since Catalina the boot disk is a volume group: the sealed system
    /// volume at `/` and the Data volume at `/System/Volumes/Data`, with
    /// firmlinks grafting Data folders into the system tree (`/Users` is
    /// `/System/Volumes/Data/Users`). Both share one device id, so the
    /// same-filesystem test cannot separate them, and a scan of `/`
    /// would see every user file twice. The system's own table names
    /// every firmlink, so exactly those targets are left out.
    pub fn duplicate_directories(root: &Path) -> Vec<std::path::PathBuf> {
        let table = std::fs::read_to_string("/usr/share/firmlinks").unwrap_or_default();
        firmlink_duplicates(&table, root)
    }

    pub(super) fn firmlink_duplicates(table: &str, root: &Path) -> Vec<std::path::PathBuf> {
        let data = Path::new("/System/Volumes/Data");
        table
            .lines()
            .filter_map(|line| {
                let mut columns = line.split('\t');
                Some((Path::new(columns.next()?), data.join(columns.next()?.trim_start_matches('/'))))
            })
            // A target is a duplicate only when its firmlink is inside the
            // scan too. Scanning the Data volume itself, or the target or
            // something within it, must still count it.
            .filter(|(source, target)| source.starts_with(root) && !root.starts_with(target))
            .map(|(_, target)| target)
            .collect()
    }

    pub fn len(metadata: &Metadata) -> u64 {
        metadata.len()
    }

    pub fn allocated_size(metadata: &Metadata) -> u64 {
        metadata.allocated_size()
    }

    /// Identity of multiply-linked data and its total link count.
    pub fn link_identity(metadata: &Metadata) -> Option<((u64, u64), Option<u64>)> {
        (metadata.nlink() > 1).then(|| ((metadata.dev(), metadata.ino()), Some(metadata.nlink())))
    }
}

#[cfg(windows)]
mod platform {
    use dua_core::Metadata;
    use std::{io, path::Path};

    /// Nothing to prepare on this platform.
    pub fn prepare_process() {}

    /// No directory repeats another on this platform.
    pub fn duplicate_directories(_root: &Path) -> Vec<std::path::PathBuf> {
        Vec::new()
    }

    pub fn root_device(root: &Path) -> io::Result<Option<u64>> {
        std::fs::symlink_metadata(root)?;
        // TODO: compare volume serials once dua-core exposes them for all entries.
        Ok(None)
    }

    pub fn device(_metadata: &Metadata) -> Option<u64> {
        None
    }

    pub fn len(metadata: &Metadata) -> u64 {
        metadata.len()
    }

    pub fn allocated_size(metadata: &Metadata) -> u64 {
        metadata.allocated_size()
    }

    /// Link counts are not part of directory enumeration, so every file id
    /// is tracked for the whole scan.
    pub fn link_identity(metadata: &Metadata) -> Option<((u64, u64), Option<u64>)> {
        metadata.hard_link_id().map(|id| (id, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sums_sizes_and_sorts_children() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("small"), vec![0u8; 10]).unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/big"), vec![0u8; 100_000]).unwrap();

        let options = ScanOptions {
            size_metric: SizeMetric::Apparent,
            ..ScanOptions::default()
        };
        let tree = scan(dir.path(), &options).unwrap();
        let root = tree.root();
        assert_eq!(tree.node(root).file_count, 2);
        assert_eq!(tree.node(root).dir_count, 1, "one subdirectory below the root");
        let newest = tree.nodes().filter_map(|(_, n)| n.modified).max();
        assert_eq!(tree.node(root).modified, newest, "root carries the newest change below it");
        let children = tree.children(root);
        assert_eq!(children.len(), 2);
        assert_eq!(&*tree.node(children[0]).name, OsStr::new("sub"));
        assert!(tree.size(root) >= 100_010);
        assert_eq!(tree.path(children[1]), dir.path().join("small"));
    }

    #[cfg(unix)]
    #[test]
    fn counts_hard_links_once() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), vec![1u8; 50_000]).unwrap();
        fs::hard_link(dir.path().join("a"), dir.path().join("b")).unwrap();

        let options = ScanOptions {
            size_metric: SizeMetric::Apparent,
            ..ScanOptions::default()
        };
        let tree = scan(dir.path(), &options).unwrap();
        let duplicates = tree.nodes().filter(|(_, n)| n.duplicate_link).count();
        assert_eq!(duplicates, 1);
        assert!(tree.size(tree.root()) < 100_000);
    }

    #[test]
    fn cancelled_scan_errors() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(true);
        let result = scan_with(
            dir.path(),
            &ScanOptions::default(),
            &cancel,
            &Progress::default(),
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    }

    // The following cases mirror dust's symlink and size tests
    // (Apache-2.0, by bootandy and contributors), rewritten against this API.

    fn find(tree: &Tree, name: &str) -> Option<NodeId> {
        tree.nodes()
            .find(|(_, n)| &*n.name == OsStr::new(name))
            .map(|(id, _)| id)
    }

    #[test]
    fn reports_exact_apparent_sizes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a_file"), b"").unwrap();
        fs::write(dir.path().join("hello_file"), b"hello\n").unwrap();

        let options = ScanOptions {
            size_metric: SizeMetric::Apparent,
            ..ScanOptions::default()
        };
        let tree = scan(dir.path(), &options).unwrap();
        assert_eq!(tree.size(find(&tree, "a_file").unwrap()), 0);
        assert_eq!(tree.size(find(&tree, "hello_file").unwrap()), 6);
        // Directory entries carry their own on-disk size, so only a lower bound holds.
        assert!(tree.size(tree.root()) >= 6);
    }

    #[test]
    fn preserves_unicode_names() {
        let dir = tempfile::tempdir().unwrap();
        let names = ["ラウトは難しいです！.japan", "👩.unicode"];
        for name in names {
            fs::write(dir.path().join(name), b"").unwrap();
        }

        let tree = scan(dir.path(), &ScanOptions::default()).unwrap();
        for name in names {
            let id = find(&tree, name).expect(name);
            assert_eq!(tree.path(id), dir.path().join(name));
        }
        assert_eq!(tree.node(tree.root()).file_count, 2);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_file_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.txt"), vec![0u8; 10_000]).unwrap();
        std::os::unix::fs::symlink(dir.path().join("notes.txt"), dir.path().join("the_link"))
            .unwrap();

        let options = ScanOptions {
            size_metric: SizeMetric::Apparent,
            ..ScanOptions::default()
        };
        let tree = scan(dir.path(), &options).unwrap();
        let link = find(&tree, "the_link").unwrap();
        assert_eq!(tree.node(link).kind, Kind::Symlink);
        assert!(tree.size(link) < 10_000, "link counted its target's size");
        assert_eq!(tree.node(tree.root()).file_count, 2);
        assert!(tree.size(tree.root()) < 20_000);
    }

    #[cfg(unix)]
    #[test]
    fn recursive_symlink_terminates() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(dir.path(), dir.path().join("the_link")).unwrap();

        let tree = scan(dir.path(), &ScanOptions::default()).unwrap();
        let link = find(&tree, "the_link").unwrap();
        assert_eq!(tree.node(link).kind, Kind::Symlink);
        assert!(tree.children(link).is_empty());
        assert_eq!(tree.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn hard_link_counted_once_across_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        fs::write(dir.path().join("a/notes.txt"), vec![1u8; 50_000]).unwrap();
        fs::hard_link(
            dir.path().join("a/notes.txt"),
            dir.path().join("a/b/the_link"),
        )
        .unwrap();

        let options = ScanOptions {
            size_metric: SizeMetric::Apparent,
            ..ScanOptions::default()
        };
        let tree = scan(dir.path(), &options).unwrap();
        let duplicates = tree.nodes().filter(|(_, n)| n.duplicate_link).count();
        assert_eq!(duplicates, 1);

        let counted_twice = scan(
            dir.path(),
            &ScanOptions {
                count_hard_links_once: false,
                ..options
            },
        )
        .unwrap();
        // Directory sizes vary by filesystem; the link must add exactly one more copy.
        assert_eq!(
            counted_twice.size(counted_twice.root()) - tree.size(tree.root()),
            50_000
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_root_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc_geteuid() } == 0 {
            return; // root ignores permission bits
        }
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("f"), b"x").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let result = scan(&locked, &ScanOptions::default());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let err = result.expect_err("an unreadable root fails the scan");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("locked"), "{err}");
    }

    #[cfg(unix)]
    unsafe extern "C" {
        #[link_name = "geteuid"]
        fn libc_geteuid() -> u32;
    }

    #[cfg(all(target_os = "linux", feature = "linux-fast"))]
    #[test]
    fn linux_walker_matches_the_portable_one() {
        use std::collections::BTreeMap;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        fs::create_dir(root.join("empty")).unwrap();
        fs::create_dir(root.join("wide")).unwrap();
        // More entries than one stat chunk, so the chunks are shared out.
        for i in 0..1000 {
            fs::write(root.join("wide").join(format!("f{i}")), vec![7u8; i]).unwrap();
            if i % 100 == 0 {
                fs::create_dir(root.join("wide").join(format!("d{i}"))).unwrap();
                fs::write(root.join("wide").join(format!("d{i}/inner")), b"x").unwrap();
            }
        }
        fs::write(root.join("a/b/c/deep"), vec![1u8; 70_000]).unwrap();
        fs::write(root.join("ラウト.txt"), b"hello").unwrap();
        fs::hard_link(root.join("a/b/c/deep"), root.join("a/link")).unwrap();
        std::os::unix::fs::symlink(root.join("a"), root.join("to_a")).unwrap();

        // Which of two links counts as the duplicate depends on arrival
        // order, which differs between walkers; compare with both counted.
        let options = ScanOptions { threads: 3, count_hard_links_once: false, ..ScanOptions::default() };
        assert!(linux::Walk::start(root, &options).is_some(), "the Linux walker runs here");
        let links_once = ScanOptions { count_hard_links_once: true, ..options.clone() };
        assert_eq!(scan(root, &links_once).unwrap().nodes().filter(|(_, n)| n.duplicate_link).count(), 1);
        for metric in [SizeMetric::Apparent, SizeMetric::Allocated] {
            let options = ScanOptions { size_metric: metric, ..options.clone() };
            let describe = |tree: &Tree| {
                tree.nodes()
                    .map(|(id, n)| (tree.path(id), (n.kind, tree.size(id), n.file_count, n.dir_count, n.error, n.modified)))
                    .collect::<BTreeMap<_, _>>()
            };
            let fast = scan(root, &options).unwrap();
            let portable = walk_generic(root, &options, &AtomicBool::new(false), &Progress::default()).unwrap();
            assert_eq!(describe(&fast), describe(&portable), "{metric:?}");
        }
    }

    /// Mount points directly below `dir`, from this process's mount table.
    #[cfg(all(target_os = "linux", feature = "linux-fast"))]
    fn mounts_below(dir: &Path) -> Vec<std::path::PathBuf> {
        let table = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        table
            .lines()
            .filter_map(|line| line.split(' ').nth(4))
            .map(std::path::PathBuf::from)
            .filter(|mount| mount.parent() == Some(dir))
            .collect()
    }

    #[cfg(all(target_os = "linux", feature = "linux-fast"))]
    #[test]
    fn linux_walker_stays_on_the_roots_mount() {
        let dev = Path::new("/dev");
        let mounts = mounts_below(dev);
        if mounts.is_empty() {
            eprintln!("skipped: nothing is mounted below /dev here");
            return;
        }
        let tree = scan(dev, &ScanOptions::default()).unwrap();
        let mut checked = 0;
        for (id, node) in tree.nodes() {
            if node.kind == Kind::Directory && mounts.contains(&tree.path(id)) {
                assert!(tree.children(id).is_empty(), "{} was entered", tree.path(id).display());
                checked += 1;
            }
        }
        assert!(checked > 0, "no mount point below /dev was listed: {mounts:?}");
    }

    /// Run by `bind_mounts_are_not_entered` inside a private mount namespace.
    #[cfg(all(target_os = "linux", feature = "linux-fast"))]
    #[test]
    #[ignore = "run inside a mount namespace by bind_mounts_are_not_entered"]
    fn bind_mount_scan_inside_namespace() {
        let Some(root) = std::env::var_os("DIRSTATS_BIND_ROOT") else {
            return;
        };
        let root = Path::new(&root);
        let tree = scan(root, &ScanOptions::default()).unwrap();
        let bound = find(&tree, "bound").unwrap();
        assert!(tree.children(bound).is_empty(), "the bind mount was entered");
        let payloads = tree.nodes().filter(|(_, n)| &*n.name == OsStr::new("payload")).count();
        assert_eq!(payloads, 1, "the bound data is counted once");
    }

    #[cfg(all(target_os = "linux", feature = "linux-fast"))]
    #[test]
    fn bind_mounts_are_not_entered() {
        use std::process::Command;
        // A bind mount needs a mount namespace, which unprivileged users only
        // get where user namespaces are allowed (not on Ubuntu 24.04 runners).
        let probe = Command::new("unshare").args(["--user", "--map-root-user", "--mount", "true"]).output();
        if !probe.is_ok_and(|output| output.status.success()) {
            eprintln!("skipped: cannot create a user and mount namespace here");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("data")).unwrap();
        fs::write(dir.path().join("data/payload"), vec![0u8; 40_000]).unwrap();
        fs::create_dir(dir.path().join("bound")).unwrap();
        let status = Command::new("unshare")
            .args(["--user", "--map-root-user", "--mount", "sh", "-c"])
            .arg(r#"mount --bind "$1/data" "$1/bound" && exec "$2" --exact scan::tests::bind_mount_scan_inside_namespace --ignored --nocapture"#)
            .arg("sh")
            .arg(dir.path())
            .arg(std::env::current_exe().unwrap())
            .env("DIRSTATS_BIND_ROOT", dir.path())
            .status()
            .unwrap();
        assert!(status.success(), "the scan inside the namespace failed");
    }

    #[cfg(all(target_os = "linux", feature = "linux-fast"))]
    #[test]
    fn linux_walker_stops_on_cancel() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..50 {
            fs::create_dir(dir.path().join(format!("d{i}"))).unwrap();
        }
        let cancel = AtomicBool::new(false);
        let mut walk = linux::Walk::start(dir.path(), &ScanOptions::default()).unwrap();
        assert!(walk.next(&cancel).is_some(), "the root comes first");
        cancel.store(true, Ordering::Relaxed);
        assert!(walk.next(&cancel).is_none());
        drop(walk); // joins the workers; hangs if one missed the stop
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn firmlink_targets_are_skipped_only_from_above() {
        use std::path::PathBuf;
        let table = "/Users\tUsers\n/usr/local\tusr/local\n";
        let data = |s: &str| PathBuf::from("/System/Volumes/Data").join(s);
        assert_eq!(platform::firmlink_duplicates(table, Path::new("/")), vec![data("Users"), data("usr/local")]);
        assert!(platform::firmlink_duplicates(table, Path::new("/System/Volumes/Data/Users/me")).is_empty(), "no firmlink below a Data folder");
        assert_eq!(platform::firmlink_duplicates(table, Path::new("/Users/me")), Vec::<PathBuf>::new(), "no firmlink inside a home scan");
        assert_eq!(platform::firmlink_duplicates(table, Path::new("/usr")), vec![data("usr/local")]);
        assert!(platform::firmlink_duplicates(table, Path::new("/System/Volumes/Data")).is_empty(), "the Data volume counts everything");
        assert!(platform::firmlink_duplicates("", Path::new("/")).is_empty());
    }
}
