//! The single reconciler (ADR-0006): turns a [`ScanTree`] into catalog operations.
//!
//! Full indexing and every kind of update go through [`reconcile`]: a full indexing
//! reconciles the volume root against a scan of the whole volume, an update reconciles a
//! folder against a scan of that folder. The same disk state therefore always produces the
//! same catalog (F05-AC3).
//!
//! # Algorithm
//!
//! Folders are visited breadth-first. For each folder, the scanned children are matched with
//! the catalog children:
//!
//! 1. by **file reference** (when both sides have one): a renamed file keeps its entry;
//! 2. by **exact name**, if the references do not contradict each other;
//! 3. by file reference **anywhere in the volume**: the entry is moved here with its whole
//!    subtree, keeping its id and everything attached to it (F05-AC4).
//!
//! Unmatched scanned children are inserted. Catalog children that are renamed, or whose name
//! is taken by another entry, are first moved to a temporary name (names are unique per
//! folder). Unmatched catalog children are deleted at the end, unless a later folder claimed
//! them by reference. Folders whose content could not be read keep the catalog children that
//! were not seen. Finally the aggregates of every touched folder and its ancestors are
//! recomputed.
//!
//! Breadth-first order guarantees that when an entry is moved into a folder, every ancestor of
//! that folder is already in its final place, so a move can never create a cycle.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use crate::cancel::{CancelToken, Cancelled};
use crate::model::{EntryId, EntryRecord, VolumeId};
use crate::scan::{Listing, NodeId, ScanTree};
use crate::store::{self, Store, StoreError, WriteOp, WriteSession};

/// First character of the temporary names given to entries while a folder is reconciled.
/// File names cannot contain control characters, so these never collide with real names.
pub const PARKED_PREFIX: char = '\u{1}';

const BATCH_SIZE: usize = 10_000;

/// What the scan tree corresponds to in the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileTarget {
    /// The root of the volume (full indexing). The root entry is created if missing.
    VolumeRoot,
    /// An existing folder of the volume (partial update).
    Folder(EntryId),
}

/// Counters of a reconciliation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReconcileStats {
    pub inserted: u64,
    pub updated: u64,
    /// Renamed or moved entries (subtrees count once).
    pub moved: u64,
    /// Deleted entries (subtrees count once).
    pub deleted: u64,
    pub unchanged: u64,
}

/// Errors of a reconciliation.
#[derive(Debug, thiserror::Error)]
pub enum ReconcileError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Cancelled(#[from] Cancelled),
    #[error("folder {0} is not in the catalog of this volume")]
    TargetNotFound(EntryId),
}

/// Applies `tree` to the catalog of `volume` at `target`.
///
/// Returns after every operation has been committed. On error or cancellation the catalog
/// may be partially updated (but structurally consistent); the caller marks the volume as
/// needing a new indexing.
///
/// # Errors
/// [`ReconcileError`] on store errors, cancellation, or a missing target.
pub fn reconcile(
    store: &Store,
    volume: VolumeId,
    target: ReconcileTarget,
    tree: &ScanTree,
    cancel: &CancelToken,
    progress: &AtomicU64,
) -> Result<ReconcileStats, ReconcileError> {
    reconcile_many(store, volume, &[(target, tree)], cancel, progress)
}

/// Applies several snapshots in **one** reconciliation (partial updates from a change
/// journal, ADR-0018). Targets must be ordered so that a folder comes after every folder that
/// is its ancestor on the disk now; then a move between two targets keeps the entry (deletes
/// are deferred to the end of the whole run) and can never create a cycle.
///
/// # Errors
/// See [`reconcile`].
pub fn reconcile_many(
    store: &Store,
    volume: VolumeId,
    targets: &[(ReconcileTarget, &ScanTree)],
    cancel: &CancelToken,
    progress: &AtomicU64,
) -> Result<ReconcileStats, ReconcileError> {
    let mut run = Run {
        store,
        volume,
        cancel,
        progress,
        session: store.session(),
        batch: Vec::new(),
        used: IdSet::default(),
        protected: HashSet::new(),
        pending_deletes: Vec::new(),
        seeds: Vec::new(),
        lookups: false,
        ref_map: None,
        stats: ReconcileStats::default(),
    };
    let outcome = store.read(|conn| Ok(run.execute(conn, targets)))?;
    // Whatever happened, wait for the submitted batches so the caller sees a settled catalog.
    let flushed = run.session.flush();
    let stats = outcome?;
    flushed?;
    Ok(stats)
}

