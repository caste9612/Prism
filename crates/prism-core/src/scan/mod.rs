//! Enumeration of a folder tree into a [`ScanTree`] (ADR-0006).
//!
//! An [`Enumerator`] (the parallel [`WalkEnumerator`] here, the MFT reader in `prism-win`
//! from T2, fakes in tests) produces a complete snapshot of a folder and everything below it,
//! with the [`ExclusionPolicy`] already applied. The reconciler compares the snapshot with the
//! catalog. Full indexing and updates use the same path.

mod walk;

use std::collections::VecDeque;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::time::Duration;

pub use walk::{WalkEnumerator, disk_meta, open_meta};

use crate::cancel::{CancelToken, Cancelled};
use crate::exclusion::ExclusionPolicy;
use crate::model::{EntryMeta, FileTime};

/// Index of a node in a [`ScanTree`].
pub type NodeId = u32;

/// Whether the content of a folder was read completely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Listing {
    /// All children are in the tree (also used for folders that are not descended, such as
    /// junctions and symbolic links, which have no children in the catalog).
    #[default]
    Complete,
    /// The folder could not be read, or only partly (access denied, disappeared, I/O error).
    /// The reconciler keeps the catalog children it does not see instead of deleting them.
    Unreadable,
    /// The content of the folder is not part of this snapshot (partial updates from a change
    /// journal, ADR-0018): the reconciler leaves its catalog children untouched.
    NotListed,
    /// Only the children that changed are listed (a folder touched by a change journal): the
    /// reconciler compares them, and the catalog children it is told about, without reading
    /// the whole folder (REVIEW L4). The other catalog children stay as they are.
    Partial,
}

/// A file or folder found by an enumeration, as read from a [`ScanTree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanNode<'a> {
    /// Stored name (see [`crate::names`]); empty for the root of the scan.
    pub name: &'a str,
    pub is_dir: bool,
    pub meta: EntryMeta,
    pub listing: Listing,
}

/// "No value" for the optional numbers of [`Node`]: a size, a reference or a time never takes
/// these values.
const NO_U64: u64 = u64::MAX;
const NO_TIME: i64 = i64::MIN;
const DIR: u8 = 1;
const LISTING_SHIFT: u8 = 1;

/// How a [`ScanTree`] keeps a node: 64 bytes, with the name in the tree's single buffer, so a
/// snapshot of millions of entries makes no allocation per entry (REVIEW L1).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Node {
    size: u64,
    alloc: u64,
    created: i64,
    modified: i64,
    file_ref: u64,
    /// Byte range of the name in the names buffer.
    name_start: u32,
    name_len: u32,
    children_start: NodeId,
    children_end: NodeId,
    attrs: u32,
    /// [`DIR`] and the [`Listing`] above [`LISTING_SHIFT`].
    flags: u8,
}

impl Node {
    fn new(name: Range<u32>, is_dir: bool, meta: &EntryMeta) -> Self {
        let mut n = Self {
            size: 0,
            alloc: NO_U64,
            created: NO_TIME,
            modified: NO_TIME,
            file_ref: NO_U64,
            name_start: name.start,
            name_len: name.end - name.start,
            children_start: 0,
            children_end: 0,
            attrs: 0,
            flags: if is_dir { DIR } else { 0 },
        };
        n.set_meta(meta);
        n
    }

    fn set_meta(&mut self, m: &EntryMeta) {
        self.size = m.size;
        self.alloc = m.alloc_size.unwrap_or(NO_U64);
        self.created = m.created.map_or(NO_TIME, |t| t.0);
        self.modified = m.modified.map_or(NO_TIME, |t| t.0);
        self.file_ref = m.file_ref.unwrap_or(NO_U64);
        self.attrs = m.attrs;
    }

    fn meta(&self) -> EntryMeta {
        let time = |t: i64| (t != NO_TIME).then_some(FileTime(t));
        EntryMeta {
            size: self.size,
            alloc_size: (self.alloc != NO_U64).then_some(self.alloc),
            created: time(self.created),
            modified: time(self.modified),
            attrs: self.attrs,
            file_ref: (self.file_ref != NO_U64).then_some(self.file_ref),
        }
    }

    fn is_dir(&self) -> bool {
        self.flags & DIR != 0
    }

    fn listing(&self) -> Listing {
        match self.flags >> LISTING_SHIFT {
            0 => Listing::Complete,
            1 => Listing::Unreadable,
            2 => Listing::NotListed,
            _ => Listing::Partial,
        }
    }

    fn set_listing(&mut self, listing: Listing) {
        let code = match listing {
            Listing::Complete => 0,
            Listing::Unreadable => 1,
            Listing::NotListed => 2,
            Listing::Partial => 3,
        };
        self.flags = (self.flags & DIR) | (code << LISTING_SHIFT);
    }

