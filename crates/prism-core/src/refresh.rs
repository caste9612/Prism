//! Targeted updates from folder-change notifications (volumes without a change journal,
//! ADR-0007): the paths reported by the watcher become the smallest set of catalog folders to
//! re-enumerate, each reconciled as a subtree with the same reconciler as a full indexing, so
//! the result equals a full indexing of the same state (F05-AC3).
//!
//! For every changed path, the folder whose listing changed is its parent; the nearest folder
//! of that parent that is in the catalog is refreshed. Folders below another refreshed folder
//! are dropped (its subtree enumeration covers them). Paths in excluded areas are ignored.
//! When the volume root itself must be refreshed, the whole volume is enumerated.

use std::path::Path;

use rusqlite::Connection;

use crate::cancel::CancelToken;
use crate::exclusion::ExclusionPolicy;
use crate::indexer::{self, IndexError, IndexProgress, IndexReport, IndexRequest};
use crate::model::{EntryId, VolumeId};
use crate::names;
use crate::reconcile::ReconcileTarget;
use crate::scan::Enumerator;
use crate::store::{self, Store, StoreError};

/// A catalog folder to re-enumerate, with its path relative to the volume root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshTarget {
    pub folder: EntryId,
    pub components: Vec<String>,
}

fn is_prefix(prefix: &[String], path: &[String]) -> bool {
    prefix.len() <= path.len()
        && prefix
            .iter()
            .zip(path)
            .all(|(a, b)| names::eq_ignore_case(a, b))
}

/// The top-most catalog folders to refresh for `changed` paths (components relative to the
/// volume root, stored names).
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
        // The nearest catalog folder of the parent.
        let parent = &path[..path.len() - 1];
        let mut folder = root.id;
        // Names as stored in the catalog (the real ones on disk), not as reported.
        let mut components = Vec::new();
        for component in parent {
            match store::child_by_name(conn, folder, component)? {
                Some(child) if child.is_dir => {
                    folder = child.id;
                    components.push(child.name);
                }
                _ => break,
            }
        }
        if !found.iter().any(|t| t.folder == folder) {
            found.push(RefreshTarget { folder, components });
        }
    }
    Ok(top_most(found))
}

/// Keeps the folders that are not below another one of the list.
fn top_most(mut found: Vec<RefreshTarget>) -> Vec<RefreshTarget> {
    found.sort_by_key(|t| t.components.len());
    let mut top: Vec<RefreshTarget> = Vec::new();
    for t in found {
        if !top
            .iter()
            .any(|kept| is_prefix(&kept.components, &t.components))
        {
            top.push(t);
        }
    }
    top
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
    pub targets: Vec<RefreshTarget>,
    pub reports: Vec<IndexReport>,
}

/// Errors of a refresh.
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Index(#[from] IndexError),
}

/// Re-enumerates the folders affected by `changed` paths (see the module documentation).
/// `rescan` (notifications were lost) refreshes the whole volume.
///
/// # Errors
/// [`RefreshError`]; the volume status follows [`indexer::index`].
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
    let targets = if rescan {
        vec![RefreshTarget {
            folder: EntryId(0),
            components: Vec::new(),
        }]
    } else {
        let found = store.read(|c| folders_to_refresh(c, volume, policy, changed))?;
        // A folder that no longer exists on disk (deleted or moved since) is refreshed
        // through its nearest existing ancestor.
        store.read(|c| {
            let mut adjusted = Vec::new();
            for mut t in found {
                while !t.components.is_empty() && !disk_path(root, &t.components).is_dir() {
                    t.components.pop();
                }
                let mut folder = store::root_entry(c, volume)?.map_or(EntryId(0), |r| r.id);
                for name in &t.components {
                    if let Some(child) = store::child_by_name(c, folder, name)? {
                        folder = child.id;
                    }
                }
                t.folder = folder;
                adjusted.push(t);
            }
            Ok(top_most(adjusted))
        })?
    };
    let mut reports = Vec::new();
    for t in &targets {
        let (path, target) = if t.components.is_empty() {
            (root.to_path_buf(), ReconcileTarget::VolumeRoot)
        } else {
            (
                disk_path(root, &t.components),
                ReconcileTarget::Folder(t.folder),
            )
        };
        reports.push(indexer::index(
            store,
            enumerator,
            &IndexRequest {
                volume,
                path: &path,
                relative: &t.components,
                target,
                policy,
            },
            cancel,
            progress,
        )?);
    }
    Ok(RefreshReport { targets, reports })
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

    #[test]
    fn nearest_catalog_folders_top_most_only() {
        let (_d, store, v) = catalog();
        let policy = ExclusionPolicy::with_defaults();
        let changed = vec![
            path(r"A\B\new.txt"),
            path(r"A\B\deep\unknown\x.bin"), // B is the nearest catalog folder
            path(r"c\file.txt"),             // case-insensitive
            path(r"$RECYCLE.BIN\S-1\gone.txt"), // excluded
            path("System Volume Information"), // excluded itself
        ];
        let t = store
            .read(|c| folders_to_refresh(c, v, &policy, &changed))
            .unwrap();
        let mut comps: Vec<Vec<String>> = t.iter().map(|t| t.components.clone()).collect();
        comps.sort();
        assert_eq!(comps, vec![path(r"A\B"), vec!["C".to_owned()]]);

        // A change in A covers A\B.
        let t = store
            .read(|c| folders_to_refresh(c, v, &policy, &[path(r"A\B\x"), path(r"A\y")]))
            .unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].components, vec!["A".to_owned()]);

        // A change at the root covers everything.
        let t = store
            .read(|c| folders_to_refresh(c, v, &policy, &[path(r"A\B\x"), path("top.txt")]))
            .unwrap();
        assert_eq!(t.len(), 1);
        assert!(t[0].components.is_empty());
    }
}