#[derive(Clone, Debug)]
enum Plan {
    New,
    /// Index into the existing children of the folder.
    Existing(usize),
    /// An entry found elsewhere in the volume by file reference.
    MovedIn(EntryRecord),
}

struct Run<'a> {
    store: &'a Store,
    volume: VolumeId,
    cancel: &'a CancelToken,
    progress: &'a AtomicU64,
    session: WriteSession,
    batch: Vec<WriteOp>,
    /// Catalog entries already matched or moved in this run.
    used: IdSet,
    /// The target and its ancestors: never moved by reference (outside the scanned tree).
    protected: HashSet<EntryId>,
    pending_deletes: Vec<EntryId>,
    /// Folders whose aggregates must be recomputed.
    seeds: Vec<EntryId>,
    /// Whether to look up unmatched scanned entries by file reference in the whole volume.
    lookups: bool,
    /// File reference → entries of the volume, loaded on the first lookup.
    ref_map: Option<store::FileRefMap>,
    stats: ReconcileStats,
}

impl Run<'_> {
    fn execute(
        &mut self,
        conn: &Connection,
        targets: &[(ReconcileTarget, &ScanTree)],
    ) -> Result<ReconcileStats, ReconcileError> {
        for (i, (target, tree)) in targets.iter().enumerate() {
            if i > 0 {
                // Later targets read the catalog as left by the earlier ones.
                self.submit()?;
                self.session.flush()?;
            }
            self.execute_target(conn, *target, tree)?;
        }
        let deletes = std::mem::take(&mut self.pending_deletes);
        for id in deletes {
            if !self.used.contains(id) {
                self.push(WriteOp::DeleteSubtree { id });
                self.stats.deleted += 1;
            }
        }
        let dirs = std::mem::take(&mut self.seeds);
        self.push(WriteOp::RecomputeDirStats { dirs });
        self.submit()?;
        Ok(self.stats)
    }

    fn execute_target(
        &mut self,
        conn: &Connection,
        target: ReconcileTarget,
        tree: &ScanTree,
    ) -> Result<(), ReconcileError> {
        let root_meta = tree.root().meta;
        let (root_id, root_is_new) = match target {
            ReconcileTarget::VolumeRoot => match store::root_entry(conn, self.volume)? {
                Some(root) => {
                    self.update_if_changed(&root, &root_meta);
                    (root.id, false)
                }
                None => {
                    let id = self.store.allocate_entry_ids(1);
                    self.push(WriteOp::Insert {
                        id,
                        volume: self.volume,
                        parent: None,
                        name: String::new(),
                        is_dir: true,
                        meta: root_meta,
                    });
                    self.stats.inserted += 1;
                    (id, true)
                }
            },
            ReconcileTarget::Folder(id) => {
                let folder = store::entry(conn, id)?
                    .filter(|e| e.volume == self.volume && e.is_dir)
                    .ok_or(ReconcileError::TargetNotFound(id))?;
                self.update_if_changed(&folder, &root_meta);
                (folder.id, false)
            }
        };
        if !root_is_new {
            self.protected = store::ancestors_and_self(conn, root_id)?
                .into_iter()
                .collect();
            self.lookups = tree.has_file_refs() && store::volume_has_entries(conn, self.volume)?;
        }
        // The target is not marked as used: in a multi-target run it may still have to be
        // removed by its parent's snapshot (moved into an excluded area). It cannot be claimed
        // inside its own subtree, because `protected` contains it.

        let mut queue: VecDeque<(NodeId, EntryId, bool)> = VecDeque::new();
        queue.push_back((ScanTree::ROOT, root_id, root_is_new));
        while let Some((node, dir, is_new)) = queue.pop_front() {
            self.cancel.check()?;
            self.diff_dir(conn, tree, node, dir, is_new, &mut queue)?;
        }
        Ok(())
    }

    fn update_if_changed(&mut self, existing: &EntryRecord, meta: &crate::model::EntryMeta) {
        if existing.meta == *meta {
            self.stats.unchanged += 1;
        } else {
            self.push(WriteOp::Update {
                id: existing.id,
                meta: *meta,
            });
            self.stats.updated += 1;
        }
    }

    #[allow(clippy::too_many_lines)]
    fn diff_dir(
        &mut self,
        conn: &Connection,
        tree: &ScanTree,
        node: NodeId,
        dir: EntryId,
        is_new: bool,
        queue: &mut VecDeque<(NodeId, EntryId, bool)>,
    ) -> Result<(), ReconcileError> {
        if tree.node(node).listing == Listing::NotListed {
            // Content not part of this snapshot: leave the catalog children as they are.
            return Ok(());
        }
        let kids: Vec<NodeId> = tree.children(node).collect();
        self.progress
            .fetch_add(kids.len() as u64, Ordering::Relaxed);
        let existing: Vec<EntryRecord> = if is_new {
            Vec::new()
        } else {
            store::children(conn, dir)?
                .into_iter()
                .filter(|e| !self.used.contains(e.id))
                .collect()
        };

        let mut plan: Vec<Plan> = vec![Plan::New; kids.len()];
        let mut matched = vec![false; existing.len()];
        if !existing.is_empty() {
            let by_ref: HashMap<u64, usize> = existing
                .iter()
                .enumerate()
                .filter_map(|(i, e)| e.meta.file_ref.map(|r| (r, i)))
                .collect();
            let by_name: HashMap<&str, usize> = existing
                .iter()
                .enumerate()
                .map(|(i, e)| (e.name.as_str(), i))
                .collect();
            // 1. Same file reference in the same folder (rename in place).
            for (k, &n) in kids.iter().enumerate() {
                let c = tree.node(n);
                let Some(r) = c.meta.file_ref else { continue };
                if let Some(&i) = by_ref.get(&r)
                    && !matched[i]
                    && existing[i].is_dir == c.is_dir
                {
                    matched[i] = true;
                    plan[k] = Plan::Existing(i);
                }
            }
            // 2. Same name, unless the references say they are different files.
            for (k, &n) in kids.iter().enumerate() {
                if !matches!(plan[k], Plan::New) {
                    continue;
                }
                let c = tree.node(n);
                if let Some(&i) = by_name.get(c.name)
                    && !matched[i]
                    && existing[i].is_dir == c.is_dir
                    && refs_compatible(existing[i].meta.file_ref, c.meta.file_ref)
                {
                    matched[i] = true;
                    plan[k] = Plan::Existing(i);
                }
            }
            for (i, e) in existing.iter().enumerate() {
                if matched[i] {
                    self.used.insert(e.id);
                }
            }
        }
        // 3. Same file reference elsewhere in the volume (moved here).
        if self.lookups {
            for (k, &n) in kids.iter().enumerate() {
                if !matches!(plan[k], Plan::New) {
                    continue;
                }
                let c = tree.node(n);
                let Some(r) = c.meta.file_ref else { continue };
                // Built on first need: most updates never look anything up (ADR-0017).
                if self.ref_map.is_none() {
                    self.ref_map = Some(store::file_ref_map(conn, self.volume)?);
                }
                let candidates: Vec<EntryId> = self
                    .ref_map
                    .as_ref()
                    .map(|m| m.get(r).iter().map(|(_, id)| *id).collect())
                    .unwrap_or_default();
                let mut found = None;
                for id in candidates {
                    if self.used.contains(id) || self.protected.contains(&id) {
                        continue;
                    }
                    // The map reflects the catalog before this run; the record itself is
                    // read now (an entry moved earlier in this run is already `used`).
                    if let Some(e) = store::entry(conn, id)?.filter(|e| e.is_dir == c.is_dir) {
                        found = Some(e);
                        break;
                    }
                }
                if let Some(e) = found {
                    self.used.insert(e.id);
                    if let Some(old_parent) = e.parent {
                        self.seeds.push(old_parent);
                    }
                    plan[k] = Plan::MovedIn(e);
                }
            }
        }

        // Park the catalog children that are renamed or whose name is taken by another entry.
        let mut final_name: Vec<Option<&str>> = vec![None; existing.len()];
        for (k, p) in plan.iter().enumerate() {
            if let Plan::Existing(i) = p {
                final_name[*i] = Some(tree.node(kids[k]).name);
            }
        }
        let taken: HashSet<&str> = kids.iter().map(|&n| tree.node(n).name).collect();
        let mut parked = vec![false; existing.len()];
        let mut changed = is_new;
        for (i, e) in existing.iter().enumerate() {
            let renamed = final_name[i].is_some_and(|f| f != e.name);
            let displaced = !matched[i] && taken.contains(e.name.as_str());
            if renamed || displaced {
                parked[i] = true;
                self.push(WriteOp::Move {
                    id: e.id,
                    parent: dir,
                    name: parked_name(e.id),
                    meta: None,
                });
            }
        }

        for (k, p) in plan.into_iter().enumerate() {
            let n = kids[k];
            let c = tree.node(n);
            match p {
                Plan::Existing(i) => {
                    let e = &existing[i];
                    let new_meta = (e.meta != c.meta).then_some(c.meta);
                    if parked[i] {
                        self.push(WriteOp::Move {
                            id: e.id,
                            parent: dir,
                            name: c.name.to_string(),
                            meta: new_meta,
                        });
                        self.stats.moved += 1;
                        changed = true;
                    } else if let Some(meta) = new_meta {
                        self.push(WriteOp::Update { id: e.id, meta });
                        self.stats.updated += 1;
                        changed = true;
                    } else {
                        self.stats.unchanged += 1;
                    }
                    if c.is_dir {
                        queue.push_back((n, e.id, false));
                    }
                }
                Plan::MovedIn(e) => {
                    self.push(WriteOp::Move {
                        id: e.id,
                        parent: dir,
                        name: c.name.to_string(),
                        meta: (e.meta != c.meta).then_some(c.meta),
                    });
                    self.stats.moved += 1;
                    changed = true;
                    if c.is_dir {
                        queue.push_back((n, e.id, false));
                    }
                }
                Plan::New => {
                    let id = self.store.allocate_entry_ids(1);
                    self.push(WriteOp::Insert {
                        id,
                        volume: self.volume,
                        parent: Some(dir),
                        name: c.name.to_string(),
                        is_dir: c.is_dir,
                        meta: c.meta,
                    });
                    self.stats.inserted += 1;
                    changed = true;
                    if c.is_dir {
                        queue.push_back((n, id, true));
                    }
                }
            }
        }

        // Catalog children not seen: deleted, unless the folder was not completely read (then
        // they are kept, except those whose name now belongs to another entry).
        let complete = tree.node(node).listing == Listing::Complete;
        for (i, e) in existing.iter().enumerate() {
            if !matched[i] && (complete || parked[i]) {
                self.pending_deletes.push(e.id);
                changed = true;
            }
        }
        if changed {
            self.seeds.push(dir);
        }
        if self.batch.len() >= BATCH_SIZE {
            self.submit()?;
        }
        Ok(())
    }

    fn push(&mut self, op: WriteOp) {
        self.batch.push(op);
    }

    fn submit(&mut self) -> Result<(), StoreError> {
        let ops = std::mem::take(&mut self.batch);
        self.session.submit(self.volume, ops)
    }
}

