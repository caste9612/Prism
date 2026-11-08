//! Application state shared by commands, the volume monitor and the tray: the catalog store,
//! the job engine and the volumes connected now. No domain logic here: it lives in
//! `prism-core` and `prism-win` (ARCHITECTURE §2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use prism_core::cancel::CancelToken;
use prism_core::cleanup::{self, Category};
use prism_core::duplicates::{self, LastCopyError};
use prism_core::exclusion::ExclusionPolicy;
use prism_core::fileops::{self, DeleteRequest, Disposal, FolderCheck};
use prism_core::indexer::{self, IndexProgress, IndexRequest};
use prism_core::jobs::{JobContext, JobEngine, JobId, JobInfo, JobSpec, Priority};
use prism_core::model::{DisplayKind, EntryId, Volume, VolumeId, VolumeKind, VolumeStatus};
use prism_core::names;
use prism_core::reconcile::ReconcileTarget;
use prism_core::refresh;
use prism_core::sandbox::SandboxGuard;
use prism_core::scan::WalkEnumerator;
use prism_core::search::engine::{self, SearchError, SearchOptions, VolumeMeta};
use prism_core::search::live::LiveIndexes;
use prism_core::search::query::{self, Clock};
use prism_core::space::{self, StatsScope, TreemapOptions};
use prism_core::store::StoreError;
use prism_core::store::{self, Store};
use prism_core::volumes::DetectedVolume;
use prism_win::trash::ShellRemover;
use prism_win::{autostart, ntfs};

use crate::cleanup_views::{self, CleanupResultView, CleanupView, RecycleBinView};
use crate::dto::{ApiError, HashesView, JobView, Settings, VolumeView};
use crate::duplicates_views::{self, DupFilterInput, DuplicatesView, RuleInput};
use crate::hash_jobs::HashingState;
use crate::netprobe::NetProbe;
use crate::notices::Notices;
use crate::search_views::{self, QueryErrorView, SearchResultView};
use crate::space_views::{CrumbView, LargeFileView, MAX_ITEMS, SpaceFolderView, TypeStatsView};

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
    /// Names of the servers of network shares, to find them at a new address (REVIEW D59).
    pub names: Arc<crate::netnames::ServerNames>,
    /// New disks waiting to be announced while the window is hidden (REVIEW L8).
    pub notices: Mutex<Notices>,
    /// Identifier under which notifications are shown (`prism_win::notify`).
    pub app_id: String,
    /// The search indexes of the volumes, kept in step with the catalog (ADR-0010).
    pub search: Arc<LiveIndexes>,
    /// Why the search shortcut is not registered, if it is not (F07-AC1).
    pub shortcut_error: Mutex<Option<String>>,
    /// Background hashing: pause flags and progress per volume (F06).
    pub hashing: HashingState,
    /// Backup checks, comparisons and copies (F13–F15).
    pub backup: crate::backup_state::BackupState,
    /// Thumbnails of similar images (F11-AC3).
    pub thumbnails: crate::similar_views::Thumbnails,
    /// Connected volumes whose identity the user must choose (F02-AC3).
    pub identities: crate::identity::Identities,
    /// Last health reading of each physical disk (F17).
    pub health: crate::health_state::HealthState,
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

