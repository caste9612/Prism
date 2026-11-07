//! Enumeration of a folder tree into a [`ScanTree`] (ADR-0006).
//!
//! An [`Enumerator`] (the parallel [`WalkEnumerator`] here, the MFT reader in `prism-win`
//! from T2, fakes in tests) produces a complete snapshot of a folder and everything below it,
//! with the [`ExclusionPolicy`] already applied. The reconciler compares the snapshot with the
//! catalog. Full indexing and updates use the same path.

mod walk;

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::time::Duration;

pub use walk::{WalkEnumerator, disk_meta};

use crate::cancel::{CancelToken, Cancelled};
use crate::exclusion::ExclusionPolicy;
use crate::model::EntryMeta;

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
}

/// A file or folder found by an enumeration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanNode {
    /// Stored name (see [`crate::names`]); empty for the root of the scan.
    pub name: Box<str>,
    pub is_dir: bool,
    pub meta: EntryMeta,
    pub listing: Listing,
    children: Range<NodeId>,
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
}

/// A snapshot of a folder tree. Children of a folder are stored contiguously; the root is
/// node 0 and represents the scanned folder itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanTree {
    nodes: Vec<ScanNode>,
    pub stats: ScanStats,
}

impl ScanTree {
    pub const ROOT: NodeId = 0;

    #[must_use]
    pub fn root(&self) -> &ScanNode {
        &self.nodes[0]
    }

    /// The node `id`.
    ///
    /// # Panics
    /// If `id` is not a node of this tree (ids come from [`ScanTree::children`]).
    #[must_use]
    pub fn node(&self, id: NodeId) -> &ScanNode {
        &self.nodes[id as usize]
    }

    /// Children of `id` (empty for files and for folders that were not descended).
    #[must_use]
    pub fn children(&self, id: NodeId) -> Range<NodeId> {
        self.nodes
            .get(id as usize)
            .map_or(0..0, |n| n.children.clone())
    }

    /// Number of nodes, root included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Whether any node carries a file reference (enables move detection by reference).
    #[must_use]
    pub fn has_file_refs(&self) -> bool {
        self.nodes.iter().any(|n| n.meta.file_ref.is_some())
    }
}

/// Builds a [`ScanTree`] one folder listing at a time.
#[derive(Debug)]
pub struct ScanTreeBuilder {
    nodes: Vec<ScanNode>,
}

impl ScanTreeBuilder {
    /// Starts a tree whose root (the scanned folder) has the given metadata.
    #[must_use]
    pub fn new(root_meta: EntryMeta) -> Self {
        Self {
            nodes: vec![ScanNode {
                name: "".into(),
                is_dir: true,
                meta: root_meta,
                listing: Listing::Complete,
                children: 0..0,
            }],
        }
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
            .get(dir as usize)
            .is_some_and(|n| n.is_dir && n.children.is_empty());
        if !valid {
            debug_assert!(false, "invalid listing for node {dir}");
            return 0..0;
        }
        let start = self.next_id();
        self.nodes.extend(children.into_iter().map(|c| ScanNode {
            name: c.name.into_boxed_str(),
            is_dir: c.is_dir,
            meta: c.meta,
            listing: Listing::Complete,
            children: 0..0,
        }));
        let range = start..self.next_id();
        if let Some(node) = self.nodes.get_mut(dir as usize) {
            node.children = range.clone();
        }
        range
    }

    /// Replaces the metadata of node `id` (e.g. with values read from the folder itself).
    pub fn set_meta(&mut self, id: NodeId, meta: EntryMeta) {
        if let Some(node) = self.nodes.get_mut(id as usize) {
            node.meta = meta;
        }
    }

    /// Marks folder `dir` as not listed: its catalog content is left as it is.
    pub fn mark_not_listed(&mut self, dir: NodeId) {
        if let Some(node) = self.nodes.get_mut(dir as usize) {
            node.listing = Listing::NotListed;
        }
    }

    /// Marks folder `dir` as not completely read.
    pub fn mark_unreadable(&mut self, dir: NodeId) {
        if let Some(node) = self.nodes.get_mut(dir as usize) {
            node.listing = Listing::Unreadable;
        }
    }

    /// The node `id`, if it exists.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&ScanNode> {
        self.nodes.get(id as usize)
    }

    /// Number of nodes so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    fn next_id(&self) -> NodeId {
        NodeId::try_from(self.nodes.len()).unwrap_or(NodeId::MAX)
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
        assert_eq!(&*t.node(1).name, "a");
        assert_eq!(t.node(1).listing, Listing::Unreadable);
        assert_eq!(t.children(1), 3..4);
        assert_eq!(t.children(2), 0..0);
        assert!(!t.has_file_refs());
    }
}
