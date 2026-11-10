//! Parallel directory walker (ADR-0006): the enumerator for non-NTFS volumes, network drives
//! and every case where the MFT reader is not available (e.g. without administrator rights).
//!
//! Metadata comes from the directory enumeration itself: on Windows `DirEntry::metadata()` is
//! filled from `FindNextFileW` data, so there is **no extra system call per file** (on SMB
//! every extra call is a network round trip).
//!
//! Folders that are name-surrogate reparse points (junctions, symbolic links, mount points)
//! are recorded but not descended: following them would count the same data twice and can
//! loop. Other reparse points (cloud placeholders, deduplicated files) are normal entries.
//!
//! The metadata of each folder itself is read with one `metadata()` call when the folder is
//! listed (one call per folder, not per file): NTFS updates the copy of a folder's dates kept
//! in its parent's index lazily, so the value seen by the parent listing can be stale and two
//! scans of an unchanged tree would disagree.
//!
//! Paths are used in verbatim form (`\\?\C:\...`): Win32 path normalization would strip
//! trailing dots and spaces, which are valid in NTFS names (created by WSL or macOS), and
//! make those folders unreadable.
//!
//! Structure: worker threads only list folders; a single collector (the calling thread)
//! builds the tree and hands out the subfolders to list.

use std::fs::{self, DirEntry, Metadata};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender};

use super::{
    Enumerator, NodeId, ScanChild, ScanError, ScanRequest, ScanStats, ScanTree, ScanTreeBuilder,
};
use crate::exclusion::{ChildCheck, ExclusionPolicy, RuleCursor};
use crate::model::{ATTR_DIRECTORY, EntryMeta, FileTime};
use crate::names;

/// Enumerates folders with `std::fs::read_dir` on several threads.
#[derive(Clone, Debug)]
pub struct WalkEnumerator {
    threads: usize,
}

impl Default for WalkEnumerator {
    fn default() -> Self {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
        Self { threads }
    }
}

impl WalkEnumerator {
    /// A walker using `threads` listing threads (at least 1).
    #[must_use]
    pub fn with_threads(threads: usize) -> Self {
        Self {
            threads: threads.max(1),
        }
    }

    #[must_use]
    pub fn threads(&self) -> usize {
        self.threads
    }
}

struct Job {
    node: NodeId,
    path: PathBuf,
    cursor: RuleCursor,
}

struct FoundChild {
    child: ScanChild,
    /// For folders to descend: their path and exclusion cursor.
    descend: Option<(PathBuf, RuleCursor)>,
}

struct ListingResult {
    node: NodeId,
    /// Metadata of the folder itself, read from the folder (not from its parent's index).
    own_meta: Option<EntryMeta>,
    children: Vec<FoundChild>,
    excluded: u64,
    error: Option<String>,
}

/// Whether `path` is a folder and its metadata as the walker records them, read from the item
/// itself without following a final link: what a catalog entry is compared with before acting
/// on the disk (ADR-0012).
///
/// # Errors
/// I/O errors (`NotFound` if the item is gone).
pub fn disk_meta(path: &Path) -> std::io::Result<(bool, EntryMeta)> {
    Ok(meta_from(&std::fs::symlink_metadata(path)?))
}

#[cfg(windows)]
fn filetime(raw: u64) -> Option<FileTime> {
    (raw != 0).then(|| FileTime(i64::try_from(raw).unwrap_or(i64::MAX)))
}

#[cfg(windows)]
fn meta_from(md: &Metadata) -> (bool, EntryMeta) {
    use std::os::windows::fs::MetadataExt;
    let attrs = md.file_attributes();
    let is_dir = attrs & ATTR_DIRECTORY != 0;
    let meta = EntryMeta {
        size: if is_dir { 0 } else { md.file_size() },
        alloc_size: None,
        created: filetime(md.creation_time()),
        modified: filetime(md.last_write_time()),
        attrs,
        file_ref: None,
    };
    (is_dir, meta)
}

#[cfg(not(windows))]
fn meta_from(md: &Metadata) -> (bool, EntryMeta) {
    let to_ft = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .map(FileTime::from_unix_millis)
    };
    let is_dir = md.is_dir();
    let meta = EntryMeta {
        size: if is_dir { 0 } else { md.len() },
        alloc_size: None,
        created: to_ft(md.created()),
        modified: to_ft(md.modified()),
        attrs: if is_dir { ATTR_DIRECTORY } else { 0 },
        file_ref: None,
    };
    (is_dir, meta)
}

