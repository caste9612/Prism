//! Backups (F13–F15), on the catalog: they work with disconnected disks too.
//!
//! - [`verify`]: which files of a source folder have no copy in one or more destinations
//!   (F13). A copy is found by content (same size and full hash) when both sides have a hash,
//!   otherwise by name, size and date; the report says how much was checked each way.
//! - [`compare`]: two folders side by side (F14): only in A, only in B, different, identical,
//!   matched by relative path or by content (moved and renamed files).
//! - [`folder_view`]: a level of the result tree with the totals of each subfolder (F13-AC4,
//!   F14-AC3).
//! - [`copy`]: the verified copy of the missing files (F15).

pub mod copy;

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::cancel::CancelToken;
use crate::model::{EntryId, FileTime, VolumeId};
use crate::names;
use crate::store::{self, Digest, StoreError, SubtreeFile};

/// A folder of a volume (its root for a whole volume).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Side {
    pub volume: VolumeId,
    pub root: EntryId,
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error(transparent)]
    Store(#[from] StoreError),
    /// One folder contains the other: every file would be its own copy.
    #[error("the source and a destination overlap")]
    Overlap,
    #[error("cancelled")]
    Cancelled,
}

/// Dates closer than this are the same date: FAT keeps them with a 2-second resolution.
pub const TIME_TOLERANCE: i64 = 2 * 10_000_000;

fn same_time(a: Option<FileTime>, b: Option<FileTime>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a.0 - b.0).abs() <= TIME_TOLERANCE,
        (None, None) => true,
        _ => false,
    }
}

/// Folded file name and size: the key of a match by metadata.
type NameSize = (String, u64);

/// The last component of a relative path, folded for comparisons.
fn folded_name(rel: &str) -> String {
    names::fold(rel.rsplit('\\').next().unwrap_or(rel))
}

fn check_cancel(cancel: &CancelToken) -> Result<(), BackupError> {
    if cancel.is_cancelled() {
        Err(BackupError::Cancelled)
    } else {
        Ok(())
    }
}

/// Whether one folder contains the other (or they are the same).
///
/// # Errors
/// SQLite errors, or `NotFound` for an unknown folder.
pub fn overlaps(conn: &Connection, a: &Side, b: &Side) -> Result<bool, StoreError> {
    if a.volume != b.volume {
        return Ok(false);
    }
    Ok(store::ancestors_and_self(conn, a.root)?.contains(&b.root)
        || store::ancestors_and_self(conn, b.root)?.contains(&a.root))
}

/// A number of files and their size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub files: u64,
    pub bytes: u64,
}

impl Tally {
    pub fn add(&mut self, size: u64) {
        self.files += 1;
        self.bytes += size;
    }
}

/// How a match was established (F13-AC3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Evidence {
    /// Same size and same full hash.
    Content,
    /// Same name, size and date: no hash on one of the sides.
    Metadata,
}

/// The result of a backup check (F13).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VerifyReport {
    pub total: Tally,
    pub by_content: Tally,
    pub by_metadata: Tally,
    /// Source files without a copy, ordered by relative path.
    pub missing: Vec<SubtreeFile>,
}

impl VerifyReport {
    #[must_use]
    pub fn missing_tally(&self) -> Tally {
        let mut t = Tally::default();
        for f in &self.missing {
            t.add(f.size);
        }
        t
    }
}

