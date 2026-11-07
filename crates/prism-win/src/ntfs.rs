//! Indexing and updating NTFS volumes: full read of the MFT, then incremental updates from
//! the change journal (ADR-0017, ADR-0018). Needs administrator rights.
//!
//! - [`index_ntfs`]: reads the journal position, indexes the volume through the MFT, and saves
//!   the position on success, so that every change made during the read is applied later.
//! - [`update_ntfs`]: reads the journal from the saved position and applies the changed files;
//!   falls back to [`index_ntfs`] when the journal cannot be used (no position, journal
//!   recreated or overwritten, changes that cannot be applied incrementally).

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use prism_core::cancel::CancelToken;
use prism_core::changes::{self, ApplyOutcome, ChangeSource, ChangedFile, ChangedName, FileChange};
use prism_core::exclusion::ExclusionPolicy;
use prism_core::indexer::{self, IndexError, IndexProgress, IndexReport, IndexRequest};
use prism_core::model::{UsnPosition, VolumeId, VolumeStatus};
use prism_core::reconcile::{ReconcileStats, ReconcileTarget};
use prism_core::store::{self, Store, StoreError};

use crate::mft::{self, FetchError, FileState, MftEnumerator};
use crate::usn::{self, REASON_FILE_CREATE, UsnError};

/// Records read per update at most; the rest is read by the next update.
const MAX_RECORDS: usize = 1_000_000;
/// Records below this number are NTFS metadata files, never in the catalog.
const FIRST_USER_RECORD: u64 = 24;
const RECORD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Errors of NTFS indexing and updates.
#[derive(Debug, thiserror::Error)]
pub enum NtfsError {
    #[error("cannot open the volume {path}: {source}")]
    Open {
        path: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Apply(#[from] changes::ApplyError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Usn(#[from] UsnError),
    /// A file could not be read consistently yet: nothing was changed, try again later.
    #[error("the update must be retried later: {0}")]
    Retry(String),
    #[error("{0} is not a drive letter or volume path")]
    NotAVolume(String),
}

/// What an update did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// Applied from the journal: number of records read and catalog changes.
    Journal {
        records: usize,
        stats: ReconcileStats,
    },
    /// The journal could not be used; the volume was indexed again.
    Reindexed { reason: String, report: IndexReport },
}

/// `\\.\X:` for a root on drive X (plain or verbatim).
fn device(root: &Path) -> Result<String, NtfsError> {
    let s = root.to_string_lossy();
    let rest = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let b = rest.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        Ok(format!(r"\\.\{}:", &rest[..1]))
    } else {
        Err(NtfsError::NotAVolume(s.into_owned()))
    }
}

fn open_volume(root: &Path) -> Result<File, NtfsError> {
    let path = device(root)?;
    File::open(&path).map_err(|source| NtfsError::Open { path, source })
}

/// Indexes the whole volume rooted at `root` through the MFT and saves the journal position
/// (cleared if the volume has no journal or the indexing fails).
///
/// # Errors
/// [`NtfsError`]; the volume status follows [`indexer::index`].
pub fn index_ntfs(
    store: &Store,
    volume: VolumeId,
    root: &Path,
    policy: &ExclusionPolicy,
    cancel: &CancelToken,
    progress: &IndexProgress,
) -> Result<IndexReport, NtfsError> {
    let handle = open_volume(root)?;
    // Read before the MFT: changes made during the read are replayed by the next update.
    let journal = usn::query_journal(&handle)?;
    let result = indexer::index(
        store,
        &MftEnumerator,
        &IndexRequest {
            volume,
            path: root,
            relative: &[],
            target: ReconcileTarget::VolumeRoot,
            policy,
        },
        cancel,
        progress,
    );
    let position = match (&result, journal) {
        (Ok(_), Some(j)) => Some(UsnPosition {
            journal_id: j.id,
            next_usn: j.next_usn,
        }),
        _ => None,
    };
    store.write(move |w| store::set_usn_position(w, volume, position))?;
    Ok(result?)
}

/// Reads files through `FSCTL_GET_NTFS_FILE_RECORD` for the change applier.
struct FetchSource<'a> {
    handle: &'a File,
}

fn to_changed(state: &FileState) -> ChangedFile {
    ChangedFile {
        is_dir: state.is_dir,
        names: (0..state.names.len())
            .map(|i| ChangedName {
                parent_ref: state.names[i].0,
                name: state.name(i),
                meta: state.meta(i),
            })
            .collect(),
    }
}