fn read_child(
    entry: &DirEntry,
    dir_path: &Path,
    cursor: &RuleCursor,
    policy: &ExclusionPolicy,
) -> std::io::Result<Option<FoundChild>> {
    let os_name = entry.file_name();
    let name = names::encode(&os_name);
    let file_type = entry.file_type()?;
    let md = entry.metadata()?;
    let (is_dir, meta) = meta_from(&md);
    let next_cursor = match policy.child(cursor, &name, is_dir) {
        ChildCheck::Excluded => return Ok(None),
        ChildCheck::Included(c) => c,
    };
    // Junctions and directory symlinks are folders for the catalog but are not followed.
    let descend =
        (is_dir && !file_type.is_symlink()).then(|| (dir_path.join(&os_name), next_cursor));
    Ok(Some(FoundChild {
        child: ScanChild { name, is_dir, meta },
        descend,
    }))
}

fn folder_meta(md: &Metadata) -> EntryMeta {
    let (_, mut meta) = meta_from(md);
    meta.size = 0;
    meta
}

fn list_dir(job: &Job, policy: &ExclusionPolicy) -> ListingResult {
    let mut result = ListingResult {
        node: job.node,
        own_meta: fs::metadata(&job.path).ok().map(|md| folder_meta(&md)),
        children: Vec::new(),
        excluded: 0,
        error: None,
    };
    let reader = match fs::read_dir(&job.path) {
        Ok(r) => r,
        Err(e) => {
            result.error = Some(format!("{}: {e}", job.path.display()));
            return result;
        }
    };
    for item in reader {
        let entry = match item {
            Ok(e) => e,
            Err(e) => {
                result.error = Some(format!("{}: {e}", job.path.display()));
                break;
            }
        };
        match read_child(&entry, &job.path, &job.cursor, policy) {
            Ok(Some(found)) => result.children.push(found),
            Ok(None) => result.excluded += 1,
            Err(e) => {
                result.error = Some(format!("{}: {e}", entry.path().display()));
            }
        }
    }
    result
}

fn worker(jobs: &Receiver<Job>, results: &Sender<ListingResult>, request: &ScanRequest<'_>) {
    for job in jobs {
        // Every job gets exactly one answer, so the collector never waits forever; once
        // cancelled, folders are answered without being read.
        let result = if request.cancel.is_cancelled() {
            ListingResult {
                node: job.node,
                own_meta: None,
                children: Vec::new(),
                excluded: 0,
                error: None,
            }
        } else {
            list_dir(&job, request.policy)
        };
        if results.send(result).is_err() {
            return;
        }
    }
}

/// The root of a request: its metadata, its verbatim path and its exclusion cursor.
fn open_root(request: &ScanRequest<'_>) -> Result<(Metadata, PathBuf, RuleCursor), ScanError> {
    let unreadable = |source| ScanError::RootUnreadable {
        path: request.root.to_path_buf(),
        source,
    };
    let root_md = fs::metadata(request.root).map_err(unreadable)?;
    if !root_md.is_dir() {
        return Err(ScanError::RootNotADirectory(request.root.to_path_buf()));
    }
    // Verbatim form (\\?\...), so that names ending with a dot or a space work.
    let root_path = fs::canonicalize(request.root).map_err(unreadable)?;
    let root_cursor = request
        .policy
        .cursor_for(request.relative)
        .ok_or_else(|| ScanError::RootExcluded(request.root.to_path_buf()))?;
    Ok((root_md, root_path, root_cursor))
}