    fn children(&self) -> Range<NodeId> {
        self.children_start..self.children_end
    }
}

/// Nodes and the buffer of their names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Nodes {
    nodes: Vec<Node>,
    names: String,
}

impl Nodes {
    fn view(&self, n: &Node) -> ScanNode<'_> {
        let start = n.name_start as usize;
        ScanNode {
            name: self
                .names
                .get(start..start + n.name_len as usize)
                .unwrap_or_default(),
            is_dir: n.is_dir(),
            meta: n.meta(),
            listing: n.listing(),
        }
    }

    fn push(&mut self, name: &str, is_dir: bool, meta: &EntryMeta) {
        let start = u32::try_from(self.names.len()).unwrap_or(u32::MAX);
        self.names.push_str(name);
        let end = u32::try_from(self.names.len()).unwrap_or(u32::MAX);
        self.nodes.push(Node::new(start..end, is_dir, meta));
    }
}

/// One child reported for a folder, as given to [`ScanTreeBuilder::add_listing`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanChild {
    pub name: String,
    pub is_dir: bool,
    pub meta: EntryMeta,
}

/// Counters of an enumeration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub files: u64,
    pub dirs: u64,
    /// Folders whose content could not be read completely.
    pub unreadable_dirs: u64,
    /// Files and folders skipped by the exclusion policy.
    pub excluded: u64,
    /// The first few errors, for logs and reports.
    pub errors: Vec<String>,
    pub elapsed: Duration,
}

impl ScanStats {
    const MAX_ERRORS: usize = 20;

    /// Records an error message, keeping only the first few.
    pub fn push_error(&mut self, message: String) {
        if self.errors.len() < Self::MAX_ERRORS {
            self.errors.push(message);
        }
    }

    /// Adds the counters and errors of another enumeration (the time is not added).
    pub fn add(&mut self, other: &Self) {
        self.files += other.files;
        self.dirs += other.dirs;
        self.unreadable_dirs += other.unreadable_dirs;
        self.excluded += other.excluded;
        for e in &other.errors {
            self.push_error(e.clone());
        }
    }
}

/// A snapshot of a folder tree. Children of a folder are stored contiguously; the root is
/// node 0 and represents the scanned folder itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanTree {
    nodes: Nodes,
    pub stats: ScanStats,
}

impl ScanTree {
    pub const ROOT: NodeId = 0;

    #[must_use]
    pub fn root(&self) -> ScanNode<'_> {
        self.node(Self::ROOT)
    }

    /// The node `id`.
    ///
    /// # Panics
    /// If `id` is not a node of this tree (ids come from [`ScanTree::children`]).
    #[must_use]
    pub fn node(&self, id: NodeId) -> ScanNode<'_> {
        self.nodes.view(&self.nodes.nodes[id as usize])
    }

    /// Children of `id` (empty for files and for folders that were not descended).
    #[must_use]
    pub fn children(&self, id: NodeId) -> Range<NodeId> {
        self.nodes
            .nodes
            .get(id as usize)
            .map_or(0..0, Node::children)
    }

    /// Number of nodes, root included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.nodes.len() <= 1
    }

    /// Whether any node carries a file reference (enables move detection by reference).
    #[must_use]
    pub fn has_file_refs(&self) -> bool {
        self.nodes.nodes.iter().any(|n| n.file_ref != NO_U64)
    }

    /// Bytes held by the snapshot (nodes and names), for measurements.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.nodes.nodes.capacity() * size_of::<Node>() + self.nodes.names.capacity()
    }
}

/// Builds a [`ScanTree`] one folder listing at a time.
#[derive(Debug)]
pub struct ScanTreeBuilder {
    nodes: Nodes,
}

impl ScanTreeBuilder {
    /// Starts a tree whose root (the scanned folder) has the given metadata.
    #[must_use]
    pub fn new(root_meta: EntryMeta) -> Self {
        Self::with_capacity(root_meta, 0, 0)
    }

    /// Like [`ScanTreeBuilder::new`], with room for `nodes` nodes and `name_bytes` bytes of
    /// names, when the enumerator knows them (the MFT reader): the snapshot is then allocated
    /// once, without the spare room of a growing buffer.
    #[must_use]
    pub fn with_capacity(root_meta: EntryMeta, nodes: usize, name_bytes: usize) -> Self {
        let mut n = Nodes {
            nodes: Vec::with_capacity(nodes.max(1)),
            names: String::with_capacity(name_bytes),
        };
        n.push("", true, &root_meta);
        Self { nodes: n }
    }