fn refs_compatible(a: Option<u64>, b: Option<u64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x == y,
        _ => true,
    }
}

fn parked_name(id: EntryId) -> String {
    format!("{PARKED_PREFIX}parked-{}", id.0)
}

/// A compact set of entry ids (a bitmap: ids are dense positive integers).
#[derive(Default)]
struct IdSet {
    words: Vec<u64>,
}

impl IdSet {
    fn slot(id: EntryId) -> Option<(usize, u64)> {
        let v = usize::try_from(id.0).ok()?;
        Some((v / 64, 1u64 << (v % 64)))
    }

    fn insert(&mut self, id: EntryId) {
        if let Some((word, bit)) = Self::slot(id) {
            if word >= self.words.len() {
                self.words.resize(word + 1, 0);
            }
            self.words[word] |= bit;
        }
    }

    fn contains(&self, id: EntryId) -> bool {
        Self::slot(id).is_some_and(|(word, bit)| self.words.get(word).is_some_and(|w| w & bit != 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_set() {
        let mut s = IdSet::default();
        assert!(!s.contains(EntryId(5)));
        s.insert(EntryId(5));
        s.insert(EntryId(1000));
        assert!(s.contains(EntryId(5)));
        assert!(s.contains(EntryId(1000)));
        assert!(!s.contains(EntryId(999)));
        assert!(!s.contains(EntryId(-1)));
    }

    #[test]
    fn parked_names_start_with_the_prefix() {
        assert!(parked_name(EntryId(42)).starts_with(PARKED_PREFIX));
    }
}