/// The files of `source` that have no copy anywhere in `destinations` (F13-AC1/AC2/AC3).
///
/// A source file with a hash is backed up when a destination file has the same size and hash;
/// otherwise when a destination file has the same name, size and date and one of the two has
/// no hash (with hashes on both sides and different contents it is not a copy).
///
/// # Errors
/// [`BackupError::Overlap`] if a destination and the source contain each other; SQLite errors;
/// [`BackupError::Cancelled`].
pub fn verify(
    conn: &Connection,
    source: &Side,
    destinations: &[Side],
    cancel: &CancelToken,
) -> Result<VerifyReport, BackupError> {
    for d in destinations {
        if overlaps(conn, source, d)? {
            return Err(BackupError::Overlap);
        }
    }
    let mut by_hash: HashSet<(u64, Digest)> = HashSet::new();
    let mut by_meta: HashMap<NameSize, Vec<(Option<FileTime>, bool)>> = HashMap::new();
    for d in destinations {
        check_cancel(cancel)?;
        for f in store::subtree_files(conn, d.root)? {
            if let Some(h) = f.full {
                by_hash.insert((f.size, h));
            }
            by_meta
                .entry((folded_name(&f.rel), f.size))
                .or_default()
                .push((f.modified, f.full.is_some()));
        }
    }
    check_cancel(cancel)?;
    let mut report = VerifyReport::default();
    for f in store::subtree_files(conn, source.root)? {
        report.total.add(f.size);
        if let Some(h) = f.full
            && by_hash.contains(&(f.size, h))
        {
            report.by_content.add(f.size);
            continue;
        }
        let metadata = by_meta
            .get(&(folded_name(&f.rel), f.size))
            .is_some_and(|candidates| {
                candidates
                    .iter()
                    .any(|(t, hashed)| same_time(f.modified, *t) && (f.full.is_none() || !hashed))
            });
        if metadata {
            report.by_metadata.add(f.size);
        } else {
            report.missing.push(f);
        }
    }
    Ok(report)
}

/// How files of the two sides are paired (F14-AC2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchMode {
    /// Same relative path.
    Path,
    /// Same content wherever it is: finds moved and renamed files.
    Content,
}

/// The state of a file in a comparison (F14-AC1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    OnlyA = 0,
    OnlyB = 1,
    /// Same relative path, different content.
    Different = 2,
    Same = 3,
}

impl Status {
    pub const ALL: [Self; 4] = [Self::OnlyA, Self::OnlyB, Self::Different, Self::Same];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnlyA => "onlyA",
            Self::OnlyB => "onlyB",
            Self::Different => "different",
            Self::Same => "same",
        }
    }
}

/// A file of a comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareItem {
    /// Relative path in A (in B for files only in B).
    pub rel: String,
    /// Relative path in B when it differs from `rel` (moved or renamed, content mode).
    pub other: Option<String>,
    /// Size in A (in B for files only in B).
    pub size: u64,
    pub status: Status,
    /// For identical and different files: how it was decided.
    pub evidence: Option<Evidence>,
}

/// A comparison of two folders, ordered by relative path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompareReport {
    pub items: Vec<CompareItem>,
}

impl CompareReport {
    #[must_use]
    pub fn tally(&self, status: Status) -> Tally {
        let mut t = Tally::default();
        for i in self.items.iter().filter(|i| i.status == status) {
            t.add(i.size);
        }
        t
    }
}

/// Same or different, for two files at the same path.
fn judge(a: &SubtreeFile, b: &SubtreeFile) -> (Status, Evidence) {
    match (a.full, b.full) {
        (Some(x), Some(y)) => (
            if a.size == b.size && x == y {
                Status::Same
            } else {
                Status::Different
            },
            Evidence::Content,
        ),
        _ if a.size != b.size => (Status::Different, Evidence::Content),
        _ => (
            if same_time(a.modified, b.modified) {
                Status::Same
            } else {
                Status::Different
            },
            Evidence::Metadata,
        ),
    }
}