    /// Sets the children of folder `dir` and returns their node ids. A folder is listed at
    /// most once; listing a file or listing twice is ignored and returns an empty range.
    pub fn add_listing(
        &mut self,
        dir: NodeId,
        children: impl IntoIterator<Item = ScanChild>,
    ) -> Range<NodeId> {
        let valid = self
            .nodes
            .nodes
            .get(dir as usize)
            .is_some_and(|n| n.is_dir() && n.children().is_empty());
        if !valid {
            debug_assert!(false, "invalid listing for node {dir}");
            return 0..0;
        }
        let start = self.next_id();
        for c in children {
            self.nodes.push(&c.name, c.is_dir, &c.meta);
        }
        let range = start..self.next_id();
        if let Some(node) = self.nodes.nodes.get_mut(dir as usize) {
            node.children_start = range.start;
            node.children_end = range.end;
        }
        range
    }

    /// Replaces the metadata of node `id` (e.g. with values read from the folder itself).
    pub fn set_meta(&mut self, id: NodeId, meta: EntryMeta) {
        if let Some(node) = self.nodes.nodes.get_mut(id as usize) {
            node.set_meta(&meta);
        }
    }

    /// Marks folder `dir` as not listed: its catalog content is left as it is.
    pub fn mark_not_listed(&mut self, dir: NodeId) {
        if let Some(node) = self.nodes.nodes.get_mut(dir as usize) {
            node.set_listing(Listing::NotListed);
        }
    }

    /// Marks folder `dir` as listed only in part: its children are the ones that changed.
    pub fn mark_partial(&mut self, dir: NodeId) {
        if let Some(node) = self.nodes.nodes.get_mut(dir as usize) {
            node.set_listing(Listing::Partial);
        }
    }

    /// Marks folder `dir` as not completely read.
    pub fn mark_unreadable(&mut self, dir: NodeId) {
        if let Some(node) = self.nodes.nodes.get_mut(dir as usize) {
            node.set_listing(Listing::Unreadable);
        }
    }

    /// Copies what `tree` holds below its node `from` (the children, their listing states and
    /// everything below them) as the content of folder `dir`, which takes the listing state of
    /// `from`. `dir` must not be listed yet.
    pub fn graft(&mut self, dir: NodeId, tree: &ScanTree, from: NodeId) {
        let mut queue = VecDeque::from([(from, dir)]);
        while let Some((src, dst)) = queue.pop_front() {
            let kids = tree.children(src);
            let range = self.add_listing(
                dst,
                kids.clone().map(|k| {
                    let n = tree.node(k);
                    ScanChild {
                        name: n.name.to_owned(),
                        is_dir: n.is_dir,
                        meta: n.meta,
                    }
                }),
            );
            let listing = tree.node(src).listing;
            if let Some(node) = self.nodes.nodes.get_mut(dst as usize) {
                node.set_listing(listing);
            }
            for (k, d) in kids.zip(range) {
                if tree.node(k).is_dir {
                    queue.push_back((k, d));
                }
            }
        }
    }

    /// The node `id`, if it exists.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<ScanNode<'_>> {
        self.nodes
            .nodes
            .get(id as usize)
            .map(|n| self.nodes.view(n))
    }

    /// Number of nodes so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.nodes.len() <= 1
    }

    fn next_id(&self) -> NodeId {
        NodeId::try_from(self.nodes.nodes.len()).unwrap_or(NodeId::MAX)
    }

    #[must_use]
    pub fn finish(self, stats: ScanStats) -> ScanTree {
        ScanTree {
            nodes: self.nodes,
            stats,
        }
    }
}

/// What to enumerate.
pub struct ScanRequest<'a> {
    /// Absolute path of the folder to enumerate (the root of the volume, or a folder in it).
    pub root: &'a Path,
    /// Components of `root` relative to the volume root (empty for the volume root), used
    /// to apply path-based exclusion rules.
    pub relative: &'a [String],
    pub policy: &'a ExclusionPolicy,
    pub cancel: &'a CancelToken,
    /// Incremented with the number of entries found, for progress reporting.
    pub progress: &'a AtomicU64,
}

/// Errors of an enumeration.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error(transparent)]
    Cancelled(#[from] Cancelled),
    #[error("cannot read {path}: {source}")]
    RootUnreadable {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0} is not a folder")]
    RootNotADirectory(PathBuf),
    #[error("{0} is excluded by the exclusion rules")]
    RootExcluded(PathBuf),
    #[error("this enumerator cannot read {path}: {reason}")]
    Unsupported { path: PathBuf, reason: String },
    #[error("internal scan error: {0}")]
    Internal(String),
}

