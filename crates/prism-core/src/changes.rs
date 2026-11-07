//! Applying a set of changed files — as reported by a change journal — to the catalog
//! (ADR-0018). Platform-independent: `prism-win` turns USN records into [`FileChange`]s and
//! provides the current state of files through a [`ChangeSource`].
//!
//! For every folder touched by the changes (old and new parents of the changed files), a
//! *partial* snapshot is built (`Listing::Partial`): the current state of the changed files
//! that are now there, and the catalog entries those files had in that folder, so that the
//! reconciler compares only what changed, however large the folder (REVIEW L4); the folder's
//! own metadata is read again (its modification time changes without a journal record of its
//! own). Sub-folders keep their content (`Listing::NotListed`), except folders that are new to
//! the catalog, whose changed children are listed below them. All snapshots go through one
//! run of the reconciler, ordered by depth on the disk now, so the result equals a full
//! indexing of the same state (F05-AC3) and moves keep their entries (F05-AC4). File
//! references are looked up in a map of the volume (there is no SQL index on them, ADR-0017),
//! loaded once per round and handed to the reconciler.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::AtomicU64;

use rusqlite::Connection;

use crate::cancel::CancelToken;
use crate::exclusion::{ChildCheck, ExclusionPolicy, RuleCursor};
use crate::model::{EntryId, EntryMeta, VolumeId};
use crate::reconcile::{self, ReconcileError, ReconcileStats};
use crate::scan::{NodeId, ScanChild, ScanTree, ScanTreeBuilder};
use crate::store::{self, FileRefMap, Store, StoreError};

/// One name of a changed file, with the catalog metadata for that name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedName {
    /// File reference of the folder that contains this name.
    pub parent_ref: u64,
    /// Stored name (see [`crate::names`]).
    pub name: String,
    pub meta: EntryMeta,
}

/// The current state of a changed file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub is_dir: bool,
    /// Every name of the file (several for hard links).
    pub names: Vec<ChangedName>,
}

/// A file touched by the changes, with its state now (`None` if it no longer exists).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub file_ref: u64,
    /// Whether the file was created within these changes (as opposed to arriving by rename).
    pub created: bool,
    /// Parent folder references named by the journal records of this file (a folder whose
    /// content changed, even if the file is no longer there).
    pub parents_seen: Vec<u64>,
    pub state: Option<ChangedFile>,
}

/// Reads the current state of a file (the folders whose content changed).
pub trait ChangeSource {
    /// # Errors
    /// A description of the failure.
    fn current(&self, file_ref: u64) -> Result<Option<ChangedFile>, String>;
}

/// What happened to the changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied(ReconcileStats),
    /// The changes cannot be applied incrementally (for instance a folder with content moved
    /// into the indexed area from an excluded one): reconcile the whole volume.
    FullReconcileNeeded(String),
}

/// Errors while applying changes.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error(transparent)]
    Reconcile(#[from] ReconcileError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("cannot read the current state of a file: {0}")]
    Source(String),
}

const MAX_DEPTH: usize = 10_000;

/// Applies `changes` to the catalog of `volume` (see the module documentation).
///
/// # Errors
/// [`ApplyError`]; on error the catalog may be partially updated (the caller marks the volume
/// for reindexing, as for any reconciliation).
pub fn apply_changes(
    store: &Store,
    volume: VolumeId,
    policy: &ExclusionPolicy,
    changes: &[FileChange],
    source: &dyn ChangeSource,
    cancel: &CancelToken,
    progress: &AtomicU64,
) -> Result<ApplyOutcome, ApplyError> {
    if changes.is_empty() {
        return Ok(ApplyOutcome::Applied(ReconcileStats::default()));
    }
    let started = std::time::Instant::now();
    let (plan, map, lookups) = store.read(|conn| {
        let planner = Planner::new(conn, volume, policy, changes, source)?;
        let lookups = started.elapsed();
        let plan = planner.plan();
        Ok((plan, planner.map, lookups))
    })?;
    let planned = started.elapsed();
    let targets = match plan? {
        Planned::Targets(t) => t,
        Planned::Full(reason) => return Ok(ApplyOutcome::FullReconcileNeeded(reason)),
    };
    let refs: Vec<(EntryId, &ScanTree, &[EntryId])> = targets
        .iter()
        .map(|(id, tree, candidates)| (*id, tree, candidates.as_slice()))
        .collect();
    let stats = reconcile::reconcile_changes(store, volume, &refs, map, cancel, progress)?;
    tracing::debug!(
        changes = changes.len(),
        targets = refs.len(),
        nodes = refs.iter().map(|(_, t, _)| t.len()).sum::<usize>(),
        lookups_ms = lookups.as_millis(),
        plan_ms = (planned - lookups).as_millis(),
        reconcile_ms = (started.elapsed() - planned).as_millis(),
        "journal changes applied"
    );
    Ok(ApplyOutcome::Applied(stats))
}

