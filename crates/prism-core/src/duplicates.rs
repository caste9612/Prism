//! Exact duplicates (F10): groups of files with the same size and the same full hash, on any
//! volume, online or not (F10-AC1), the selection rules (F10-AC3) and the last-copy rule that
//! the deletion enforces (F10-AC4, F16-AC4).

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::model::{EntryId, FileTime, VolumeId};
use crate::names;
use crate::space::{self, FileCategory};
use crate::store::{self, Digest, StoreError};

/// One copy in a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DupFile {
    pub entry: EntryId,
    pub volume: VolumeId,
    /// Display path relative to the volume root (`\` separators), name included.
    pub path: String,
    pub modified: Option<FileTime>,
    pub online: bool,
}

/// Files with the same content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DupGroup {
    pub size: u64,
    pub hash: Digest,
    pub files: Vec<DupFile>,
}

impl DupGroup {
    pub fn online(&self) -> usize {
        self.files.iter().filter(|f| f.online).count()
    }

    pub fn offline(&self) -> usize {
        self.files.len() - self.online()
    }

    /// Space freed by keeping a single copy (F10-AC2).
    pub fn recoverable(&self) -> u64 {
        self.size * (self.files.len() as u64).saturating_sub(1)
    }
}

/// Which groups to show (F10-AC5); a group is shown when any of its copies matches the volume
/// and folder filters.
#[derive(Clone, Debug, Default)]
pub struct DupFilter {
    pub min_size: u64,
    pub category: Option<FileCategory>,
    pub volume: Option<VolumeId>,
    /// Case-folded text the path of a copy must contain.
    pub folder: Option<String>,
}