/// Compares two folders (F14-AC1/AC2), also of disconnected disks.
///
/// # Errors
/// [`BackupError::Overlap`] if one contains the other; SQLite errors;
/// [`BackupError::Cancelled`].
pub fn compare(
    conn: &Connection,
    a: &Side,
    b: &Side,
    mode: MatchMode,
    cancel: &CancelToken,
) -> Result<CompareReport, BackupError> {
    if overlaps(conn, a, b)? {
        return Err(BackupError::Overlap);
    }
    let fa = store::subtree_files(conn, a.root)?;
    check_cancel(cancel)?;
    let fb = store::subtree_files(conn, b.root)?;
    check_cancel(cancel)?;
    let mut used_a = vec![false; fa.len()];
    let mut used_b = vec![false; fb.len()];
    let mut items = Vec::new();

    let b_by_path: HashMap<String, usize> = fb
        .iter()
        .enumerate()
        .map(|(j, f)| (names::fold(&f.rel), j))
        .collect();
    // Same path: in path mode every pair; in content mode only the identical ones for now.
    for (i, f) in fa.iter().enumerate() {
        if let Some(&j) = b_by_path.get(&names::fold(&f.rel)) {
            let (status, evidence) = judge(f, &fb[j]);
            if mode == MatchMode::Path || status == Status::Same {
                used_a[i] = true;
                used_b[j] = true;
                items.push(CompareItem {
                    rel: f.rel.clone(),
                    other: None,
                    size: f.size,
                    status,
                    evidence: Some(evidence),
                });
            }
        }
    }
    if mode == MatchMode::Content {
        check_cancel(cancel)?;
        // Same content elsewhere: moved or renamed.
        let mut b_by_hash: HashMap<(u64, Digest), Vec<usize>> = HashMap::new();
        let mut b_by_meta: HashMap<NameSize, Vec<usize>> = HashMap::new();
        for (j, f) in fb.iter().enumerate().filter(|(j, _)| !used_b[*j]).rev() {
            if let Some(h) = f.full {
                b_by_hash.entry((f.size, h)).or_default().push(j);
            }
            b_by_meta
                .entry((folded_name(&f.rel), f.size))
                .or_default()
                .push(j);
        }
        for (i, f) in fa.iter().enumerate() {
            if used_a[i] {
                continue;
            }
            let by_content = f
                .full
                .and_then(|h| b_by_hash.get_mut(&(f.size, h)))
                .and_then(|list| {
                    while let Some(j) = list.pop() {
                        if !used_b[j] {
                            return Some(j);
                        }
                    }
                    None
                });
            let found = by_content.map(|j| (j, Evidence::Content)).or_else(|| {
                b_by_meta
                    .get(&(folded_name(&f.rel), f.size))
                    .and_then(|list| {
                        list.iter().rev().copied().find(|&j| {
                            !used_b[j]
                                && (f.full.is_none() || fb[j].full.is_none())
                                && same_time(f.modified, fb[j].modified)
                        })
                    })
                    .map(|j| (j, Evidence::Metadata))
            });
            if let Some((j, evidence)) = found {
                used_a[i] = true;
                used_b[j] = true;
                items.push(CompareItem {
                    rel: f.rel.clone(),
                    other: Some(fb[j].rel.clone()),
                    size: f.size,
                    status: Status::Same,
                    evidence: Some(evidence),
                });
            }
        }
        // What is left at the same path differs.
        for (i, f) in fa.iter().enumerate() {
            if used_a[i] {
                continue;
            }
            if let Some(&j) = b_by_path.get(&names::fold(&f.rel))
                && !used_b[j]
            {
                let (_, evidence) = judge(f, &fb[j]);
                used_a[i] = true;
                used_b[j] = true;
                items.push(CompareItem {
                    rel: f.rel.clone(),
                    other: None,
                    size: f.size,
                    status: Status::Different,
                    evidence: Some(evidence),
                });
            }
        }
    }
    for (i, f) in fa.iter().enumerate().filter(|(i, _)| !used_a[*i]) {
        let _ = i;
        items.push(CompareItem {
            rel: f.rel.clone(),
            other: None,
            size: f.size,
            status: Status::OnlyA,
            evidence: None,
        });
    }
    for (j, f) in fb.iter().enumerate().filter(|(j, _)| !used_b[*j]) {
        let _ = j;
        items.push(CompareItem {
            rel: f.rel.clone(),
            other: None,
            size: f.size,
            status: Status::OnlyB,
            evidence: None,
        });
    }
    items.sort_by(|x, y| {
        x.rel
            .cmp(&y.rel)
            .then(x.status.as_str().cmp(y.status.as_str()))
    });
    Ok(CompareReport { items })
}