impl ChangeSource for FetchSource<'_> {
    fn current(&self, file_ref: u64) -> Result<Option<ChangedFile>, String> {
        mft::fetch_file(self.handle, file_ref)
            .map(|s| s.as_ref().map(to_changed))
            .map_err(|e| e.to_string())
    }
}

/// Updates the catalog of an NTFS volume from its change journal (ADR-0018), or indexes it
/// again when the journal cannot be used.
///
/// # Errors
/// [`NtfsError::Retry`] if a file is not consistent on the disk yet (nothing changed, the
/// position is kept); other [`NtfsError`]s, after which the volume needs reindexing.
pub fn update_ntfs(
    store: &Store,
    volume: VolumeId,
    root: &Path,
    policy: &ExclusionPolicy,
    cancel: &CancelToken,
    progress: &IndexProgress,
) -> Result<UpdateOutcome, NtfsError> {
    let reindex = |reason: String| -> Result<UpdateOutcome, NtfsError> {
        let report = index_ntfs(store, volume, root, policy, cancel, progress)?;
        Ok(UpdateOutcome::Reindexed { reason, report })
    };
    let saved = store
        .read(|c| store::volume(c, volume))?
        .and_then(|v| v.usn.filter(|_| v.status == VolumeStatus::Indexed));
    let Some(saved) = saved else {
        return reindex("no saved journal position".into());
    };
    let handle = open_volume(root)?;
    match usn::check_position(&handle, saved.journal_id, saved.next_usn) {
        Ok(_) => {}
        Err(UsnError::Lost | UsnError::NotActive) => {
            return reindex("the journal no longer covers the saved position".into());
        }
        Err(e) => return Err(e.into()),
    }
    let started = std::time::Instant::now();
    let batch = usn::read_journal(&handle, saved.journal_id, saved.next_usn, MAX_RECORDS)?;
    let records = batch.records.len();
    let read = started.elapsed();

    // One change per file: created if any record says so, every parent named by a record.
    let mut grouped: BTreeMap<u64, (bool, Vec<u64>)> = BTreeMap::new();
    for r in &batch.records {
        if r.file_ref & RECORD_MASK < FIRST_USER_RECORD {
            continue;
        }
        let entry = grouped.entry(r.file_ref).or_insert((false, Vec::new()));
        entry.0 |= r.reason & REASON_FILE_CREATE != 0;
        if !entry.1.contains(&r.parent_ref) {
            entry.1.push(r.parent_ref);
        }
    }
    let mut changes = Vec::with_capacity(grouped.len());
    for (file_ref, (created, parents_seen)) in grouped {
        cancel
            .check()
            .map_err(|e| NtfsError::Retry(e.to_string()))?;
        let state = match mft::fetch_file(&handle, file_ref) {
            Ok(state) => state.as_ref().map(to_changed),
            Err(FetchError::Stale(_)) => {
                return Err(NtfsError::Retry(format!(
                    "file {file_ref:#x} is not on the disk yet"
                )));
            }
            Err(e) => return Err(NtfsError::Retry(e.to_string())),
        };
        changes.push(FileChange {
            file_ref,
            created,
            parents_seen,
            state,
        });
    }

    tracing::debug!(
        records,
        files = changes.len(),
        read_ms = read.as_millis(),
        fetch_ms = (started.elapsed() - read).as_millis(),
        "journal read"
    );
    store.write(move |w| store::set_volume_status(w, volume, VolumeStatus::Indexing))?;
    let applied = changes::apply_changes(
        store,
        volume,
        policy,
        &changes,
        &FetchSource { handle: &handle },
        cancel,
        &progress.reconciled,
    );
    match applied {
        Ok(ApplyOutcome::Applied(stats)) => {
            let position = UsnPosition {
                journal_id: saved.journal_id,
                next_usn: batch.next_usn,
            };
            store.write(move |w| {
                store::set_volume_status(w, volume, VolumeStatus::Indexed)?;
                store::set_usn_position(w, volume, Some(position))
            })?;
            Ok(UpdateOutcome::Journal { records, stats })
        }
        Ok(ApplyOutcome::FullReconcileNeeded(reason)) => {
            store.write(move |w| store::set_volume_status(w, volume, VolumeStatus::Indexed))?;
            reindex(reason)
        }
        Err(e) => {
            store.write(move |w| {
                store::set_volume_status(w, volume, VolumeStatus::ReindexNeeded)?;
                store::set_usn_position(w, volume, None)
            })?;
            Err(e.into())
        }
    }
}