fn display_path(conn: &Connection, entry: EntryId) -> Result<String, StoreError> {
    Ok(store::relative_path(conn, entry)?
        .split('\\')
        .filter(|p| !p.is_empty())
        .map(|p| names::decode(p).to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\\"))
}

/// The duplicate groups, largest recoverable space first.
///
/// # Errors
/// SQLite errors.
pub fn groups(
    conn: &Connection,
    online: &HashSet<VolumeId>,
    filter: &DupFilter,
) -> Result<Vec<DupGroup>, StoreError> {
    let files = store::duplicate_files(conn, filter.min_size.max(1))?;
    let mut out: Vec<DupGroup> = Vec::new();
    for f in files {
        let new_group = out
            .last()
            .is_none_or(|g| g.size != f.size || g.hash != f.full);
        if new_group {
            out.push(DupGroup {
                size: f.size,
                hash: f.full,
                files: Vec::new(),
            });
        }
        let e = store::entry(conn, f.entry)?;
        let modified = e.and_then(|e| e.meta.modified);
        let path = display_path(conn, f.entry)?;
        if let Some(g) = out.last_mut() {
            g.files.push(DupFile {
                entry: f.entry,
                volume: f.volume,
                path,
                modified,
                online: online.contains(&f.volume),
            });
        }
    }
    let folder = filter.folder.as_deref().map(names::fold);
    out.retain(|g| {
        let category_ok = filter.category.is_none_or(|c| {
            g.files
                .first()
                .is_some_and(|f| space::category_of(&space::extension(&f.path)) == c)
        });
        let copy_ok = g.files.iter().any(|f| {
            filter.volume.is_none_or(|v| f.volume == v)
                && folder
                    .as_deref()
                    .is_none_or(|p| names::fold(&f.path).contains(p))
        });
        g.files.len() > 1 && category_ok && copy_ok
    });
    out.sort_by(|a, b| {
        b.recoverable()
            .cmp(&a.recoverable())
            .then(b.size.cmp(&a.size))
    });
    Ok(out)
}

/// A selection rule (F10-AC3). The `Keep*` rules choose, in order, which copy stays; the others
/// are selected for deletion, except those protected by [`Rule::NeverIn`] and those on disks
/// that are not connected (they cannot be deleted now).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rule {
    KeepOnVolume(VolumeId),
    KeepOldest,
    KeepNewest,
    KeepShortestPath,
    /// Never select copies whose path contains this text (case-insensitive).
    NeverIn(String),
}

/// The copies of a group that the rules select for deletion. One copy always stays: the one
/// the `Keep*` rules prefer, the first one when they cannot decide (F10-AC4).
pub fn select(group: &DupGroup, rules: &[Rule]) -> Vec<EntryId> {
    if group.files.len() < 2 {
        return Vec::new();
    }
    let mut keepers: Vec<&DupFile> = group.files.iter().collect();
    for rule in rules {
        let narrowed: Vec<&DupFile> = match rule {
            Rule::KeepOnVolume(v) => keepers.iter().copied().filter(|f| f.volume == *v).collect(),
            Rule::KeepOldest => {
                let min = keepers.iter().filter_map(|f| f.modified).min();
                keepers
                    .iter()
                    .copied()
                    .filter(|f| f.modified == min)
                    .collect()
            }
            Rule::KeepNewest => {
                let max = keepers.iter().filter_map(|f| f.modified).max();
                keepers
                    .iter()
                    .copied()
                    .filter(|f| f.modified == max)
                    .collect()
            }
            Rule::KeepShortestPath => {
                let min = keepers.iter().map(|f| f.path.chars().count()).min();
                keepers
                    .iter()
                    .copied()
                    .filter(|f| Some(f.path.chars().count()) == min)
                    .collect()
            }
            Rule::NeverIn(_) => continue,
        };
        if !narrowed.is_empty() {
            keepers = narrowed;
        }
    }
    let keeper = keepers.iter().min_by_key(|f| f.entry).map(|f| f.entry);
    let protected: Vec<String> = rules
        .iter()
        .filter_map(|r| match r {
            Rule::NeverIn(p) => Some(names::fold(p)),
            _ => None,
        })
        .collect();
    group
        .files
        .iter()
        .filter(|f| Some(f.entry) != keeper && f.online)
        .filter(|f| {
            let path = names::fold(&f.path);
            !protected.iter().any(|p| path.contains(p.as_str()))
        })
        .map(|f| f.entry)
        .collect()
}

/// Why a deletion of duplicates is refused (F16-AC4).
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum LastCopyError {
    /// It would remove every copy of a content.
    #[error("entry {0} is the last copy of its content")]
    LastCopy(EntryId),
    /// The only copies left would be on disks that are not connected: the user must confirm.
    #[error("entry {0} would leave copies only on disks that are not connected")]
    OnlyOffline(EntryId),
    /// The entry has no valid full hash (changed since it was hashed).
    #[error("entry {0} has no valid hash")]
    NoHash(EntryId),
}

/// Checks the last-copy rule for deleting `entries` (F16-AC4): every content must keep at least
/// one copy, and a connected one unless `allow_offline_only`.
///
/// # Errors
/// [`LastCopyError`] for the first entry that breaks the rule; SQLite errors as `Store`.
pub fn check_last_copies(
    conn: &Connection,
    entries: &[EntryId],
    online: &HashSet<VolumeId>,
    allow_offline_only: bool,
) -> Result<Result<(), LastCopyError>, StoreError> {
    let doomed: HashSet<EntryId> = entries.iter().copied().collect();
    let mut groups: HashMap<(u64, Digest), Vec<(EntryId, VolumeId)>> = HashMap::new();
    for f in store::duplicate_files(conn, 1)? {
        groups
            .entry((f.size, f.full))
            .or_default()
            .push((f.entry, f.volume));
    }
    let mut key_of: HashMap<EntryId, (u64, Digest)> = HashMap::new();
    for (key, members) in &groups {
        for (e, _) in members {
            key_of.insert(*e, *key);
        }
    }
    for &e in entries {
        let Some(key) = key_of.get(&e) else {
            // Not in any group: either unique content (the last copy) or no valid hash.
            return Ok(Err(match store::valid_hashes(conn, e)? {
                Some(h) if h.full.is_some() => LastCopyError::LastCopy(e),
                _ => LastCopyError::NoHash(e),
            }));
        };
        let left: Vec<&(EntryId, VolumeId)> = groups[key]
            .iter()
            .filter(|(id, _)| !doomed.contains(id))
            .collect();
        if left.is_empty() {
            return Ok(Err(LastCopyError::LastCopy(e)));
        }
        if !allow_offline_only && !left.iter().any(|(_, v)| online.contains(v)) {
            return Ok(Err(LastCopyError::OnlyOffline(e)));
        }
    }
    Ok(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: i64, volume: i64, path: &str, modified: i64, online: bool) -> DupFile {
        DupFile {
            entry: EntryId(id),
            volume: VolumeId(volume),
            path: path.to_owned(),
            modified: Some(FileTime(modified)),
            online,
        }
    }

    fn group() -> DupGroup {
        DupGroup {
            size: 1000,
            hash: [0; 32],
            files: vec![
                file(1, 1, r"Foto\2019\mare.jpg", 300, true),
                file(2, 1, r"Download\mare.jpg", 100, true),
                file(3, 2, r"Backup\Foto\2019\mare.jpg", 200, true),
                file(4, 3, r"Vecchio\mare.jpg", 50, false),
            ],
        }
    }

    #[test]
    fn f10_ac2_counts_and_recoverable_space() {
        let g = group();
        assert_eq!((g.online(), g.offline(), g.recoverable()), (3, 1, 3000));
    }

    // F10-AC3: rules combine in order; F10-AC4: one copy always stays; offline copies are never
    // selected (they cannot be deleted now).
    #[test]
    fn f10_ac3_selection_rules() {
        let g = group();
        let sel = |rules: &[Rule]| -> Vec<i64> { select(&g, rules).iter().map(|e| e.0).collect() };
        // No rule: the first copy stays.
        assert_eq!(sel(&[]), [2, 3]);
        // Keep on volume 2.
        assert_eq!(sel(&[Rule::KeepOnVolume(VolumeId(2))]), [1, 2]);
        // Keep the oldest (the offline one, 50): every online copy is selected.
        assert_eq!(sel(&[Rule::KeepOldest]), [1, 2, 3]);
        // Keep the newest.
        assert_eq!(sel(&[Rule::KeepNewest]), [2, 3]);
        // Keep the shortest path: the offline copy ("Vecchio\mare.jpg"), so every online copy is
        // selected (deleting them then needs the explicit confirmation of F16-AC4).
        assert_eq!(sel(&[Rule::KeepShortestPath]), [1, 2, 3]);
        // Among the copies on volume 1, the shortest path: keep 2.
        assert_eq!(
            sel(&[Rule::KeepOnVolume(VolumeId(1)), Rule::KeepShortestPath]),
            [1, 3]
        );
        // Volume 1, then the newest among those: keep 1.
        assert_eq!(
            sel(&[Rule::KeepOnVolume(VolumeId(1)), Rule::KeepNewest]),
            [2, 3]
        );
        // A rule that matches nothing is skipped.
        assert_eq!(
            sel(&[Rule::KeepOnVolume(VolumeId(9)), Rule::KeepShortestPath]),
            [1, 2, 3]
        );
        // Never select under "backup".
        assert_eq!(sel(&[Rule::NeverIn("BACKUP".into())]), [2]);
        // One copy always stays, whatever the rules.
        for rules in [vec![], vec![Rule::KeepOldest], vec![Rule::KeepNewest]] {
            let selected = select(&g, &rules);
            assert!(selected.len() < g.files.len());
        }
        let single = DupGroup {
            files: vec![file(1, 1, "a", 1, true)],
            ..group()
        };
        assert!(select(&single, &[]).is_empty());
    }
}
