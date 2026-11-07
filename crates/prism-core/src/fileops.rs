//! Safe deletion (ADR-0012, F16): the single path through which Prism removes files, used by
//! the cleanup (T4) and later by duplicates and similar files.
//!
//! The interface names entries by catalog id; this module resolves their paths, checks every
//! item on the disk against the catalog right before acting (a changed or missing item is
//! skipped and reported, never removed), hands the verified paths to a [`Remover`] (the Recycle
//! Bin by default, `prism-win::trash`), records every outcome in the actions log and removes the
//! deleted entries from the catalog.
//!
//! In tests and development commands a [`SandboxGuard`] refuses any path outside the allowed
//! roots (TESTING S2): the check happens before anything is touched.
//!
//! Not here yet: the "last copy of a duplicate" rule (F16-AC4) and the hash check of duplicates
//! (F16-AC3) arrive with the duplicates (T6).

use std::path::{Path, PathBuf};

use crate::model::{EntryId, VolumeId, unix_millis_now};
use crate::names;
use crate::sandbox::SandboxGuard;
use crate::scan::disk_meta;
use crate::store::{self, ActionRecord, Store, StoreError, WriteOp};

/// Where removed items go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposal {
    /// The Windows Recycle Bin (F16-AC1): the user can restore.
    RecycleBin,
    /// Gone for good: only on volumes without a Recycle Bin (network shares, removable disks),
    /// and only after the user confirmed it explicitly (F16-AC2).
    Permanent,
}

impl Disposal {
    fn action(self) -> &'static str {
        match self {
            Self::RecycleBin => "recycle",
            Self::Permanent => "delete",
        }
    }
}

/// What must still hold for a folder to be removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderCheck {
    /// The folder exists (removing it removes its content: browser caches).
    Exists,
    /// The folder exists and has no file anywhere below it, only empty folders (empty-folder
    /// cleanup). Links and junctions below it count as content: they are never removed this
    /// way.
    NoFiles,
}

/// Removes verified paths from the disk.
pub trait Remover {
    /// Removes `paths` (all on one volume) the way `disposal` says. Returns the paths that could
    /// not be removed, with the reason; the caller also checks the disk afterwards.
    fn remove(&self, paths: &[PathBuf], disposal: Disposal) -> Vec<(PathBuf, String)>;
}

/// A deletion asked by the interface.
#[derive(Clone, Debug)]
pub struct DeleteRequest<'a> {
    pub volume: VolumeId,
    /// Path through which the volume is reached now (mount or UNC).
    pub root: &'a Path,
    pub entries: &'a [EntryId],
    pub disposal: Disposal,
    pub folders: FolderCheck,
}

/// What happened to one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Size, date or kind differ from the catalog: the item was left alone (F16-AC3).
    SkippedChanged,
    SkippedMissing,
    Failed(String),
}

impl Outcome {
    fn key(&self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::SkippedChanged => "skipped_changed",
            Self::SkippedMissing => "skipped_missing",
            Self::Failed(_) => "failed",
        }
    }
}

/// One item of a deletion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemReport {
    pub entry: EntryId,
    pub path: PathBuf,
    /// Size in the catalog (subtree size for a folder).
    pub size: u64,
    pub outcome: Outcome,
}

/// The result of a deletion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeleteReport {
    pub items: Vec<ItemReport>,
}

impl DeleteReport {
    /// Bytes removed from the disk (moved to the Recycle Bin or deleted).
    pub fn freed(&self) -> u64 {
        self.items
            .iter()
            .filter(|i| i.outcome == Outcome::Done)
            .map(|i| i.size)
            .sum()
    }

    pub fn count(&self, outcome: &str) -> usize {
        self.items
            .iter()
            .filter(|i| i.outcome.key() == outcome)
            .count()
    }
}

/// Errors that stop a deletion before anything is removed.
#[derive(Debug, thiserror::Error)]
pub enum FileOpsError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("entry {0} is not on volume {1}, or is its root")]
    WrongEntry(EntryId, VolumeId),
    #[error("refused by the sandbox guard: {0}")]
    Sandbox(String),
}

struct Target {
    entry: EntryId,
    parent: Option<EntryId>,
    path: PathBuf,
    is_dir: bool,
    size: u64,
    modified: Option<crate::model::FileTime>,
}