impl Enumerator for WalkEnumerator {
    fn scan(&self, request: &ScanRequest<'_>) -> Result<ScanTree, ScanError> {
        let started = Instant::now();
        let (root_md, root_path, root_cursor) = open_root(request)?;

        let mut builder = ScanTreeBuilder::new(folder_meta(&root_md));
        let mut stats = ScanStats {
            dirs: 1,
            ..ScanStats::default()
        };

        let (job_tx, job_rx) = crossbeam_channel::unbounded::<Job>();
        let (res_tx, res_rx) = crossbeam_channel::unbounded::<ListingResult>();
        let (cancelled, incomplete) = std::thread::scope(|scope| {
            for i in 0..self.threads {
                let jobs = job_rx.clone();
                let results = res_tx.clone();
                let spawned = std::thread::Builder::new()
                    .name(format!("prism-walk-{i}"))
                    .spawn_scoped(scope, move || worker(&jobs, &results, request));
                if let Err(e) = spawned {
                    tracing::warn!(error = %e, "cannot start a walker thread");
                }
            }
            drop(res_tx);

            let mut outstanding: u64 = 1;
            let _ = job_tx.send(Job {
                node: ScanTree::ROOT,
                path: root_path,
                cursor: root_cursor,
            });
            let mut cancelled = false;
            while outstanding > 0 {
                let Ok(listing) = res_rx.recv() else {
                    break; // every worker failed to start or exited
                };
                outstanding -= 1;
                if request.cancel.is_cancelled() {
                    cancelled = true;
                    break;
                }
                stats.excluded += listing.excluded;
                if let Some(meta) = listing.own_meta.filter(|_| listing.node != ScanTree::ROOT) {
                    builder.set_meta(listing.node, meta);
                }
                if let Some(err) = listing.error {
                    stats.unreadable_dirs += 1;
                    stats.push_error(err);
                    builder.mark_unreadable(listing.node);
                }
                let count = listing.children.len() as u64;
                let mut descend = Vec::new();
                let range = builder.add_listing(
                    listing.node,
                    listing.children.into_iter().map(|found| {
                        if found.child.is_dir {
                            stats.dirs += 1;
                        } else {
                            stats.files += 1;
                        }
                        descend.push(found.descend);
                        found.child
                    }),
                );
                for (node, d) in range.zip(descend) {
                    if let Some((path, cursor)) = d {
                        outstanding += 1;
                        let _ = job_tx.send(Job { node, path, cursor });
                    }
                }
                request.progress.fetch_add(count, Ordering::Relaxed);
            }
            drop(job_tx);
            (cancelled, outstanding > 0)
        });
        if cancelled || request.cancel.is_cancelled() {
            return Err(ScanError::Cancelled(crate::cancel::Cancelled));
        }
        if incomplete {
            return Err(ScanError::Internal(
                "the walker threads stopped before the end of the scan".into(),
            ));
        }
        stats.elapsed = started.elapsed();
        Ok(builder.finish(stats))
    }

    /// One listing on the calling thread: the subfolders are not read.
    fn list(&self, request: &ScanRequest<'_>) -> Result<ScanTree, ScanError> {
        let started = Instant::now();
        let (root_md, path, cursor) = open_root(request)?;
        request.cancel.check()?;
        let listing = list_dir(
            &Job {
                node: ScanTree::ROOT,
                path,
                cursor,
            },
            request.policy,
        );
        let mut builder = ScanTreeBuilder::new(folder_meta(&root_md));
        let mut stats = ScanStats {
            dirs: 1,
            excluded: listing.excluded,
            ..ScanStats::default()
        };
        if let Some(err) = listing.error {
            stats.unreadable_dirs += 1;
            stats.push_error(err);
            builder.mark_unreadable(ScanTree::ROOT);
        }
        let count = listing.children.len() as u64;
        let mut descend = Vec::new();
        let range = builder.add_listing(
            ScanTree::ROOT,
            listing.children.into_iter().map(|found| {
                if found.child.is_dir {
                    stats.dirs += 1;
                } else {
                    stats.files += 1;
                }
                descend.push(found.descend.is_some());
                found.child
            }),
        );
        for (node, d) in range.zip(descend) {
            if d {
                builder.mark_not_listed(node);
            }
        }
        request.progress.fetch_add(count, Ordering::Relaxed);
        stats.elapsed = started.elapsed();
        Ok(builder.finish(stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_a_missing_folder_reports_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let job = Job {
            node: 3,
            path: dir.path().join("missing"),
            cursor: RuleCursor::default(),
        };
        let result = list_dir(&job, &ExclusionPolicy::default());
        assert_eq!(result.node, 3);
        assert!(result.error.is_some());
        assert!(result.children.is_empty());
        assert!(result.own_meta.is_none());
    }
}
