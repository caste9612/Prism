//! Indexing and updating a volume: enumeration, then reconciliation, with the volume status
//! kept truthful at every step (F04-AC6).
//!
//! - The enumeration does not write anything: if it fails or is cancelled, the catalog and the
//!   volume status are left exactly as they were.
//! - While the reconciler writes, the volume is `indexing`. If the reconciliation fails or is
//!   cancelled, the volume becomes `reindex_needed`. A crash leaves it `indexing`, which the
//!   next [`Store::open`] turns into `reindex_needed`.
//! - On success the volume is `indexed`, with the indexing time.

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use crate::cancel::CancelToken;
use crate::exclusion::ExclusionPolicy;
use crate::model::{VolumeId, VolumeStatus};
use crate::reconcile::{self, ReconcileError, ReconcileStats, ReconcileTarget};
use crate::scan::{Enumerator, ScanError, ScanRequest, ScanStats};
use crate::store::{self, Store, StoreError};

/// Live counters, readable from another thread while indexing runs.
#[derive(Debug, Default)]
pub struct IndexProgress {
    /// Entries found by the enumeration so far.
    pub scanned: AtomicU64,
    /// Entries processed by the reconciler so far.
    pub reconciled: AtomicU64,
}

/// What an indexing or update did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexReport {
    pub scan: ScanStats,
    pub reconcile: ReconcileStats,
    pub scan_time: Duration,
    pub reconcile_time: Duration,
}

/// Errors of an indexing or update.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error(transparent)]
    Scan(#[from] ScanError),
    #[error(transparent)]
    Reconcile(#[from] ReconcileError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl IndexError {
    /// Whether the operation stopped because it was cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Scan(ScanError::Cancelled(_)) | Self::Reconcile(ReconcileError::Cancelled(_))
        )
    }
}

/// What to enumerate and where it goes in the catalog.
pub struct IndexRequest<'a> {
    pub volume: VolumeId,
    /// Absolute path of the folder to enumerate.
    pub path: &'a Path,
    /// Components of `path` relative to the volume root (empty for the whole volume).
    pub relative: &'a [String],
    /// [`ReconcileTarget::VolumeRoot`] for the whole volume, or the catalog folder that
    /// corresponds to `path`.
    pub target: ReconcileTarget,
    pub policy: &'a ExclusionPolicy,
}

/// Enumerates `request.path` and reconciles the result into the catalog.
///
/// # Errors
/// [`IndexError`]; see the module documentation for the state left in each case.
pub fn index(
    store: &Store,
    enumerator: &dyn Enumerator,
    request: &IndexRequest<'_>,
    cancel: &CancelToken,
    progress: &IndexProgress,
) -> Result<IndexReport, IndexError> {
    let volume = request.volume;
    let scan_started = Instant::now();
    let tree = enumerator.scan(&ScanRequest {
        root: request.path,
        relative: request.relative,
        policy: request.policy,
        cancel,
        progress: &progress.scanned,
    })?;
    let scan_time = scan_started.elapsed();
    tracing::info!(
        volume = volume.0,
        entries = tree.len(),
        unreadable = tree.stats.unreadable_dirs,
        ms = scan_time.as_millis(),
        "enumeration finished"
    );

    store.write(move |w| store::set_volume_status(w, volume, VolumeStatus::Indexing))?;
    let reconcile_started = Instant::now();
    match reconcile::reconcile(
        store,
        volume,
        request.target,
        &tree,
        cancel,
        &progress.reconciled,
    ) {
        Ok(stats) => {
            store.write(move |w| store::set_volume_status(w, volume, VolumeStatus::Indexed))?;
            let reconcile_time = reconcile_started.elapsed();
            tracing::info!(
                volume = volume.0,
                ?stats,
                ms = reconcile_time.as_millis(),
                "reconciliation finished"
            );
            Ok(IndexReport {
                scan: tree.stats,
                reconcile: stats,
                scan_time,
                reconcile_time,
            })
        }
        Err(e) => {
            tracing::warn!(volume = volume.0, error = %e, "reconciliation did not complete");
            store
                .write(move |w| store::set_volume_status(w, volume, VolumeStatus::ReindexNeeded))?;
            Err(e.into())
        }
    }
}