enum Planned {
    /// Folders to reconcile, with their partial snapshot and the catalog children whose file
    /// changed.
    Targets(Vec<(EntryId, ScanTree, Vec<EntryId>)>),
    Full(String),
}

/// A new folder whose changed children are listed below it, with its exclusion cursor.
type Nested = Option<(u64, RuleCursor)>;

struct Planner<'a> {
    conn: &'a Connection,
    policy: &'a ExclusionPolicy,
    source: &'a dyn ChangeSource,
    map: FileRefMap,
    changed: HashMap<u64, &'a FileChange>,
}

impl<'a> Planner<'a> {
    fn new(
        conn: &'a Connection,
        volume: VolumeId,
        policy: &'a ExclusionPolicy,
        changes: &'a [FileChange],
        source: &'a dyn ChangeSource,
    ) -> Result<Self, StoreError> {
        Ok(Self {
            conn,
            policy,
            source,
            map: store::file_ref_map(conn, volume)?,
            changed: changes.iter().map(|c| (c.file_ref, c)).collect(),
        })
    }

    /// The catalog folder with this file reference, if any.
    fn catalog_dir(&self, file_ref: u64) -> Result<Option<EntryId>, StoreError> {
        for (_, id) in self.map.get(file_ref) {
            if store::entry(self.conn, *id)?.is_some_and(|e| e.is_dir) {
                return Ok(Some(*id));
            }
        }
        Ok(None)
    }

    /// Whether `file_ref` is a folder created within these changes and not yet in the catalog.
    fn is_new_dir(&self, file_ref: u64) -> bool {
        self.map.get(file_ref).is_empty()
            && self
                .changed
                .get(&file_ref)
                .and_then(|c| c.state.as_ref())
                .is_some_and(|s| s.is_dir)
    }

    fn plan(&self) -> Result<Planned, ApplyError> {
        // Every volume indexed through the MFT carries file references; without them the
        // journal cannot be applied.
        if self.map.is_empty() {
            return Ok(Planned::Full("the catalog has no file references".into()));
        }
        let mut affected: BTreeSet<EntryId> = BTreeSet::new();
        // Catalog children whose file changed, by folder: compared even if not listed.
        let mut candidates: HashMap<EntryId, Vec<EntryId>> = HashMap::new();
        for change in self.changed.values() {
            // Folders named by the journal records.
            for parent in &change.parents_seen {
                if let Some(dir) = self.catalog_dir(*parent)? {
                    affected.insert(dir);
                }
            }
            // Old positions.
            for (_, id) in self.map.get(change.file_ref) {
                if let Some(parent) = store::entry(self.conn, *id)?.and_then(|e| e.parent) {
                    affected.insert(parent);
                    candidates.entry(parent).or_default().push(*id);
                }
            }
            // New positions.
            let Some(state) = &change.state else { continue };
            for name in &state.names {
                if let Some(dir) = self.catalog_dir(name.parent_ref)? {
                    affected.insert(dir);
                }
            }
            // A folder arriving by rename from outside the catalog brings content that the
            // changes do not describe.
            if state.is_dir
                && !change.created
                && self.map.get(change.file_ref).is_empty()
                && self.reaches_catalog(change.file_ref)?
            {
                return Ok(Planned::Full(format!(
                    "folder {:#x} moved into the indexed area with its content",
                    change.file_ref
                )));
            }
        }

        let mut targets: Vec<(usize, EntryId, ScanTree, Vec<EntryId>)> = Vec::new();
        for dir in affected {
            let Some(entry) = store::entry(self.conn, dir)? else {
                continue;
            };
            let Some(dir_ref) = entry.meta.file_ref else {
                return Ok(Planned::Full("a folder has no file reference".into()));
            };
            // The folder's own metadata, read now; a folder that no longer exists is removed by
            // its parent's snapshot.
            let current = self.source.current(dir_ref).map_err(ApplyError::Source)?;
            let Some(current) = current.filter(|c| c.is_dir) else {
                continue;
            };
            let meta = current.names.first().map_or(entry.meta, |n| n.meta);
            // Where the folder is now decides its exclusion rules and its order: a folder
            // moved into an excluded area is not a target (its parent's snapshot removes it).
            let Some(components) = self.real_path(dir_ref)? else {
                return Ok(Planned::Full("cannot place a changed folder".into()));
            };
            let Some(cursor) = self.policy.cursor_for(&components) else {
                continue;
            };
            let tree = self.snapshot(dir_ref, meta, &cursor);
            let olds = candidates.remove(&dir).unwrap_or_default();
            targets.push((components.len(), dir, tree, olds));
        }
        targets.sort_by_key(|(depth, id, _, _)| (*depth, *id));
        Ok(Planned::Targets(
            targets
                .into_iter()
                .map(|(_, id, t, c)| (id, t, c))
                .collect(),
        ))
    }