/// A subfolder in a level of the result tree, with totals by class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderNode {
    /// Stored name of the subfolder.
    pub name: String,
    pub tallies: Vec<Tally>,
}

impl FolderNode {
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.tallies.iter().map(|t| t.bytes).sum()
    }
}

/// One level of a result tree (F13-AC4, F14-AC3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderView {
    /// Subfolders, largest first.
    pub folders: Vec<FolderNode>,
    /// Positions (in the input) of the files directly in the folder.
    pub files: Vec<usize>,
    /// Totals of the whole folder.
    pub tallies: Vec<Tally>,
}

/// The level `folder` (relative path, `""` for the top) of a tree of items given as
/// `(relative path, size, class)` with `classes` classes (statuses).
pub fn folder_view<'a>(
    items: impl Iterator<Item = (&'a str, u64, usize)>,
    folder: &str,
    classes: usize,
) -> FolderView {
    let mut view = FolderView {
        tallies: vec![Tally::default(); classes],
        ..FolderView::default()
    };
    let mut index: HashMap<String, usize> = HashMap::new();
    for (pos, (rel, size, class)) in items.enumerate() {
        let rest = if folder.is_empty() {
            rel
        } else {
            match rel.strip_prefix(folder).and_then(|r| r.strip_prefix('\\')) {
                Some(r) => r,
                None => continue,
            }
        };
        let class = class.min(classes.saturating_sub(1));
        view.tallies[class].add(size);
        match rest.split_once('\\') {
            None => view.files.push(pos),
            Some((sub, _)) => {
                let k = *index.entry(sub.to_owned()).or_insert_with(|| {
                    view.folders.push(FolderNode {
                        name: sub.to_owned(),
                        tallies: vec![Tally::default(); classes],
                    });
                    view.folders.len() - 1
                });
                view.folders[k].tallies[class].add(size);
            }
        }
    }
    view.folders
        .sort_by(|a, b| b.bytes().cmp(&a.bytes()).then(a.name.cmp(&b.name)));
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_levels_with_totals() {
        let items = [
            (r"a\x.txt", 10, 0),
            (r"a\b\y.txt", 5, 1),
            (r"a\b\z.txt", 7, 0),
            (r"c.txt", 1, 1),
            (r"ab\w.txt", 100, 0),
        ];
        let top = folder_view(items.iter().copied(), "", 2);
        assert_eq!(top.files, [3]);
        assert_eq!(
            top.folders
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            ["ab", "a"]
        );
        assert_eq!(
            top.tallies[0],
            Tally {
                files: 3,
                bytes: 117
            }
        );
        let a = folder_view(items.iter().copied(), "a", 2);
        assert_eq!(a.files, [0]);
        assert_eq!(a.folders.len(), 1);
        assert_eq!(a.folders[0].tallies[1], Tally { files: 1, bytes: 5 });
        // "a" is not a prefix of "ab".
        assert_eq!(
            a.tallies[0],
            Tally {
                files: 2,
                bytes: 17
            }
        );
        assert!(
            folder_view(items.iter().copied(), r"a\b\nothing", 2)
                .files
                .is_empty()
        );
    }

    #[test]
    fn dates_within_two_seconds_are_the_same() {
        let t = FileTime(133_000_000_000_000_000);
        assert!(same_time(Some(t), Some(FileTime(t.0 + TIME_TOLERANCE))));
        assert!(!same_time(
            Some(t),
            Some(FileTime(t.0 + TIME_TOLERANCE + 1))
        ));
        assert!(!same_time(Some(t), None));
        assert_eq!(folded_name(r"Foto\IMG.JPG"), names::fold("img.jpg"));
    }
}