/// The server of a UNC path as written (`\\ZimaOS\Foto` → `ZimaOS`).
fn unc_server(unc: &str) -> Option<String> {
    let rest = unc.replace('/', "\\");
    let server = rest.strip_prefix(r"\\")?.split('\\').next()?;
    (!server.is_empty() && server != "?").then(|| server.to_owned())
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
    pub fn shortcut_error(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.shortcut_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub fn notices(&self) -> std::sync::MutexGuard<'_, Notices> {
        self.notices.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn online(&self) -> std::sync::MutexGuard<'_, HashMap<VolumeId, DetectedVolume>> {
        self.online.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The catalog screen: every volume with its labels, state, totals and current job.
    pub fn volume_views(&self) -> Result<Vec<VolumeView>, ApiError> {
        let (rows, disks) = self.store.read(|c| {
            let mut out = Vec::new();
            for v in store::volumes(c)? {
                let totals = match store::root_entry(c, v.id)? {
                    Some(root) => store::dir_stats(c, root.id)?.unwrap_or_default(),
                    None => prism_core::model::DirStats::default(),
                };
                out.push((v, totals));
            }
            Ok((out, store::volume_disks(c)?))
        })?;
        let jobs = self.engine.jobs();
        let online = self.online();
        Ok(rows
            .into_iter()
            .map(|(v, totals)| {
                let detected = online.get(&v.id);
                let server = v.info.unc_path.as_deref().and_then(unc_server);
                // The physical disk: as saved, or as read now (before it is saved).
                let disk_key = disks.get(&v.id).cloned().or_else(|| {
                    detected
                        .and_then(|d| d.disks.first())
                        .and_then(|n| self.health.snapshot(*n))
                        .map(|s| s.key)
                });
                let group = if v.info.kind == VolumeKind::Network {
                    server.as_ref().map(|s| format!("net:{}", names::fold(s)))
                } else {
                    disk_key.as_ref().map(|k| format!("disk:{k}"))
                };
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
                    path: v.info.unc_path.clone(),
                    group,
                    disk_model: disk_key
                        .as_deref()
                        .and_then(|k| k.split('|').next())
                        .map(str::to_owned),
                    bus: v.info.bus.map(|b| b.as_str().to_owned()),
                    server_name: server
                        .as_deref()
                        .and_then(|s| self.names.name_of(&names::fold(s))),
                    server,
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
                    health: detected.and_then(|d| self.health_badge(d)),
                    hashes: self.hashing.progress(v.id).map(|h| {
                        let active = |hash: bool| {
                            jobs.iter().any(|j| {
                                j.volume == Some(v.id)
                                    && (j.kind == "hash") == hash
                                    && !j.state.is_finished()
                            })
                        };
                        let running = active(true);
                        let left = h.total.saturating_sub(h.hashed);
                        HashesView {
                            total: h.total,
                            hashed: h.hashed,
                            running,
                            waiting: self
                                .hashing
                                .waiting(v.id, left, running, active(false))
                                .map(|w| w.as_str().to_owned()),
                            skipped: self.hashing.skipped_files(v.id).min(left),
                        }
                    }),
                }
            })
            .collect())
    }

    pub(crate) fn detected(&self, volume: VolumeId) -> Result<DetectedVolume, ApiError> {
        self.online()
            .get(&volume)
            .cloned()
            .ok_or_else(|| ApiError::new("volume_offline", format!("volume {volume}")))
    }

    /// The running or queued jobs of a volume that are not background work (hashes): those
    /// give way to an indexing (`JobEngine::should_yield`) and never make it wait.
    pub fn foreground_jobs(&self, volume: VolumeId) -> Vec<JobInfo> {
        self.engine
            .active_for(volume)
            .into_iter()
            .filter(|j| j.priority != Priority::Background)
            .collect()
    }

    /// Queues an indexing or an update of an online volume; if a job already runs on it,
    /// returns that one.
    pub fn submit_index(&self, volume: VolumeId, kind: IndexKind) -> Result<JobId, ApiError> {
        if let Some(active) = self.foreground_jobs(volume).first() {
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
            disks: kind.disks(&detected),
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

/// Space analysis (F08); catalog reads only, so offline volumes work too (F08-AC4).
impl AppState {
    pub fn space_folder(
        &self,
        volume: VolumeId,
        folder: Option<EntryId>,
    ) -> Result<SpaceFolderView, ApiError> {
        Ok(self.store.read(|c| {
            let folder = space::folder_of_volume(c, volume, folder)?;
            let breadcrumb = space::breadcrumb(c, folder)?
                .into_iter()
                .map(|(id, name)| CrumbView { id: id.0, name })
                .collect();
            let mut items = space::folder_items(c, folder)?;
            let more_items = items.len().saturating_sub(MAX_ITEMS) as u64;
            items.truncate(MAX_ITEMS);
            let treemap = space::treemap(c, folder, TreemapOptions::default())?;
            Ok(SpaceFolderView {
                volume_id: volume.0,
                folder_id: folder.0,
                breadcrumb,
                items: items.into_iter().map(Into::into).collect(),
                more_items,
                treemap: treemap.into(),
            })
        })?)
    }

    pub fn largest_files(
        &self,
        volume: VolumeId,
        folder: Option<EntryId>,
        limit: usize,
    ) -> Result<Vec<LargeFileView>, ApiError> {
        Ok(self.store.read(|c| {
            let folder = space::folder_of_volume(c, volume, folder)?;
            Ok(space::largest_files(c, folder, limit.min(1000))?
                .into_iter()
                .map(Into::into)
                .collect())
        })?)
    }

    /// The most recent records of the actions log, newest first (at most 1000).
    pub fn recent_actions(&self, limit: usize) -> Result<Vec<crate::dto::ActionView>, ApiError> {
        let limit = limit.min(1000);
        Ok(self
            .store
            .read(|c| store::actions_log(c, limit))?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn type_stats(&self, volume: Option<VolumeId>) -> Result<TypeStatsView, ApiError> {
        let scope = volume.map_or(StatsScope::Catalog, StatsScope::Volume);
        Ok(self.store.read(|c| space::type_stats(c, scope, 25))?.into())
    }

    /// Path on disk of a catalog entry of a connected volume (ADR-0003: the interface passes
    /// ids, the backend resolves paths).
    pub fn entry_path(&self, volume: VolumeId, entry: EntryId) -> Result<PathBuf, ApiError> {
        let detected = self.detected(volume)?;
        let root = access_path(&detected)
            .ok_or_else(|| ApiError::new("volume_offline", format!("volume {volume}")))?;
        let rel = self.store.read(|c| {
            let e = store::entry(c, entry)?
                .filter(|e| e.volume == volume)
                .ok_or_else(|| StoreError::NotFound(format!("entry {entry} of {volume}")))?;
            store::relative_path(c, e.id)
        })?;
        let mut path = root;
        for part in rel.split('\\').filter(|p| !p.is_empty()) {
            path.push(names::decode(part));
        }
        Ok(path)
    }
}

/// Guided cleanup (F09) and safe deletion (ADR-0012).
impl AppState {
    /// What can be cleaned on the connected, indexed volumes.
    pub fn cleanup_scan(&self) -> Result<CleanupView, ApiError> {
        let running = prism_win::process::running_process_names();
        let known = self.store.read(store::volumes)?;
        let online: Vec<(VolumeId, DetectedVolume)> =
            self.online().iter().map(|(k, v)| (*k, v.clone())).collect();
        let mut view = CleanupView::default();
        for (id, detected) in online {
            let indexed = known
                .iter()
                .any(|v| v.id == id && auto_update(v.status) && v.last_indexed_at.is_some());
            let Some(root) = access_path(&detected).filter(|_| indexed) else {
                continue;
            };
            let rules = cleanup_views::rules_for(&root);
            let permanent = detected.info.kind != VolumeKind::Fixed;
            let groups = self.store.read(|c| {
                let mut out = vec![
                    cleanup_views::group(
                        Category::Temp,
                        id.0,
                        cleanup::temp_files(c, id, &rules)?,
                        None,
                        None,
                        permanent,
                    ),
                    cleanup_views::group(
                        Category::EmptyFolders,
                        id.0,
                        cleanup::empty_folders(c, id, &rules)?,
                        None,
                        None,
                        permanent,
                    ),
                    cleanup_views::group(
                        Category::LargeOld,
                        id.0,
                        cleanup::large_old_files(c, id, &rules)?,
                        None,
                        None,
                        permanent,
                    ),
                ];
                for b in &rules.browsers {
                    let blocked = running.contains(&b.process).then(|| b.browser.clone());
                    out.push(cleanup_views::group(
                        Category::BrowserCache,
                        id.0,
                        cleanup::browser_caches(c, id, b)?,
                        Some(b.browser.clone()),
                        blocked,
                        permanent,
                    ));
                }
                Ok(out)
            })?;
            view.groups.extend(groups.into_iter().flatten());
            if !permanent
                && let Some((items, bytes)) = prism_win::trash::recycle_bin_info(&root)
                && items > 0
            {
                view.recycle_bins.push(RecycleBinView {
                    volume_id: id.0,
                    items,
                    bytes,
                });
            }
        }
        Ok(view)
    }

    /// Deletes catalog entries of a connected volume (ADR-0012): to the Recycle Bin on local
    /// fixed volumes; elsewhere only permanently and only with `permanent` confirmed by the
    /// user (F16-AC2).
    pub fn delete_entries(
        &self,
        volume: VolumeId,
        entries: &[EntryId],
        empty_folders: bool,
        permanent: bool,
    ) -> Result<CleanupResultView, ApiError> {
        let folders = if empty_folders {
            FolderCheck::NoFiles
        } else {
            FolderCheck::Exists
        };
        let report = self.delete_on(volume, entries, folders, permanent, false)?;
        Ok((&report).into())
    }

    /// How the files of a connected volume are disposed of (F16-AC2).
    fn disposal(&self, volume: VolumeId, permanent: bool) -> Result<Disposal, ApiError> {
        let detected = self.detected(volume)?;
        if detected.info.kind == VolumeKind::Fixed {
            Ok(Disposal::RecycleBin)
        } else if permanent {
            Ok(Disposal::Permanent)
        } else {
            Err(ApiError::new("needs_permanent", format!("volume {volume}")))
        }
    }

    fn delete_on(
        &self,
        volume: VolumeId,
        entries: &[EntryId],
        folders: FolderCheck,
        permanent: bool,
        verify_hash: bool,
    ) -> Result<fileops::DeleteReport, ApiError> {
        let disposal = self.disposal(volume, permanent)?;
        let detected = self.detected(volume)?;
        let root = access_path(&detected)
            .ok_or_else(|| ApiError::new("volume_offline", format!("volume {volume}")))?;
        let report = fileops::delete_entries(
            &self.store,
            &DeleteRequest {
                volume,
                root: &root,
                entries,
                disposal,
                folders,
                verify_hash,
            },
            &ShellRemover,
            self.deletion_guard()?.as_ref(),
        )
        .map_err(|e| ApiError::new("delete_failed", e))?;
        tracing::info!(
            volume = volume.0,
            done = report.count("done"),
            freed = report.freed(),
            "deletion finished"
        );
        Ok(report)
    }

    /// The duplicate groups (F10) with the copies the rules propose.
    pub fn duplicates(
        &self,
        filter: &DupFilterInput,
        rules: &[RuleInput],
    ) -> Result<DuplicatesView, ApiError> {
        let online: HashMap<VolumeId, Option<String>> = self
            .online()
            .iter()
            .map(|(id, d)| {
                (
                    *id,
                    access_path(d).map(|p| p.to_string_lossy().into_owned()),
                )
            })
            .collect();
        let online_ids = online.keys().copied().collect();
        let (result, volumes) = self.store.read(|c| {
            let result = duplicates::find(
                c,
                &online_ids,
                &filter.to_filter(),
                duplicates_views::MAX_GROUPS,
            )?;
            Ok((result, store::volumes(c)?))
        })?;
        let volumes = volumes
            .into_iter()
            .map(|v| {
                let mount = online.get(&v.id).cloned().flatten();
                (v.id, (v, mount))
            })
            .collect();
        let pending = self.hashing.pending_files(filter.volume_id.map(VolumeId));
        Ok(duplicates_views::view(
            &result,
            &duplicates_views::rules(rules),
            &volumes,
            pending,
        ))
    }

    /// Deletes copies of duplicates, on several disks (F16): refused if a content would lose
    /// its last copy, and, unless confirmed, if only copies on disconnected disks would stay
    /// (F16-AC4); each file is hashed again just before and skipped if it changed (F16-AC3).
    pub fn delete_duplicates(
        &self,
        entries: &[EntryId],
        allow_offline_only: bool,
        permanent: bool,
    ) -> Result<CleanupResultView, ApiError> {
        let online: std::collections::HashSet<VolumeId> = self.online().keys().copied().collect();
        let (check, records) = self.store.read(|c| {
            let check = duplicates::check_last_copies(c, entries, &online, allow_offline_only)?;
            let mut records = Vec::with_capacity(entries.len());
            for &e in entries {
                records.push((e, store::entry(c, e)?));
            }
            Ok((check, records))
        })?;
        if let Err(e) = check {
            let (code, entry) = match e {
                LastCopyError::LastCopy(e) => ("last_copy", e),
                LastCopyError::OnlyOffline(e) => ("only_offline_copies", e),
                LastCopyError::NoHash(e) => ("not_hashed", e),
            };
            return Err(ApiError::new(code, entry.0));
        }
        self.check_same_files(entries)?;
        let mut by_volume: std::collections::BTreeMap<VolumeId, Vec<EntryId>> =
            std::collections::BTreeMap::new();
        for (id, record) in records {
            let record = record.ok_or_else(|| ApiError::new("not_found", id.0))?;
            by_volume.entry(record.volume).or_default().push(id);
        }
        // Everything is checked before the first file goes: disconnected disks, permanent
        // deletion where there is no Recycle Bin.
        for &volume in by_volume.keys() {
            self.disposal(volume, permanent)?;
        }
        let mut total = CleanupResultView::default();
        for (volume, ids) in by_volume {
            let report = self.delete_on(volume, &ids, FolderCheck::Exists, permanent, true)?;
            total.add(&(&report).into());
            crate::hash_jobs::refresh(self, volume);
        }
        Ok(total)
    }

    /// Refuses to delete a copy that is, on the disk, the same file as a copy that stays: a hard
    /// link, or one file a NAS shows under two shares (REVIEW D62). The catalog sees two copies;
    /// deleting "one" would delete the only one. Checked on the connected copies, before
    /// anything is touched.
    fn check_same_files(&self, entries: &[EntryId]) -> Result<(), ApiError> {
        use prism_win::files::{identity, same_file};
        let kept = self.store.read(|c| duplicates::kept_copies(c, entries))?;
        // A copy of the map: `entry_path` takes the lock itself.
        let online = self.online().clone();
        let server = |v: VolumeId| {
            online
                .get(&v)
                .filter(|d| d.info.kind == VolumeKind::Network)
                .and_then(|d| d.info.unc_path.as_deref())
                .and_then(prism_core::volumes::unc_parts)
                .map(|(s, _)| s)
        };
        for (doomed, copies) in kept {
            let Some(volume) = self
                .store
                .read(move |c| store::entry(c, doomed))?
                .map(|r| r.volume)
            else {
                continue;
            };
            if !online.contains_key(&volume) {
                continue;
            }
            let Some(id) = self
                .entry_path(volume, doomed)
                .ok()
                .and_then(|p| identity(&p))
            else {
                continue;
            };
            for (copy, copy_volume) in copies {
                if !online.contains_key(&copy_volume) {
                    continue;
                }
                let Ok(path) = self.entry_path(copy_volume, copy) else {
                    continue;
                };
                let same_server = server(volume).is_some() && server(volume) == server(copy_volume);
                if identity(&path).is_some_and(|other| same_file(&id, &other, same_server)) {
                    tracing::warn!(entry = doomed.0, same_as = %path.display(), "one file under two paths: not deleted");
                    return Err(ApiError::new("same_file", path.display()));
                }
            }
        }
        Ok(())
    }

    /// The guard of every destructive operation of the app (CLAUDE.md, TESTING S2): development
    /// builds, which are driven with catalogs of real disks, may act only inside the PRISMTEST
    /// test volume (found by its label, never by letter), the temporary folder and the
    /// development data; release builds have no guard.
    pub fn deletion_guard(&self) -> Result<Option<SandboxGuard>, ApiError> {
        if !self.dev {
            return Ok(None);
        }
        let test_volume = self
            .online()
            .values()
            .find(|d| d.info.os_label.as_deref() == Some(TEST_VOLUME_LABEL))
            .and_then(access_path);
        let dev_data = std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("PrismDev"));
        let roots = debug_guard_roots(test_volume, std::env::temp_dir(), dev_data);
        SandboxGuard::new(roots)
            .map(Some)
            .map_err(|e| ApiError::new("delete_failed", e))
    }
}

/// Label of the virtual test disk (docs/TESTING.md §4).
pub const TEST_VOLUME_LABEL: &str = "PRISMTEST";

/// The folders a development build may change: those that exist among the test volume, the
/// temporary folder and the development data.
pub fn debug_guard_roots(
    test_volume: Option<PathBuf>,
    temp: PathBuf,
    dev_data: Option<PathBuf>,
) -> Vec<PathBuf> {
    [test_volume, Some(temp), dev_data]
        .into_iter()
        .flatten()
        .filter(|p| p.is_dir())
        .collect()
}

/// Search (F07): parse, run on the in-memory indexes, read the details of the best hits.
impl AppState {
    pub fn search(&self, text: &str, utc_offset_min: i32) -> Result<SearchResultView, ApiError> {
        let started = std::time::Instant::now();
        let clock = Clock {
            now_ms: prism_core::model::unix_millis_now(),
            utc_offset_min,
        };
        let query = match query::parse(text, clock) {
            Ok(q) if q.is_empty() => return Ok(SearchResultView::default()),
            Ok(q) => q,
            Err(e) => {
                return Ok(SearchResultView {
                    error: Some(QueryErrorView::from_query(&e, text)),
                    ..SearchResultView::default()
                });
            }
        };
        let volumes = self.store.read(store::volumes)?;
        let online: std::collections::HashSet<VolumeId> = self.online().keys().copied().collect();
        let volumes: HashMap<VolumeId, (Volume, bool)> = volumes
            .into_iter()
            .filter(|v| v.status != VolumeStatus::Ignored)
            .map(|v| (v.id, (v.clone(), online.contains(&v.id))))
            .collect();
        let metas: Vec<VolumeMeta> = volumes
            .values()
            .map(|(v, on)| VolumeMeta {
                id: v.id,
                names: [
                    v.labels.name.clone(),
                    v.info.os_label.clone(),
                    v.info.last_mount.clone(),
                    v.info.unc_path.clone(),
                ]
                .into_iter()
                .flatten()
                .filter(|n| !n.trim().is_empty())
                .collect(),
                online: *on,
            })
            .collect();
        let indexes = self.search.snapshot();
        let outcome = match engine::search(&indexes, &metas, &query, SearchOptions::default()) {
            Ok(o) => o,
            Err(SearchError::UnknownDisk(name)) => {
                let start = text.find(&name).unwrap_or(0);
                return Ok(SearchResultView {
                    error: Some(QueryErrorView::new(
                        "unknown_disk",
                        &name,
                        text,
                        start,
                        start + name.len(),
                    )),
                    ..SearchResultView::default()
                });
            }
        };
        let hits = self
            .store
            .read(|c| search_views::hit_views(c, &outcome.hits, &volumes))?;
        Ok(SearchResultView {
            hits,
            total: outcome.total as u64,
            capped: outcome.capped,
            error: None,
            ms: started.elapsed().as_secs_f64() * 1000.0,
        })
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

    /// Saves the settings as they are (validation is the caller's).
    pub fn save_settings(&self, settings: &Settings) -> Result<(), ApiError> {
        let json =
            serde_json::to_string(settings).map_err(|e| ApiError::new("invalid_settings", e))?;
        self.store
            .write(move |w| store::set_setting(w, SETTINGS_KEY, &json))?;
        Ok(())
    }

    /// Empties the catalog (F21-AC4): every job stops, every volume and its data go; settings and
    /// the actions log stay, files on the disks are never touched. The connected disks come
    /// back as new ones at the next detection.
    pub fn reset_catalog(&self) -> Result<usize, ApiError> {
        for job in self.engine.jobs().iter().filter(|j| !j.state.is_finished()) {
            self.engine.cancel(job.id);
        }
        for job in self.engine.jobs().iter().filter(|j| !j.state.is_finished()) {
            let _ = self.engine.wait(job.id);
        }
        let removed = self.store.write(store::reset_catalog)?;
        self.online().clear();
        self.hashing.clear();
        self.backup.clear();
        self.thumbnails.clear();
        // Choices made per volume refer to ids that no longer exist.
        let mut settings = self.settings();
        if !settings.hashing_excluded.is_empty() {
            settings.hashing_excluded.clear();
            self.save_settings(&settings)?;
            self.apply_hashing(&settings);
        }
        tracing::info!(removed, "catalog reset");
        Ok(removed)
    }

    /// Applies the hashing settings to the running jobs (F06-AC5, F21-AC3).
    pub fn apply_hashing(&self, settings: &Settings) {
        self.hashing.battery_pause.store(
            settings.pause_hashing_on_battery,
            std::sync::atomic::Ordering::Relaxed,
        );
        crate::hash_jobs::configure(self, settings.hashing_paused, &settings.hashing_excluded);
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

    /// The disks the job keeps busy. A journal update reads a few records every 15 s: it does
    /// not reserve the disk, otherwise the content analysis would give way to it every time and
    /// never progress (REVIEW D31, seen on the real app).
    pub fn disks(self, detected: &DetectedVolume) -> Vec<String> {
        match self {
            Self::Journal => Vec::new(),
            Self::Index | Self::Update => job_disks(detected),
        }
    }
}

/// Whether a job of this kind is shown on the volume card.
pub fn shown_on_card(kind: &str) -> bool {
    matches!(kind, "index" | "update")
}

/// The physical resources a job on this volume keeps busy (one heavy job per disk, ADR-0009):
/// for a network share its server, so the shares of one NAS are read one at a time.
pub(crate) fn job_disks(d: &DetectedVolume) -> Vec<String> {
    if d.info.kind == VolumeKind::Network {
        let unc = d.info.unc_path.as_deref().unwrap_or_default();
        let server = prism_core::volumes::unc_parts(unc).map_or_else(|| unc.to_owned(), |(s, _)| s);
        vec![format!("net:{server}")]
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
