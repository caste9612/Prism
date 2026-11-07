//! Content hashes in the background (F06): a `hash` job per connected, indexed local volume,
//! run in Windows background mode (F06-AC2) with the lowest job priority, paused by the user
//! (tray, F06-AC5) or on battery if the setting asks (F21-AC3). The progress of each volume is
//! kept here, because counting it on a large catalog takes a few hundred milliseconds.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use prism_core::hashing;
use prism_core::jobs::{JobSpec, Priority};
use prism_core::model::{VolumeId, VolumeKind, VolumeStatus};
use prism_core::store;

use crate::state::{AppState, access_path, job_disks};

/// Files and hashed files of a volume (F06-AC5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HashStatus {
    pub total: u64,
    pub hashed: u64,
}

/// The progress of a volume is counted again at most this often, or this often while a hash job
/// runs on it.
const RECHECK: Duration = Duration::from_secs(300);
const RECHECK_RUNNING: Duration = Duration::from_secs(60);
/// And hashed again at most this often (a journal update every 15 s must not restart it).
const RERUN: Duration = Duration::from_secs(120);
/// How often a running hash job asks whether an indexing waits for its disk.
const YIELD_CHECK: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct HashingState {
    /// Paused by the user (persisted in the settings).
    pub paused: AtomicBool,
    /// Pause while on battery (setting, off by default).
    pub battery_pause: AtomicBool,
    /// Volumes the user excluded from the analysis (F21-AC3).
    excluded: Mutex<HashSet<VolumeId>>,
    status: Mutex<HashMap<VolumeId, (HashStatus, Instant)>>,
    last_run: Mutex<HashMap<VolumeId, Instant>>,
}

impl HashingState {
    fn status(&self) -> MutexGuard<'_, HashMap<VolumeId, (HashStatus, Instant)>> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn last_run(&self) -> MutexGuard<'_, HashMap<VolumeId, Instant>> {
        self.last_run.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn is_excluded(&self, volume: VolumeId) -> bool {
        self.excluded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&volume)
    }

    /// The known progress of a volume.
    pub fn progress(&self, volume: VolumeId) -> Option<HashStatus> {
        self.status().get(&volume).map(|(s, _)| *s)
    }

    /// Files still without a hash on the volumes counted so far.
    pub fn pending_files(&self) -> u64 {
        self.status()
            .values()
            .map(|(s, _)| s.total.saturating_sub(s.hashed))
            .sum()
    }

    /// Whether hashing must stop now (between two files).
    pub fn should_pause(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
            || (self.battery_pause.load(Ordering::Relaxed) && prism_win::process::on_battery())
    }
}

/// Counts the progress of a volume again (after hashing or deleting).
pub fn refresh(state: &AppState, volume: VolumeId) {
    if let Ok((total, hashed)) = state.store.read(|c| store::hash_progress(c, volume)) {
        state
            .hashing
            .status()
            .insert(volume, (HashStatus { total, hashed }, Instant::now()));
    }
}

/// Called every 30 s: counts the progress of the volumes that need it and queues a hash job
/// where there is work and nothing else runs on the volume. Returns whether some progress
/// changed (the cards are refreshed).
pub fn tick(state: &Arc<AppState>) -> bool {
    let Ok(known) = state.store.read(store::volumes) else {
        return false;
    };
    let hashing: Vec<VolumeId> = state
        .engine
        .jobs()
        .iter()
        .filter(|j| j.kind == "hash" && !j.state.is_finished())
        .filter_map(|j| j.volume)
        .collect();
    // Progress of every indexed volume, online or not, counted now and then.
    let mut changed = false;
    for v in known.iter().filter(|v| v.last_indexed_at.is_some()) {
        let every = if hashing.contains(&v.id) {
            RECHECK_RUNNING
        } else {
            RECHECK
        };
        let before = state.hashing.status().get(&v.id).copied();
        if before.is_none_or(|(_, at)| at.elapsed() >= every) {
            refresh(state, v.id);
            changed |= state.hashing.progress(v.id) != before.map(|(s, _)| s);
        }
    }
    if state.hashing.should_pause() {
        return changed;
    }
    let online: Vec<_> = state
        .online()
        .iter()
        .map(|(id, d)| (*id, d.clone()))
        .collect();
    for (id, detected) in online {
        // Network shares are not read in full in the background: every byte would cross the
        // network (REVIEW D30). Nor the volumes the user excluded (F21-AC3).
        if detected.info.kind == VolumeKind::Network || state.hashing.is_excluded(id) {
            continue;
        }
        let indexed = known.iter().any(|v| {
            v.id == id && matches!(v.status, VolumeStatus::Indexed) && v.last_indexed_at.is_some()
        });
        let pending = state
            .hashing
            .progress(id)
            .is_some_and(|s| s.hashed < s.total);
        let recent = state
            .hashing
            .last_run()
            .get(&id)
            .is_some_and(|t| t.elapsed() < RERUN);
        if !indexed || !pending || recent || !state.engine.active_for(id).is_empty() {
            // Another job on the volume (an indexing, or hashes already running).
            continue;
        }
        let Some(root) = access_path(&detected) else {
            continue;
        };
        state.hashing.last_run().insert(id, Instant::now());
        let s = Arc::clone(state);
        state.engine.submit(JobSpec {
            kind: "hash".to_owned(),
            volume: Some(id),
            priority: Priority::Background,
            disks: job_disks(&detected),
            run: Box::new(move |ctx| {
                let _background = prism_win::process::BackgroundMode::enter();
                // Asking the engine takes a lock: at most a few times a second.
                let checked = Cell::new(Instant::now());
                let yielding = Cell::new(false);
                let pause = || {
                    if !yielding.get() && checked.get().elapsed() >= YIELD_CHECK {
                        checked.set(Instant::now());
                        yielding.set(s.engine.should_yield(ctx.id));
                    }
                    yielding.get() || s.hashing.should_pause()
                };
                let result =
                    hashing::hash_volume(&s.store, id, &root, &ctx.cancel, &pause, &ctx.progress);
                refresh(&s, id);
                let report = result.map_err(|e| e.to_string())?;
                tracing::info!(
                    volume = id.0,
                    quick = report.quick,
                    full = report.full,
                    changed = report.changed,
                    unreadable = report.unreadable,
                    interrupted = report.interrupted,
                    "hashing finished"
                );
                Ok(())
            }),
        });
    }
    changed
}

/// Applies the hashing settings: pause (tray, F06-AC5) and excluded volumes (F21-AC3). The
/// running jobs that must stop are cancelled; they stop between two files.
pub fn configure(state: &AppState, paused: bool, excluded: &[i64]) {
    state.hashing.paused.store(paused, Ordering::Relaxed);
    let excluded: HashSet<VolumeId> = excluded.iter().copied().map(VolumeId).collect();
    for job in state.engine.jobs() {
        let stop = paused || job.volume.is_some_and(|v| excluded.contains(&v));
        if job.kind == "hash" && !job.state.is_finished() && stop {
            state.engine.cancel(job.id);
        }
    }
    *state
        .hashing
        .excluded
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = excluded;
}
