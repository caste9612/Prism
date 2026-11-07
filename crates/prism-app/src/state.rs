//! Application state shared by commands, the volume monitor and the tray: the catalog store,
//! the job engine and the volumes connected now. No domain logic here: it lives in
//! `prism-core` and `prism-win` (ARCHITECTURE §2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::indexer::{self, IndexProgress, IndexRequest};
use prism_core::jobs::{JobContext, JobEngine, JobId, JobInfo, JobSpec, Priority};
use prism_core::model::{DisplayKind, Volume, VolumeId, VolumeKind, VolumeStatus};
use prism_core::reconcile::ReconcileTarget;
use prism_core::refresh;
use prism_core::scan::WalkEnumerator;
use prism_core::store::{self, Store};
use prism_core::volumes::DetectedVolume;
use prism_win::{autostart, ntfs};

use crate::dto::{ApiError, JobView, Settings, VolumeView};
use crate::netprobe::NetProbe;

pub struct AppState {
    pub store: Store,
    pub engine: JobEngine,
    /// Volumes connected now, by catalog id (updated by the monitor).
    pub online: Mutex<HashMap<VolumeId, DetectedVolume>>,
    pub elevated: bool,
    pub data_dir: PathBuf,
    pub dev: bool,
    /// Reachability and free space of network shares (REVIEW L5).
    pub net: Arc<NetProbe>,
}

pub fn job_view(j: &JobInfo) -> JobView {
    JobView {
        id: j.id.0,
        kind: j.kind.clone(),
        volume_id: j.volume.map(|v| v.0),
        state: j.state.as_str().to_owned(),
        done: j.done,
        total: j.total,
        phase: j.phase.clone(),
        error: j.error.clone(),
    }
}

fn kind_name(v: &Volume) -> String {
    if v.info.fs_serial.is_none() && v.info.unc_path.is_none() {
        return "folder".into();
    }
    match v.info.display_kind() {
        DisplayKind::Internal => "internal",
        DisplayKind::External => "external",
        DisplayKind::Stick => "stick",
        DisplayKind::Network => "network",
    }
    .into()
}

impl AppState {
    pub fn online(&self) -> std::sync::MutexGuard<'_, HashMap<VolumeId, DetectedVolume>> {
        self.online.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The catalog screen: every volume with its labels, state, totals and current job.
    pub fn volume_views(&self) -> Result<Vec<VolumeView>, ApiError> {
        let rows = self.store.read(|c| {
            let mut out = Vec::new();
            for v in store::volumes(c)? {
                let totals = match store::root_entry(c, v.id)? {
                    Some(root) => store::dir_stats(c, root.id)?.unwrap_or_default(),
                    None => prism_core::model::DirStats::default(),
                };
                out.push((v, totals));
            }
            Ok(out)
        })?;
        let jobs = self.engine.jobs();
        let online = self.online();
        Ok(rows
            .into_iter()
            .map(|(v, totals)| {
                let detected = online.get(&v.id);
                let job = jobs
                    .iter()
                    .filter(|j| {
                        j.volume == Some(v.id) && !j.state.is_finished() && shown_on_card(&j.kind)
                    })
                    .min_by_key(|j| j.id)
                    .map(job_view);
                let mount = detected
                    .and_then(|d| d.info.last_mount.clone())
                    .or_else(|| v.info.last_mount.clone());
                VolumeView {
                    id: v.id.0,
                    name: v
                        .labels
                        .name
                        .clone()
                        .filter(|n| !n.trim().is_empty())
                        .or_else(|| v.info.os_label.clone().filter(|n| !n.trim().is_empty()))
                        .or_else(|| mount.clone())
                        .unwrap_or_else(|| format!("#{}", v.id.0)),
                    kind: kind_name(&v),
                    user_name: v.labels.name.clone(),
                    location: v.labels.location.clone(),
                    notes: v.labels.notes.clone(),
                    color: v.labels.color.clone(),
                    os_label: v.info.os_label.clone(),
                    fs_type: v.info.fs_type.clone(),
                    mount,
                    online: detected.is_some(),
                    status: v.status.as_str().to_owned(),
                    capacity: detected.and_then(|d| d.info.capacity).or(v.info.capacity),
                    free_space: detected
                        .and_then(|d| d.info.free_space)
                        .or(v.info.free_space),
                    indexed_size: totals.total_size,
                    file_count: totals.file_count,
                    folder_count: totals.dir_count,
                    last_indexed_at: v.last_indexed_at,
                    last_seen_at: v.last_seen_at,
                    job,
                }
            })
            .collect())
    }

