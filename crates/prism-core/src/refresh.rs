//! Targeted updates from folder-change notifications (volumes without a change journal,
//! ADR-0007): the paths reported by the watcher become a set of catalog folders whose listing
//! is read again and reconciled with the same reconciler as a full indexing, so the result
//! equals a full indexing of the same state (F05-AC3).
//!
//! For every changed path, the folder whose listing changed is its parent: the nearest folder
//! of that parent that is in the catalog is refreshed. When the path itself is a folder of the
//! catalog, it is refreshed too (its own dates, or its content if it was replaced by another
//! folder of the same name). Paths in excluded areas are ignored.
//!
//! A refresh reads the **listing** of each folder, not what is below it: the subfolders that
//! are already in the catalog are left as they are (what changes in them comes with its own
//! notifications), the new ones are read in full. Seen on a busy company file server:
//! re-reading the whole subtree of each changed folder turned every change made directly in a
//! top-level folder into a re-read of the whole share (3.2 million entries, 6 minutes), back to
//! back for hours. When notifications were lost (`rescan`), the whole volume is enumerated.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::cancel::CancelToken;
use crate::exclusion::ExclusionPolicy;
use crate::indexer::{self, IndexError, IndexProgress, IndexRequest};
use crate::model::{EntryId, EntryMeta, VolumeId, VolumeStatus};
use crate::names;
use crate::reconcile::{self, ReconcileStats, ReconcileTarget};
use crate::scan::{
    Enumerator, Listing, ScanChild, ScanError, ScanNode, ScanRequest, ScanStats, ScanTree,
    ScanTreeBuilder,
};
use crate::store::{self, Store, StoreError};

/// A refresh that reads at least this many entries, or takes at least this long, is logged at
/// the info level; the others (every few seconds on a busy share) at the debug level.
const NOTABLE_ENTRIES: u64 = 10_000;
const NOTABLE_TIME: Duration = Duration::from_secs(5);

/// A catalog folder whose listing is read again, with its path relative to the volume root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshTarget {
    pub folder: EntryId,
    pub components: Vec<String>,
}

/// The catalog folders to refresh for `changed` paths (components relative to the volume root,
/// stored names), each once, parents before their subfolders.
///
/// # Errors
/// SQLite errors.
pub fn folders_to_refresh(
    conn: &Connection,
    volume: VolumeId,
    policy: &ExclusionPolicy,
    changed: &[Vec<String>],
) -> Result<Vec<RefreshTarget>, StoreError> {
    let Some(root) = store::root_entry(conn, volume)? else {
        return Ok(Vec::new());
    };
    let mut found: Vec<RefreshTarget> = Vec::new();
    for path in changed {
        if path.is_empty() || policy.is_excluded(path, true) || policy.is_excluded(path, false) {
            continue;
        }
        // The catalog folders along the path, from the root; names as stored in the catalog
        // (the real ones on disk), not as reported.
        let mut chain = vec![RefreshTarget {
            folder: root.id,
            components: Vec::new(),
        }];
        for component in path {
            let Some(last) = chain.last() else { break };
            match store::child_by_name(conn, last.folder, component)? {
                Some(child) if child.is_dir => {
                    let mut components = last.components.clone();
                    components.push(child.name);
                    chain.push(RefreshTarget {
                        folder: child.id,
                        components,
                    });
                }
                _ => break,
            }
        }
        // The nearest catalog folder of the parent, and the path itself if it is a folder.
        let parent = chain.len().min(path.len()) - 1;
        for t in chain.into_iter().skip(parent) {
            add_target(&mut found, t);
        }
    }
    found.sort_by_key(|t| t.components.len());
    Ok(found)
}

fn add_target(found: &mut Vec<RefreshTarget>, t: RefreshTarget) {
    if !found.iter().any(|f| f.folder == t.folder) {
        found.push(t);
    }
}

/// Whether `path` is a folder, not a link to one (junctions and symbolic links are not followed).
fn is_real_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

fn disk_path(root: &Path, components: &[String]) -> std::path::PathBuf {
    let mut p = root.to_path_buf();
    for c in components {
        p.push(names::decode(c));
    }
    p
}

/// What a refresh did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshReport {
    /// The folders whose listing was read (the volume root for a full re-read).
    pub targets: Vec<RefreshTarget>,
    pub scan: ScanStats,
    pub reconcile: ReconcileStats,
}

/// Errors of a refresh.
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Index(#[from] IndexError),
}

/// Whether two file references can belong to the same file (as the reconciler matches them).
fn refs_compatible(a: Option<u64>, b: Option<u64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x == y,
        _ => true,
    }
}

