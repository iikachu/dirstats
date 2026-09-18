// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors

//! Archived experiment: a Linux walker that lists directories with
//! `getdents64` and stats entries with `statx`, with each idea for making it
//! faster than dua-core's `std::fs` walker as a switch. See README.md for
//! the results that got it shelved and how to run the bench again.
//!
//! Scans build an ordinary [`dirstats_scan::Tree`] through the public
//! [`TreeBuilder`], so results compare directly with [`dirstats_scan::scan`].

use dirstats_scan::{Kind, Node, NodeId, SizeMetric, Tree, TreeBuilder};
use foldhash::HashMap;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::time::SystemTime;

#[cfg(target_os = "linux")]
mod walker;

/// Which variant of the walker to run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinuxWalker {
    /// Stat each directory's entries in inode order rather than listing order.
    pub inode_order: bool,
    /// Share work through rayon's work-stealing pool instead of one locked queue.
    pub work_stealing: bool,
    /// Submit each directory's `statx` calls as one io_uring batch.
    pub io_uring: bool,
    /// Pass `AT_STATX_DONT_SYNC`: answer network and FUSE mounts from cached attributes.
    pub dont_sync: bool,
    /// Detect mount boundaries by mount id, so bind mounts are recognised.
    pub mount_id: bool,
    /// On XFS, as root, read every inode's attributes up front with
    /// `XFS_IOC_BULKSTAT` and stat only directories while walking.
    pub xfs_bulkstat: bool,
}

impl LinuxWalker {
    /// Every variant measured in #18, named for reports.
    pub fn variants() -> Vec<(&'static str, Self)> {
        let steal = Self { work_stealing: true, ..Self::default() };
        vec![
            ("queue + inode order (#4)", Self { inode_order: true, ..Self::default() }),
            ("queue", Self::default()),
            ("stealing", steal.clone()),
            ("stealing + inode order", Self { inode_order: true, ..steal.clone() }),
            ("stealing + io_uring", Self { io_uring: true, ..steal.clone() }),
            ("stealing + io_uring + inode order", Self { io_uring: true, inode_order: true, ..steal.clone() }),
            ("stealing + dont_sync", Self { dont_sync: true, ..steal.clone() }),
            ("stealing + mount id", Self { mount_id: true, ..steal.clone() }),
            ("stealing + xfs bulkstat", Self { xfs_bulkstat: true, ..steal }),
        ]
    }
}

/// The subset of `dirstats_scan::ScanOptions` the walker honours.
#[derive(Clone, Debug)]
pub struct Options {
    pub threads: usize,
    pub same_filesystem: bool,
    pub count_hard_links_once: bool,
    pub size_metric: SizeMetric,
    pub walker: LinuxWalker,
}

impl Default for Options {
    fn default() -> Self {
        let scan = dirstats_scan::ScanOptions::default();
        Self {
            threads: scan.threads,
            same_filesystem: scan.same_filesystem,
            count_hard_links_once: scan.count_hard_links_once,
            size_metric: scan.size_metric,
            walker: LinuxWalker::default(),
        }
    }
}

/// Whether this kernel lets the process create an io_uring; the walker
/// falls back to plain `statx` without one.
pub fn io_uring_available() -> bool {
    #[cfg(target_os = "linux")]
    return walker::io_uring_available();
    #[allow(unreachable_code)]
    false
}

/// Scan `root` with the archived walker. Linux only; elsewhere, and when
/// this kernel has no `statx`, it fails with `Unsupported`.
pub fn scan(root: impl AsRef<Path>, options: &Options) -> io::Result<Tree> {
    let root = root.as_ref();
    #[cfg(target_os = "linux")]
    if let Some(mut walk) = walker::Walk::start(root, options) {
        let never = std::sync::atomic::AtomicBool::new(false);
        return build(root, options, || walk.next(&never));
    }
    let _ = (root, options);
    Err(io::Error::new(io::ErrorKind::Unsupported, "the archived walker needs Linux 4.11+ with statx"))
}

/// One entry of a walk. Walks yield every directory before its contents.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct Stat {
    apparent_size: u64,
    allocated_size: u64,
    modified: Option<SystemTime>,
    /// Identity of multiply-linked data and its total link count.
    link: Option<((u64, u64), Option<u64>)>,
}

/// `blocks` in 512-byte units, unless implausibly larger than the file;
/// the same rule as dirstats-scan's Unix platform code.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn allocated_from_blocks(len: u64, blocks: u64, io_block: u64) -> u64 {
    const PLAUSIBLE_EXTRA_BLOCKS: u64 = 1 << 16;
    let reported = blocks.saturating_mul(512);
    let io_block = io_block.max(1);
    let rounded_len = len.next_multiple_of(io_block);
    let plausible_max = rounded_len.saturating_add(io_block.saturating_mul(PLAUSIBLE_EXTRA_BLOCKS));
    if reported <= plausible_max { reported } else { rounded_len }
}

/// Build a tree from the entries `next` yields, as dirstats-scan does.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn build(root: &Path, options: &Options, mut next: impl FnMut() -> Option<io::Result<Walked>>) -> io::Result<Tree> {
    let mut tree = TreeBuilder::new();
    let mut len = 0usize;
    let mut directory_nodes: Vec<Option<NodeId>> = Vec::new();
    // Hard-linked data seen so far, with the links not yet encountered.
    let mut pending_links: HashMap<(u64, u64), u64> = HashMap::default();
    let mut root_error: Option<io::Error> = None;

    while let Some(item) = next() {
        let entry = match item {
            Ok(entry) => entry,
            Err(err) if len == 0 => return Err(err),
            Err(err) => {
                if len == 1 && root_error.is_none() {
                    root_error = Some(err);
                }
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
        let name: Box<OsStr> = if parent.is_none() { root.as_os_str().into() } else { entry.name };
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
            Some(Err(_)) => node.error = true,
            None => {}
        }
        let id = tree.push(node);
        len += 1;
        if let Some(index) = entry.directory {
            if directory_nodes.len() <= index {
                directory_nodes.resize(index + 1, None);
            }
            directory_nodes[index] = Some(id);
        }
    }
    if len == 0 {
        return Err(io::Error::new(io::ErrorKind::NotFound, "nothing scanned"));
    }
    if len == 1
        && let Some(err) = root_error
    {
        return Err(io::Error::new(err.kind(), format!("cannot read {}: {err}", root.display())));
    }
    Ok(tree.finish(options.size_metric))
}
