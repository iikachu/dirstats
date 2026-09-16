// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors
//
// Traversal is provided by dua-core (MIT, by Sebastian Thiel,
// https://github.com/Byron/dua-cli), which uses getattrlistbulk on macOS and
// FileIdBothDirectoryInfo enumeration on Windows.
//
// Capping inflated block counts on Linux NTFS mounts is an idea credited to
// dust (https://github.com/bootandy/dust, issue #295); the code here was
// written independently.

//! Parallel filesystem scan.

use crate::tree::{Kind, Node, NodeId, SizeMetric, Tree};
use dua_core::{Entry, Order};
use foldhash::HashSet;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
    let root_device = platform::root_device(root)?;
    let same_filesystem = options.same_filesystem;
    let descend = move |entry: &Entry| match (same_filesystem, root_device, &entry.metadata) {
        (true, Some(root_device), Some(Ok(metadata))) => {
            platform::device(metadata).is_none_or(|device| device == root_device)
        }
        _ => true,
    };

    let mut walk = dua_core::walk(
        root,
        options.threads,
        Order::ParentFirst,
        dua_core::Options::default(),
        descend,
    );
    let mut tree = Tree::new();
    // Maps dua-core's dense directory ids to tree nodes.
    let mut directory_nodes: Vec<Option<NodeId>> = Vec::new();
    let mut seen_links = HashSet::default();

    while let Some(item) = walk.next_cancellable(cancel) {
        let entry = match item {
            Ok(entry) => entry,
            Err(err) if tree.is_empty() => return Err(err),
            Err(_) => {
                progress.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };

        let parent = match entry.parent_directory_id {
            None => None,
            Some(id) => match directory_nodes.get(id.index()).copied().flatten() {
                Some(parent) => Some(parent),
                None => continue,
            },
        };
        let name: Box<OsStr> = if parent.is_none() {
            root.as_os_str().into()
        } else {
            entry.file_name.as_os_str().into()
        };
        let kind = if entry.file_type.is_dir() {
            Kind::Directory
        } else if entry.file_type.is_file() {
            Kind::File
        } else if entry.file_type.is_symlink() {
            Kind::Symlink
        } else {
            Kind::Other
        };

        let mut node = Node {
            name,
            parent,
            kind,
            apparent_size: 0,
            allocated_size: 0,
            file_count: u64::from(kind != Kind::Directory),
            duplicate_link: false,
            error: false,
        };
        match &entry.metadata {
            Some(Ok(metadata)) => {
                node.apparent_size = platform::len(metadata);
                node.allocated_size = platform::allocated_size(metadata);
                if options.count_hard_links_once
                    && kind == Kind::File
                    && let Some(identity) = platform::link_identity(metadata)
                {
                    node.duplicate_link = !seen_links.insert(identity);
                }
            }
            Some(Err(_)) => {
                node.error = true;
                progress.errors.fetch_add(1, Ordering::Relaxed);
            }
            None => {}
        }

        let id = tree.push(node);
        if let Some(directory) = entry.directory_id {
            let index = directory.index();
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
    tree.finish(options.size_metric);
    Ok(tree)
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use dua_core::Metadata;
    use std::os::unix::fs::MetadataExt;
    use std::{io, path::Path};

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

    /// `st_blocks` in 512-byte units, unless it is implausibly larger than the
    /// file (seen on NTFS mounts), in which case the length rounded up to whole
    /// I/O blocks is used instead.
    pub fn allocated_size(metadata: &Metadata) -> u64 {
        let reported = metadata.blocks().saturating_mul(512);
        let io_block = metadata.blksize().max(1);
        let rounded_len = metadata.len().next_multiple_of(io_block);
        let plausible_max =
            rounded_len.saturating_add(io_block.saturating_mul(PLAUSIBLE_EXTRA_BLOCKS));
        if reported <= plausible_max {
            reported
        } else {
            rounded_len
        }
    }

    pub fn link_identity(metadata: &Metadata) -> Option<(u64, u64)> {
        (metadata.nlink() > 1).then(|| (metadata.dev(), metadata.ino()))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use dua_core::Metadata;
    use std::os::unix::fs::MetadataExt;
    use std::{io, path::Path};

    pub fn root_device(root: &Path) -> io::Result<Option<u64>> {
        Ok(Some(std::fs::symlink_metadata(root)?.dev()))
    }

    pub fn device(metadata: &Metadata) -> Option<u64> {
        Some(metadata.dev())
    }

    pub fn len(metadata: &Metadata) -> u64 {
        metadata.len()
    }

    pub fn allocated_size(metadata: &Metadata) -> u64 {
        metadata.allocated_size()
    }

    pub fn link_identity(metadata: &Metadata) -> Option<(u64, u64)> {
        (metadata.nlink() > 1).then(|| (metadata.dev(), metadata.ino()))
    }
}

#[cfg(windows)]
mod platform {
    use dua_core::Metadata;
    use std::{io, path::Path};

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

    /// Link counts are not part of directory enumeration, so every file id is tracked.
    pub fn link_identity(metadata: &Metadata) -> Option<(u64, u64)> {
        metadata.hard_link_id()
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
}