/// Produces a [`ScanTree`] for a folder.
pub trait Enumerator: Send + Sync {
    /// Enumerates `request.root` and everything below it.
    ///
    /// # Errors
    /// [`ScanError`] if the root cannot be read or the scan was cancelled. Errors below the
    /// root do not fail the scan: the affected folders are marked [`Listing::Unreadable`].
    fn scan(&self, request: &ScanRequest<'_>) -> Result<ScanTree, ScanError>;

    /// Lists `request.root` alone, for the refresh of one folder (ADR-0007): its children,
    /// with the folders among them that [`Enumerator::scan`] would descend marked
    /// [`Listing::NotListed`]. An enumerator may give more, as the default does (the whole
    /// subtree): the refresh then takes what it needs.
    ///
    /// # Errors
    /// As [`Enumerator::scan`].
    fn list(&self, request: &ScanRequest<'_>) -> Result<ScanTree, ScanError> {
        self.scan(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(name: &str, is_dir: bool) -> ScanChild {
        ScanChild {
            name: name.into(),
            is_dir,
            meta: EntryMeta::default(),
        }
    }

    // REVIEW L1: the compact nodes give back exactly what the enumerator reported, with and
    // without the optional values, and the listing state does not touch the folder flag.
    #[test]
    fn compact_nodes_keep_every_value() {
        let full = EntryMeta {
            size: 123_456_789,
            alloc_size: Some(126_976),
            created: Some(FileTime(-5)),
            modified: Some(FileTime(133_000_000_000_000_000)),
            attrs: 0x20,
            file_ref: Some(0x0001_0000_0000_002A),
        };
        let empty = EntryMeta::default();
        let mut b = ScanTreeBuilder::with_capacity(full, 4, 8);
        let ids = b.add_listing(
            ScanTree::ROOT,
            [
                ScanChild {
                    name: "è più".into(),
                    is_dir: true,
                    meta: empty,
                },
                ScanChild {
                    name: "b.txt".into(),
                    is_dir: false,
                    meta: full,
                },
            ],
        );
        b.mark_unreadable(ids.start);
        let t = b.finish(ScanStats::default());
        assert_eq!(t.root().meta, full);
        assert_eq!(t.root().name, "");
        let dir = t.node(ids.start);
        assert_eq!(
            (dir.name, dir.is_dir, dir.meta, dir.listing),
            ("è più", true, empty, Listing::Unreadable)
        );
        let file = t.node(ids.start + 1);
        assert_eq!(
            (file.name, file.is_dir, file.meta, file.listing),
            ("b.txt", false, full, Listing::Complete)
        );
        assert!(t.has_file_refs());
        assert!(t.memory() >= 3 * size_of::<Node>());
        assert_eq!(size_of::<Node>(), 64);
    }

    // A subtree copied into another tree keeps its names, metadata, folders and listing states.
    #[test]
    fn grafted_subtrees_are_copied_whole() {
        let mut src = ScanTreeBuilder::new(EntryMeta::default());
        let top = src.add_listing(ScanTree::ROOT, [child("d", true), child("f", false)]);
        let below = src.add_listing(top.start, [child("e", true), child("g", false)]);
        src.add_listing(below.start, [child("h", false)]);
        src.mark_unreadable(below.start);
        let src = src.finish(ScanStats::default());

        let mut b = ScanTreeBuilder::new(EntryMeta::default());
        let mine = b.add_listing(ScanTree::ROOT, [child("x", true), child("y", true)]);
        b.graft(mine.start, &src, top.start);
        b.mark_not_listed(mine.start + 1);
        let t = b.finish(ScanStats::default());
        let x = mine.start;
        let names = |id: NodeId| -> Vec<&str> { t.children(id).map(|k| t.node(k).name).collect() };
        assert_eq!(names(x), ["e", "g"]);
        let e = t.children(x).start;
        assert_eq!(t.node(e).listing, Listing::Unreadable);
        assert_eq!(names(e), ["h"]);
        assert_eq!(t.node(x).listing, Listing::Complete);
        assert_eq!(t.node(x + 1).listing, Listing::NotListed);
        assert_eq!(t.len(), 6);
    }

    #[test]
    fn builder_groups_children() {
        let mut b = ScanTreeBuilder::new(EntryMeta::default());
        let top = b.add_listing(ScanTree::ROOT, [child("a", true), child("f", false)]);
        assert_eq!(top, 1..3);
        let below = b.add_listing(1, [child("x", false)]);
        assert_eq!(below, 3..4);
        b.mark_unreadable(1);
        let t = b.finish(ScanStats::default());
        assert_eq!(t.len(), 4);
        assert_eq!(t.children(ScanTree::ROOT), 1..3);
        assert_eq!(t.node(1).name, "a");
        assert_eq!(t.node(1).listing, Listing::Unreadable);
        assert_eq!(t.children(1), 3..4);
        assert_eq!(t.children(2), 0..0);
        assert!(!t.has_file_refs());
    }
}
