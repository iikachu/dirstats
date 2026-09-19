// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors
//
// The arena layout (one node vector addressed by index) follows dua-cli's
// `Tree` (MIT, by Sebastian Thiel, https://github.com/Byron/dua-cli).

//! Compact in-memory directory tree.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Index of a node in a [`Tree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    /// The scanned root is always the first node.
    pub const ROOT: NodeId = NodeId(0);

    /// Position in the tree's node vector. Nodes are numbered in the order
    /// they were added, so a parent's index is always below its children's.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// What an entry is on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory; the only kind that has children.
    Directory,
    /// A symbolic link (or, from the NTFS fast path, a junction or mount
    /// point). Never followed: it has no children and counts only its own size.
    Symlink,
    /// Anything else, such as a FIFO, socket or device node.
    Other,
}

/// Which size drives sorting and treemap weights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeMetric {
    /// Bytes allocated on disk (what `du` reports).
    #[default]
    Allocated,
    /// Logical file length.
    Apparent,
}

/// One entry in a [`Tree`].
#[derive(Clone, Debug)]
pub struct Node {
    /// File name within its parent. For the root, the path it was scanned as.
    pub name: Box<OsStr>,
    /// Containing directory; `None` only for the root.
    pub parent: Option<NodeId>,
    /// What the entry is; only a [`Kind::Directory`] has children.
    pub kind: Kind,
    /// Logical length. For directories, includes all descendants once the tree is finished.
    pub apparent_size: u64,
    /// Allocated bytes. For directories, includes all descendants once the tree is finished.
    pub allocated_size: u64,
    /// Non-directory entries (files, symlinks and others, duplicate hard
    /// links included) at or below this node; a non-directory counts itself.
    pub file_count: u64,
    /// Directories at or below this node, not counting itself.
    pub dir_count: u64,
    /// Last modification. For directories, the newest of its own and
    /// anything below once the tree is finished; `None` when unknown.
    pub modified: Option<std::time::SystemTime>,
    /// An additional hard link to data already counted elsewhere; contributes no size.
    pub duplicate_link: bool,
    /// Reading this entry's metadata failed; its sizes and time are then
    /// unknown and left at zero and `None`. A directory whose listing failed
    /// is not marked: that failure only shows in [`Progress::errors`].
    ///
    /// [`Progress::errors`]: crate::Progress::errors
    pub error: bool,
}

/// A scanned directory tree. Children are sorted by size, largest first.
#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Vec<Node>,
    metric: SizeMetric,
    /// Children of node `i` are `child_ids[child_offsets[i]..child_offsets[i + 1]]`.
    child_offsets: Vec<u32>,
    child_ids: Vec<NodeId>,
}

/// Builds a [`Tree`] from entries gathered by another scanner, such as a
/// filesystem-specific fast path living in its own crate.
#[derive(Debug)]
pub struct TreeBuilder(Tree);

impl Default for TreeBuilder {
    fn default() -> Self {
        Self(Tree::new())
    }
}

impl TreeBuilder {
    /// An empty builder; push the root first.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node. The first node is the root and has no parent; every
    /// other node names a parent pushed before it. Sizes, counts and times
    /// on the pushed node are the entry's own: [`TreeBuilder::finish`] adds
    /// every descendant's on top without resetting anything, so a directory
    /// carries its own size (or zero) and a `file_count` and `dir_count` of
    /// zero, and every other entry a `file_count` of one.
    ///
    /// # Panics
    /// If the parent rule above is broken, or past `u32::MAX` nodes.
    pub fn push(&mut self, node: Node) -> NodeId {
        match node.parent {
            None => assert!(self.0.is_empty(), "only the root has no parent"),
            Some(parent) => assert!(parent.index() < self.0.len(), "parent pushed after child"),
        }
        self.0.push(node)
    }

    /// Roll sizes up into directories and sort children by `metric`.
    #[must_use]
    pub fn finish(mut self, metric: SizeMetric) -> Tree {
        self.0.finish(metric);
        self.0
    }
}