    /// Whether a folder outside the catalog is placed (directly or through new folders) under
    /// a catalog folder.
    fn reaches_catalog(&self, file_ref: u64) -> Result<bool, StoreError> {
        let mut current = file_ref;
        for _ in 0..MAX_DEPTH {
            let Some(state) = self.changed.get(&current).and_then(|c| c.state.as_ref()) else {
                return Ok(false);
            };
            let Some(name) = state.names.first() else {
                return Ok(false);
            };
            if self.catalog_dir(name.parent_ref)?.is_some() {
                return Ok(true);
            }
            current = name.parent_ref;
        }
        Ok(false)
    }

    /// Path of a folder on the disk now, relative to the volume root (stored names): from the
    /// current state for changed folders, from the catalog for the others. `None` if the chain
    /// cannot be followed to the root.
    fn real_path(&self, file_ref: u64) -> Result<Option<Vec<String>>, StoreError> {
        let mut parts = Vec::new();
        let mut current = file_ref;
        loop {
            if parts.len() > MAX_DEPTH {
                return Ok(None);
            }
            let (name, parent_ref) = match self.changed.get(&current) {
                Some(change) => match change.state.as_ref().and_then(|s| s.names.first()) {
                    Some(n) => (n.name.clone(), Some(n.parent_ref)),
                    None => return Ok(None),
                },
                None => {
                    let Some(id) = self.catalog_dir(current)? else {
                        return Ok(None);
                    };
                    let Some(entry) = store::entry(self.conn, id)? else {
                        return Ok(None);
                    };
                    let parent_ref = match entry.parent {
                        None => None,
                        Some(p) => {
                            match store::entry(self.conn, p)?.and_then(|e| e.meta.file_ref) {
                                Some(r) => Some(r),
                                None => return Ok(None),
                            }
                        }
                    };
                    (entry.name, parent_ref)
                }
            };
            match parent_ref {
                // The volume root: its parent is itself (NTFS) or it has none (catalog).
                Some(p) if p != current => {
                    parts.push(name);
                    current = p;
                }
                _ => break,
            }
        }
        parts.reverse();
        Ok(Some(parts))
    }

    /// The partial snapshot of the catalog folder with reference `dir_ref`: the changed files
    /// that are now in it (see the module documentation).
    fn snapshot(&self, dir_ref: u64, meta: EntryMeta, cursor: &RuleCursor) -> ScanTree {
        let mut builder = ScanTreeBuilder::new(meta);
        builder.mark_partial(ScanTree::ROOT);
        let mut children: Vec<(ScanChild, Nested)> = Vec::new();
        self.changed_children(dir_ref, cursor, &mut children);
        let mut seen: HashSet<u64> = HashSet::new();
        self.add_listing(&mut builder, ScanTree::ROOT, children, &mut seen);
        builder.finish(crate::scan::ScanStats::default())
    }

    /// The changed files that are now in folder `parent_ref`, excluded names left out. New
    /// folders carry their own reference so that their content is listed too.
    fn changed_children(
        &self,
        parent_ref: u64,
        cursor: &RuleCursor,
        out: &mut Vec<(ScanChild, Nested)>,
    ) {
        let mut refs: Vec<&u64> = self.changed.keys().collect();
        refs.sort_unstable();
        for r in refs {
            let Some(state) = self.changed.get(r).and_then(|c| c.state.as_ref()) else {
                continue;
            };
            for name in state.names.iter().filter(|n| n.parent_ref == parent_ref) {
                if let ChildCheck::Included(next) =
                    self.policy.child(cursor, &name.name, state.is_dir)
                {
                    let nested = (state.is_dir && self.is_new_dir(*r)).then_some((*r, next));
                    out.push((
                        ScanChild {
                            name: name.name.clone(),
                            is_dir: state.is_dir,
                            meta: name.meta,
                        },
                        nested,
                    ));
                }
            }
        }
    }

    fn add_listing(
        &self,
        builder: &mut ScanTreeBuilder,
        node: NodeId,
        children: Vec<(ScanChild, Nested)>,
        seen: &mut HashSet<u64>,
    ) {
        let (nodes, nested): (Vec<ScanChild>, Vec<(bool, Nested)>) = children
            .into_iter()
            .map(|(c, n)| {
                let is_dir = c.is_dir;
                (c, (is_dir, n))
            })
            .unzip();
        let range = builder.add_listing(node, nodes);
        for (id, (is_dir, nested)) in range.zip(nested) {
            match nested {
                // A new folder: list its changed children below it (once, against cycles).
                Some((r, cursor)) if seen.insert(r) => {
                    let mut grand = Vec::new();
                    self.changed_children(r, &cursor, &mut grand);
                    self.add_listing(builder, id, grand, seen);
                }
                _ if is_dir => builder.mark_not_listed(id),
                _ => {}
            }
        }
    }
}