    fn detected(&self, volume: VolumeId) -> Result<DetectedVolume, ApiError> {
        self.online()
            .get(&volume)
            .cloned()
            .ok_or_else(|| ApiError::new("volume_offline", format!("volume {volume}")))
    }

    /// Queues an indexing or an update of an online volume; if a job already runs on it,
    /// returns that one.
    pub fn submit_index(&self, volume: VolumeId, kind: IndexKind) -> Result<JobId, ApiError> {
        if let Some(active) = self.engine.active_for(volume).first() {
            return Ok(active.id);
        }
        let detected = self.detected(volume)?;
        let store = self.store.clone();
        let elevated = self.elevated;
        let update = kind != IndexKind::Index;
        Ok(self.engine.submit(JobSpec {
            kind: kind.as_str().to_owned(),
            volume: Some(volume),
            priority: Priority::Indexing,
            disks: job_disks(&detected),
            run: Box::new(move |ctx| run_index(&store, volume, &detected, elevated, update, ctx)),
        }))
    }

    /// Queues a refresh of the folders where a watcher saw changes (REVIEW L6); `rescan`
    /// re-reads the whole volume.
    pub fn submit_refresh(
        &self,
        volume: VolumeId,
        changed: Vec<Vec<String>>,
        rescan: bool,
    ) -> Result<JobId, ApiError> {
        let detected = self.detected(volume)?;
        let root = access_path(&detected)
            .ok_or_else(|| ApiError::new("volume_offline", format!("volume {volume}")))?;
        let store = self.store.clone();
        Ok(self.engine.submit(JobSpec {
            kind: "refresh".to_owned(),
            volume: Some(volume),
            priority: Priority::Indexing,
            disks: job_disks(&detected),
            run: Box::new(move |ctx| {
                refresh::refresh_folders(
                    &store,
                    volume,
                    &root,
                    &WalkEnumerator::default(),
                    &ExclusionPolicy::with_defaults(),
                    &changed,
                    rescan,
                    &ctx.cancel,
                    &IndexProgress::default(),
                )
                .map(|_| ())
                .map_err(|e| e.to_string())
            }),
        }))
    }
}

/// Key of the user settings (F21) in the `settings` table: one JSON document.
pub const SETTINGS_KEY: &str = "app.settings";

impl AppState {
    /// The user settings; defaults when never saved or unreadable.
    pub fn settings(&self) -> Settings {
        self.store
            .read(|c| store::setting(c, SETTINGS_KEY))
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Registers or removes the start-with-Windows task (F19-AC3). Development builds only
    /// keep the setting: a task starting `target\debug\prism.exe` at every logon would be a
    /// surprise (REVIEW D18).
    pub fn apply_autostart(&self, enabled: bool) -> Result<(), ApiError> {
        if self.dev {
            tracing::info!(
                enabled,
                "start with Windows not changed in development builds"
            );
            return Ok(());
        }
        let exe = std::env::current_exe().map_err(|e| ApiError::new("autostart_failed", e))?;
        autostart::apply(enabled, &exe, crate::AUTOSTART_ARG)
            .map_err(|e| ApiError::new("autostart_failed", e))
    }
}

/// Kinds of indexing job. `journal` and `refresh` are the automatic background updates: the
/// interface does not show them on the cards (`ui/src/lib/catalog.ts`, `cardJob`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexKind {
    /// First indexing, asked by the user.
    Index,
    /// Update asked by the user or done when a volume comes back.
    Update,
    /// Periodic update from the NTFS change journal.
    Journal,
}