impl Tree {
    pub(crate) fn new() -> Self {
        Self { nodes: Vec::new(), metric: SizeMetric::default(), child_offsets: Vec::new(), child_ids: Vec::new() }
    }

    /// Nodes must be pushed after their parent.
    pub(crate) fn push(&mut self, node: Node) -> NodeId {
        debug_assert!(node.parent.is_none_or(|p| p.index() < self.nodes.len()));
        let id = NodeId(u32::try_from(self.nodes.len()).expect("more than u32::MAX entries"));
        self.nodes.push(node);
        id
    }

    /// Roll sizes up into directories and build sorted child lists.
    pub(crate) fn finish(&mut self, metric: SizeMetric) {
        self.metric = metric;

        // Parents precede children, so one reverse pass accumulates every subtree.
        for i in (1..self.nodes.len()).rev() {
            let node = &self.nodes[i];
            let Some(parent) = node.parent else { continue };
            let (apparent, allocated) = if node.duplicate_link { (0, 0) } else { (node.apparent_size, node.allocated_size) };
            let files = node.file_count;
            let dirs = node.dir_count + u64::from(node.kind == Kind::Directory);
            let modified = node.modified;
            let parent = &mut self.nodes[parent.index()];
            parent.apparent_size += apparent;
            parent.allocated_size += allocated;
            parent.file_count += files;
            parent.dir_count += dirs;
            parent.modified = match (parent.modified, modified) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
        }

        let mut offsets = vec![0u32; self.nodes.len() + 1];
        for node in &self.nodes {
            if let Some(parent) = node.parent {
                offsets[parent.index() + 1] += 1;
            }
        }
        for i in 1..offsets.len() {
            offsets[i] += offsets[i - 1];
        }
        let mut cursor = offsets.clone();
        let mut ids = vec![NodeId(0); self.nodes.len().saturating_sub(1)];
        for (i, node) in self.nodes.iter().enumerate() {
            if let Some(parent) = node.parent {
                let slot = &mut cursor[parent.index()];
                ids[*slot as usize] = NodeId(i as u32);
                *slot += 1;
            }
        }
        self.child_offsets = offsets;
        self.child_ids = ids;

        for i in 0..self.nodes.len() {
            let range = self.child_offsets[i] as usize..self.child_offsets[i + 1] as usize;
            let mut children = std::mem::take(&mut self.child_ids);
            children[range].sort_unstable_by_key(|&id| std::cmp::Reverse(self.size(id)));
            self.child_ids = children;
        }
    }

    /// Always [`NodeId::ROOT`]. A tree returned by a scan is never empty.
    #[must_use]
    pub fn root(&self) -> NodeId {
        NodeId::ROOT
    }

    /// Number of nodes, root included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Only an empty [`TreeBuilder`] finishes into an empty tree.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The metric the tree was finished with, which orders children and
    /// decides [`Tree::size`].
    #[must_use]
    pub fn metric(&self) -> SizeMetric {
        self.metric
    }

    /// # Panics
    /// If `id` does not belong to this tree.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// Every node in id order, so each parent comes before its children.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().enumerate().map(|(i, n)| (NodeId(i as u32), n))
    }

    /// Children of `id`, largest first.
    #[must_use]
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        let i = id.index();
        &self.child_ids[self.child_offsets[i] as usize..self.child_offsets[i + 1] as usize]
    }

    /// Size under the tree's [`SizeMetric`]; zero for duplicate hard links.
    #[must_use]
    pub fn size(&self, id: NodeId) -> u64 {
        let node = self.node(id);
        if node.duplicate_link {
            return 0;
        }
        match self.metric {
            SizeMetric::Allocated => node.allocated_size,
            SizeMetric::Apparent => node.apparent_size,
        }
    }

    /// Full path of `id`. The root node's name is the path that was scanned.
    #[must_use]
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut parts = Vec::new();
        let mut current = Some(id);
        while let Some(i) = current {
            let node = self.node(i);
            parts.push(&*node.name);
            current = node.parent;
        }
        let mut path = PathBuf::new();
        for part in parts.into_iter().rev() {
            path.push(Path::new(part));
        }
        path
    }
}