/// The snapshot of one refreshed folder: its listing, where the subfolders already in the
/// catalog (`known`: their stored names and catalog metadata) are not listed and keep their
/// metadata, and the new ones are read in full.
fn folder_snapshot(
    enumerator: &dyn Enumerator,
    request: &ScanRequest<'_>,
    known: &HashMap<String, EntryMeta>,
) -> Result<ScanTree, ScanError> {
    let listing = enumerator.list(request)?;
    let known_meta = |n: ScanNode<'_>| -> Option<EntryMeta> {
        if !n.is_dir {
            return None;
        }
        known
            .get(n.name)
            .filter(|m| refs_compatible(m.file_ref, n.meta.file_ref))
            .copied()
    };
    let mut stats = listing.stats.clone();
    let mut builder = ScanTreeBuilder::new(listing.root().meta);
    if listing.root().listing == Listing::Unreadable {
        builder.mark_unreadable(ScanTree::ROOT);
    }
    let kids = listing.children(ScanTree::ROOT);
    let range = builder.add_listing(
        ScanTree::ROOT,
        kids.clone().map(|k| {
            let n = listing.node(k);
            ScanChild {
                name: n.name.to_owned(),
                is_dir: n.is_dir,
                meta: known_meta(n).unwrap_or(n.meta),
            }
        }),
    );
    for (k, node) in kids.zip(range) {
        let n = listing.node(k);
        if !n.is_dir {
            continue;
        }
        if known_meta(n).is_some() {
            builder.mark_not_listed(node);
        } else if n.listing == Listing::NotListed {
            // A new folder: read in full, with its own metadata as a full scan records it.
            let mut relative = request.relative.to_vec();
            relative.push(n.name.to_owned());
            let path = request.root.join(names::decode(n.name));
            let sub = enumerator.scan(&ScanRequest {
                root: &path,
                relative: &relative,
                policy: request.policy,
                cancel: request.cancel,
                progress: request.progress,
            });
            match sub {
                Ok(sub) => {
                    stats.add(&sub.stats);
                    builder.set_meta(node, sub.root().meta);
                    builder.graft(node, &sub, ScanTree::ROOT);
                }
                Err(e @ ScanError::Cancelled(_)) => return Err(e),
                Err(e) => {
                    // Gone or unreadable since the listing: its own notification follows.
                    stats.unreadable_dirs += 1;
                    stats.push_error(e.to_string());
                    builder.mark_unreadable(node);
                }
            }
        } else {
            // The enumerator gave the content already.
            builder.graft(node, &listing, k);
        }
    }
    Ok(builder.finish(stats))
}