impl IndexKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::Update => "update",
            Self::Journal => "journal",
        }
    }
}

/// Whether a job of this kind is shown on the volume card.
pub fn shown_on_card(kind: &str) -> bool {
    matches!(kind, "index" | "update")
}

/// The physical resources a job on this volume keeps busy (one heavy job per disk, ADR-0009).
fn job_disks(d: &DetectedVolume) -> Vec<String> {
    if d.info.kind == VolumeKind::Network {
        vec![format!(
            "net:{}",
            d.info.unc_path.clone().unwrap_or_default()
        )]
    } else {
        d.disks.iter().map(|n| format!("disk{n}")).collect()
    }
}

/// Root path through which a volume is read: the UNC path for network shares (an elevated
/// process does not see the user's mapped letters, ADR-0016), the mount otherwise.
pub fn access_path(d: &DetectedVolume) -> Option<PathBuf> {
    match d.info.kind {
        VolumeKind::Network => d.info.unc_path.as_ref().map(PathBuf::from),
        _ => d.info.last_mount.as_ref().map(PathBuf::from),
    }
}

fn run_index(
    store: &Store,
    volume: VolumeId,
    detected: &DetectedVolume,
    elevated: bool,
    update: bool,
    ctx: &JobContext,
) -> Result<(), String> {
    let root = access_path(detected).ok_or_else(|| "the volume has no mount".to_owned())?;
    let policy = ExclusionPolicy::with_defaults();
    let ntfs = elevated
        && detected.info.kind != VolumeKind::Network
        && detected
            .info
            .fs_type
            .as_deref()
            .is_some_and(|f| f.eq_ignore_ascii_case("NTFS"));
    let progress = IndexProgress::default();
    let finished = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        // Mirror the indexing counters into the job progress for the interface.
        s.spawn(|| {
            while !finished.load(Ordering::Relaxed) {
                let scanned = progress.scanned.load(Ordering::Relaxed);
                let written = progress.reconciled.load(Ordering::Relaxed);
                if written > 0 {
                    ctx.progress.set_phase("writing");
                    ctx.progress.done.store(written, Ordering::Relaxed);
                    ctx.progress
                        .total
                        .store(scanned.max(written), Ordering::Relaxed);
                } else {
                    ctx.progress.set_phase("scanning");
                    ctx.progress.done.store(scanned, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        let result = index_with(
            store,
            volume,
            &root,
            &policy,
            ntfs,
            update,
            &ctx.cancel,
            &progress,
        );
        finished.store(true, Ordering::Relaxed);
        result
    })
}

#[allow(clippy::too_many_arguments)]
fn index_with(
    store: &Store,
    volume: VolumeId,
    root: &Path,
    policy: &ExclusionPolicy,
    ntfs: bool,
    update: bool,
    cancel: &CancelToken,
    progress: &IndexProgress,
) -> Result<(), String> {
    if ntfs {
        if update {
            ntfs::update_ntfs(store, volume, root, policy, cancel, progress)
                .map(|_| ())
                .map_err(|e| e.to_string())
        } else {
            ntfs::index_ntfs(store, volume, root, policy, cancel, progress)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    } else {
        indexer::index(
            store,
            &WalkEnumerator::default(),
            &IndexRequest {
                volume,
                path: root,
                relative: &[],
                target: ReconcileTarget::VolumeRoot,
                policy,
            },
            cancel,
            progress,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }
}

/// Whether a volume should be kept up to date automatically (indexed and not ignored).
pub fn auto_update(status: VolumeStatus) -> bool {
    matches!(status, VolumeStatus::Indexed | VolumeStatus::ReindexNeeded)
}