/// Deletes catalog entries from the disk after checking each one (ADR-0012).
///
/// # Errors
/// [`FileOpsError`] if an id does not belong to the volume, a path is refused by the guard or
/// the catalog cannot be read; nothing is removed in that case. Problems with single items are
/// outcomes in the report, not errors.
pub fn delete_entries(
    store: &Store,
    req: &DeleteRequest<'_>,
    remover: &dyn Remover,
    guard: Option<&SandboxGuard>,
) -> Result<DeleteReport, FileOpsError> {
    let targets = resolve(store, req)?;
    if let Some(guard) = guard {
        for t in &targets {
            guard
                .check_no_follow(&t.path)
                .map_err(|e| FileOpsError::Sandbox(e.to_string()))?;
        }
    }

    // Check every item on the disk right before acting (F16-AC3).
    let mut items: Vec<ItemReport> = Vec::new();
    let mut verified: Vec<usize> = Vec::new();
    for t in &targets {
        let outcome = match disk_meta(&t.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Outcome::SkippedMissing),
            Err(e) => Some(Outcome::Failed(e.to_string())),
            Ok((is_dir, meta)) => {
                let same = if t.is_dir {
                    is_dir && (req.folders == FolderCheck::Exists || has_no_files(&t.path))
                } else {
                    !is_dir && meta.size == t.size && meta.modified == t.modified
                };
                (!same).then_some(Outcome::SkippedChanged)
            }
        };
        if outcome.is_none() {
            verified.push(items.len());
        }
        items.push(ItemReport {
            entry: t.entry,
            path: t.path.clone(),
            size: t.size,
            outcome: outcome.unwrap_or(Outcome::Done),
        });
    }

    let paths: Vec<PathBuf> = verified.iter().map(|&i| items[i].path.clone()).collect();
    let refused = if paths.is_empty() {
        Vec::new()
    } else {
        remover.remove(&paths, req.disposal)
    };
    for &i in &verified {
        let item = &mut items[i];
        // The disk tells what happened, whatever the remover reported.
        let still_there = std::fs::symlink_metadata(&item.path).is_ok();
        if still_there {
            let reason = refused
                .iter()
                .find(|(p, _)| *p == item.path)
                .map_or_else(|| "still on the disk".to_owned(), |(_, r)| r.clone());
            item.outcome = Outcome::Failed(reason);
        }
    }

    // Catalog: removed entries go, their parents' totals are recomputed.
    let mut ops = Vec::new();
    let mut parents = Vec::new();
    for (item, t) in items.iter().zip(&targets) {
        if item.outcome == Outcome::Done {
            ops.push(WriteOp::DeleteSubtree { id: item.entry });
            if let Some(p) = t.parent {
                parents.push(p);
            }
        }
    }
    if !ops.is_empty() {
        parents.sort_unstable();
        parents.dedup();
        ops.push(WriteOp::RecomputeDirStats { dirs: parents });
        let session = store.session();
        session.submit(req.volume, ops)?;
        session.flush()?;
    }

    // Actions log (F16-AC6).
    let at = unix_millis_now();
    let records: Vec<ActionRecord> = items
        .iter()
        .map(|i| ActionRecord {
            id: 0,
            at,
            action: req.disposal.action().to_owned(),
            volume: Some(req.volume),
            path: i.path.to_string_lossy().into_owned(),
            size: i.size,
            outcome: i.outcome.key().to_owned(),
            detail: match &i.outcome {
                Outcome::Failed(r) => Some(r.clone()),
                _ => None,
            },
        })
        .collect();
    store.write(move |w| store::log_actions(w, &records))?;
    Ok(DeleteReport { items })
}

/// Whether a folder holds no file at any depth. Anything unreadable, any link or junction
/// counts as content (the folder is then not removed).
fn has_no_files(path: &Path) -> bool {
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return false;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return false;
            };
            match entry.file_type() {
                Ok(t) if t.is_dir() && !t.is_symlink() => stack.push(entry.path()),
                _ => return false,
            }
        }
    }
    true
}

/// Resolves the ids to paths and catalog data; drops entries inside another selected folder.
fn resolve(store: &Store, req: &DeleteRequest<'_>) -> Result<Vec<Target>, FileOpsError> {
    let volume = req.volume;
    let entries = req.entries.to_vec();
    let root = req.root.to_path_buf();
    let found = store.read(move |c| {
        let mut out = Vec::new();
        for id in entries {
            let Some(e) = store::entry(c, id)? else {
                continue; // removed meanwhile: nothing to do
            };
            if e.volume != volume || e.parent.is_none() {
                return Ok(Err(FileOpsError::WrongEntry(id, volume)));
            }
            let size = if e.is_dir {
                store::dir_stats(c, id)?.map_or(0, |d| d.total_size)
            } else {
                e.meta.size
            };
            let mut path = root.clone();
            for part in store::relative_path(c, id)?
                .split('\\')
                .filter(|p| !p.is_empty())
            {
                path.push(names::decode(part));
            }
            let chain = store::ancestors_and_self(c, id)?;
            out.push((
                Target {
                    entry: id,
                    parent: e.parent,
                    path,
                    is_dir: e.is_dir,
                    size,
                    modified: e.meta.modified,
                },
                chain,
            ));
        }
        Ok(Ok(out))
    })??;
    let selected: std::collections::HashSet<EntryId> = found.iter().map(|(t, _)| t.entry).collect();
    let mut seen = std::collections::HashSet::new();
    Ok(found
        .into_iter()
        .filter(|(_, chain)| !chain.iter().skip(1).any(|a| selected.contains(a)))
        .filter(|(t, _)| seen.insert(t.entry))
        .map(|(t, _)| t)
        .collect())
}