/// Re-reads the listings of the folders affected by `changed` paths (see the module
/// documentation). `rescan` (notifications were lost) re-reads the whole volume.
///
/// # Errors
/// [`RefreshError`]. Nothing is written while the folders are read; while the reconciler
/// writes, the volume is `indexing`, and `reindex_needed` if it does not complete, as for
/// [`indexer::index`].
#[allow(clippy::too_many_arguments)]
pub fn refresh_folders(
    store: &Store,
    volume: VolumeId,
    root: &Path,
    enumerator: &dyn Enumerator,
    policy: &ExclusionPolicy,
    changed: &[Vec<String>],
    rescan: bool,
    cancel: &CancelToken,
    progress: &IndexProgress,
) -> Result<RefreshReport, RefreshError> {
    if rescan {
        let report = indexer::index(
            store,
            enumerator,
            &IndexRequest {
                volume,
                path: root,
                relative: &[],
                target: ReconcileTarget::VolumeRoot,
                policy,
            },
            cancel,
            progress,
        )?;
        return Ok(RefreshReport {
            targets: vec![RefreshTarget {
                folder: EntryId(0),
                components: Vec::new(),
            }],
            scan: report.scan,
            reconcile: report.reconcile,
        });
    }
    let started = Instant::now();
    let found = store.read(|c| folders_to_refresh(c, volume, policy, changed))?;
    // A folder that no longer exists on disk (deleted or moved since), or that is a junction or a
    // symbolic link (a full indexing records it without following it), is refreshed through its
    // nearest real ancestor.
    let targets = store.read(|c| {
        let mut adjusted: Vec<RefreshTarget> = Vec::new();
        for mut t in found {
            while !t.components.is_empty() && !is_real_folder(&disk_path(root, &t.components)) {
                t.components.pop();
            }
            let mut folder = store::root_entry(c, volume)?.map_or(EntryId(0), |r| r.id);
            for name in &t.components {
                if let Some(child) = store::child_by_name(c, folder, name)? {
                    folder = child.id;
                }
            }
            t.folder = folder;
            add_target(&mut adjusted, t);
        }
        adjusted.sort_by_key(|t| t.components.len());
        Ok(adjusted)
    })?;

    // Reading: nothing is written yet.
    let mut snapshots: Vec<(ReconcileTarget, ScanTree)> = Vec::new();
    let mut read: Vec<RefreshTarget> = Vec::new();
    let mut scan = ScanStats::default();
    for t in targets {
        let known: HashMap<String, EntryMeta> = store.read(|c| {
            Ok(store::children(c, t.folder)?
                .into_iter()
                .filter(|e| e.is_dir)
                .map(|e| (e.name, e.meta))
                .collect())
        })?;
        let path = disk_path(root, &t.components);
        let request = ScanRequest {
            root: &path,
            relative: &t.components,
            policy,
            cancel,
            progress: &progress.scanned,
        };
        match folder_snapshot(enumerator, &request, &known) {
            Ok(tree) => {
                scan.add(&tree.stats);
                let target = if t.components.is_empty() {
                    ReconcileTarget::VolumeRoot
                } else {
                    ReconcileTarget::Folder(t.folder)
                };
                snapshots.push((target, tree));
                read.push(t);
            }
            Err(e @ ScanError::Cancelled(_)) => return Err(IndexError::Scan(e).into()),
            Err(e) => {
                // Gone since: the notification of its removal refreshes its parent.
                tracing::debug!(volume = volume.0, error = %e, "folder not refreshed");
            }
        }
    }
    let scan_time = started.elapsed();
    if snapshots.is_empty() {
        return Ok(RefreshReport {
            targets: read,
            scan,
            reconcile: ReconcileStats::default(),
        });
    }

    store.write(move |w| store::set_volume_status(w, volume, VolumeStatus::Indexing))?;
    let trees: Vec<(ReconcileTarget, &ScanTree)> =
        snapshots.iter().map(|(t, tree)| (*t, tree)).collect();
    match reconcile::reconcile_many(store, volume, &trees, cancel, &progress.reconciled) {
        Ok(stats) => {
            store.write(move |w| store::set_volume_status(w, volume, VolumeStatus::Indexed))?;
            let entries = scan.files + scan.dirs;
            let ms = started.elapsed().as_millis();
            if entries >= NOTABLE_ENTRIES || started.elapsed() >= NOTABLE_TIME {
                tracing::info!(
                    volume = volume.0,
                    folders = read.len(),
                    entries,
                    read_ms = scan_time.as_millis(),
                    ms,
                    ?stats,
                    "folders refreshed"
                );
            } else {
                tracing::debug!(
                    volume = volume.0,
                    folders = read.len(),
                    entries,
                    ms,
                    ?stats,
                    "folders refreshed"
                );
            }
            Ok(RefreshReport {
                targets: read,
                scan,
                reconcile: stats,
            })
        }
        Err(e) => {
            tracing::warn!(volume = volume.0, error = %e, "refresh did not complete");
            store
                .write(move |w| store::set_volume_status(w, volume, VolumeStatus::ReindexNeeded))?;
            Err(IndexError::Reconcile(e).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntryMeta, VolumeInfo, VolumeStatus};
    use crate::store::WriteOp;

    fn path(p: &str) -> Vec<String> {
        p.split('\\').map(str::to_owned).collect()
    }

    /// root / A / B, root / C (folders).
    fn catalog() -> (tempfile::TempDir, Store, VolumeId) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("c.db")).unwrap();
        let info = VolumeInfo::dev_folder("X:");
        let v = store
            .write(move |w| store::insert_volume(w, &info, VolumeStatus::New))
            .unwrap();
        let first = store.allocate_entry_ids(4);
        let dirop = |i: i64, parent: Option<i64>, name: &str| WriteOp::Insert {
            id: EntryId(first.0 + i),
            volume: v,
            parent: parent.map(|p| EntryId(first.0 + p)),
            name: name.into(),
            is_dir: true,
            meta: EntryMeta::default(),
        };
        let s = store.session();
        s.submit(
            v,
            vec![
                dirop(0, None, ""),
                dirop(1, Some(0), "A"),
                dirop(2, Some(1), "B"),
                dirop(3, Some(0), "C"),
            ],
        )
        .unwrap();
        s.flush().unwrap();
        (dir, store, v)
    }

    fn components(t: &[RefreshTarget]) -> Vec<Vec<String>> {
        t.iter().map(|t| t.components.clone()).collect()
    }

    #[test]
    fn nearest_catalog_folders_and_changed_folders() {
        let (_d, store, v) = catalog();
        let policy = ExclusionPolicy::with_defaults();
        let refresh = |changed: &[Vec<String>]| {
            components(
                &store
                    .read(|c| folders_to_refresh(c, v, &policy, changed))
                    .unwrap(),
            )
        };
        let changed = vec![
            path(r"A\B\new.txt"),
            path(r"A\B\deep\unknown\x.bin"), // B is the nearest catalog folder
            path(r"c\file.txt"),             // case-insensitive
            path(r"$RECYCLE.BIN\S-1\gone.txt"), // excluded
            path("System Volume Information"), // excluded itself
        ];
        let mut found = refresh(&changed);
        found.sort();
        assert_eq!(found, vec![path(r"A\B"), vec!["C".to_owned()]]);

        // A change in A and one in A\B: both listings, parents first (a refresh of A does not
        // read A\B any more).
        assert_eq!(
            refresh(&[path(r"A\B\x"), path(r"A\y")]),
            vec![path("A"), path(r"A\B")]
        );
        // A change at the root reads the listing of the root, not the whole volume.
        assert_eq!(
            refresh(&[path(r"A\B\x"), path("top.txt")]),
            vec![Vec::<String>::new(), path(r"A\B")]
        );
        // A changed folder of the catalog: its parent and itself.
        assert_eq!(refresh(&[path(r"a\b")]), vec![path("A"), path(r"A\B")]);
        assert_eq!(refresh(&[path("C")]), vec![Vec::<String>::new(), path("C")]);
    }
}
