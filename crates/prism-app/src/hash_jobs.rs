//! Content hashes in the background (F06): a `hash` job per connected, indexed volume — network
//! shares included, one per server at a time (REVIEW D58) — run in Windows background mode
//! (F06-AC2) with the lowest job priority, paused by the user
//! (tray, F06-AC5) or on battery if the setting asks (F21-AC3). The progress of each volume is
//! kept here, because counting it on a large catalog takes a few hundred milliseconds.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use prism_core::hashing;
use prism_core::jobs::{JobSpec, Priority};
use prism_core::media;
use prism_core::model::{FileTime, VolumeId, VolumeStatus};
use prism_core::store;

use crate::state::{AppState, access_path, job_disks};

/// Files and hashed files of a volume (F06-AC5); images and printed images (F11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HashStatus {
    pub total: u64,
    pub hashed: u64,
    pub images: u64,
    pub printed: u64,
    pub videos: u64,
    pub videos_printed: u64,
}

impl HashStatus {
    fn pending(&self) -> bool {
        self.hashed < self.total || self.printed < self.images || self.videos_printed < self.videos
    }
}

/// The progress of a volume is counted again at most this often, or this often while a hash job
/// runs on it.
const RECHECK: Duration = Duration::from_secs(300);
const RECHECK_RUNNING: Duration = Duration::from_secs(60);
/// And hashed again at most this often (a journal update every 15 s must not restart it).
const RERUN: Duration = Duration::from_secs(120);
/// When the only files left are those the last round could not read (in use, changing,
/// unreadable): tried again this often. Seen in a diagnostic package: three files always open
/// made a round every 2 minutes all day, each doing nothing.
const RERUN_SKIPPED: Duration = Duration::from_secs(30 * 60);
/// How often a running hash job asks whether an indexing waits for its disk.
const YIELD_CHECK: Duration = Duration::from_millis(500);
/// Files modified less than this long ago (or this far in the future, a server clock ahead of
/// the PC) are probably still being written: left for a later round.
const SETTLING: Duration = Duration::from_secs(10 * 60);

/// The modification times of files still being written, around now.
fn settling_window() -> std::ops::Range<FileTime> {
    let now = unix_millis_now();
    let span = i64::try_from(SETTLING.as_millis()).unwrap_or(i64::MAX);
    FileTime::from_unix_millis(now.saturating_sub(span))
        ..FileTime::from_unix_millis(now.saturating_add(span))
}

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
    /// What the last complete round could not do.
    skipped: Mutex<HashMap<VolumeId, Skipped>>,
}

/// What the last complete round of a volume left: files it could not read (changed while read,
/// changed since the catalog was updated, unreadable, or still being written) and images and
/// videos it could not
/// print because they changed since the catalog was updated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skipped {
    pub files: u64,
    pub prints: u64,
}

/// Why the analysis of a volume with files left is not running now (the activity panel says
/// it instead of a generic wait).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waiting {
    /// The user left the volume out of the analysis.
    Excluded,
    /// Paused by the user, or on battery.
    Paused,
    /// Another job (an indexing, an update, a copy) uses the volume.
    Busy,
    /// The files left are those the last round could not read: tried again later.
    Skipped,
    /// The next round starts soon.
    Next,
}

impl Waiting {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Excluded => "excluded",
            Self::Paused => "paused",
            Self::Busy => "busy",
            Self::Skipped => "skipped",
            Self::Next => "next",
        }
    }
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

    /// What is still to do on the volumes counted so far, or on `scope` only (the screen of a
    /// disk shows what is left on that disk).
    fn pending(&self, scope: Option<VolumeId>, left: impl Fn(&HashStatus) -> u64) -> u64 {
        self.status()
            .iter()
            .filter(|(v, _)| scope.is_none_or(|s| **v == s))
            .map(|(_, (s, _))| left(s))
            .sum()
    }

    /// Files still without a hash.
    pub fn pending_files(&self, scope: Option<VolumeId>) -> u64 {
        self.pending(scope, |s| s.total.saturating_sub(s.hashed))
    }

    /// Images still without a print.
    pub fn pending_images(&self, scope: Option<VolumeId>) -> u64 {
        self.pending(scope, |s| s.images.saturating_sub(s.printed))
    }

    /// Videos still without a print.
    pub fn pending_videos(&self, scope: Option<VolumeId>) -> u64 {
        self.pending(scope, |s| s.videos.saturating_sub(s.videos_printed))
    }

    /// Forgets the counts and the runs (after a reset of the catalog).
    pub fn clear(&self) {
        self.status().clear();
        self.last_run().clear();
        self.skipped().clear();
    }

    fn skipped(&self) -> MutexGuard<'_, HashMap<VolumeId, Skipped>> {
        self.skipped.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Files of the volume the last complete round could not read.
    pub fn skipped_files(&self, volume: VolumeId) -> u64 {
        self.skipped().get(&volume).map_or(0, |s| s.files)
    }

    fn skipped_of(&self, volume: VolumeId) -> Skipped {
        self.skipped().get(&volume).copied().unwrap_or_default()
    }

    /// Why the analysis of a volume with `left` files without a hash is not running, if it is
    /// not; `busy` when another job uses the volume.
    pub fn waiting(
        &self,
        volume: VolumeId,
        left: u64,
        running: bool,
        busy: bool,
    ) -> Option<Waiting> {
        if running || left == 0 {
            return None;
        }
        Some(if self.is_excluded(volume) {
            Waiting::Excluded
        } else if self.should_pause() {
            Waiting::Paused
        } else if busy {
            Waiting::Busy
        } else if self.skipped_files(volume) >= left {
            Waiting::Skipped
        } else {
            Waiting::Next
        })
    }

    /// Whether hashing must stop now (between two files).
    pub fn should_pause(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
            || (self.battery_pause.load(Ordering::Relaxed) && prism_win::process::on_battery())
    }
}

/// Counts the progress of a volume again (after hashing or deleting).
pub fn refresh(state: &AppState, volume: VolumeId) {
    let counted = state.store.read(|c| {
        Ok((
            store::hash_progress(c, volume)?,
            media::print_progress(c, volume)?,
            media::video::video_progress(c, volume)?,
        ))
    });
    if let Ok(((total, hashed), (images, printed), (videos, videos_printed))) = counted {
        let status = HashStatus {
            total,
            hashed,
            images,
            printed,
            videos,
            videos_printed,
        };
        state
            .hashing
            .status()
            .insert(volume, (status, Instant::now()));
    }
}

/// Whether the only work left on a volume is what its last complete round could not do: files
/// it could not read and prints of files changed since the catalog was updated. Seen in a
/// diagnostic package: with the catalog of C: behind, a round every 2 minutes found 2,166
/// changed files and 377 changed images, and did nothing else.
fn only_skipped_left(s: &HashStatus, skipped: Skipped) -> bool {
    let files = s.total.saturating_sub(s.hashed);
    let prints = s.images.saturating_sub(s.printed) + s.videos.saturating_sub(s.videos_printed);
    files + prints > 0 && skipped.files >= files && skipped.prints >= prints
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
        // Indexed or updated since it was counted: count now, so the new files are analysed
        // at the next round instead of minutes later.
        let indexed_since = before.is_some_and(|(_, at)| {
            let counted =
                unix_millis_now() - i64::try_from(at.elapsed().as_millis()).unwrap_or(i64::MAX);
            v.last_indexed_at.is_some_and(|t| t > counted)
        });
        if indexed_since || before.is_none_or(|(_, at)| at.elapsed() >= every) {
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
        // Network shares too (REVIEW D58), one per server at a time (`job_disks`); not the
        // volumes the user excluded (F21-AC3).
        if state.hashing.is_excluded(id) {
            continue;
        }
        let indexed = known.iter().any(|v| {
            v.id == id && matches!(v.status, VolumeStatus::Indexed) && v.last_indexed_at.is_some()
        });
        let progress = state.hashing.progress(id);
        let pending = progress.is_some_and(|s| s.pending());
        let rerun = if progress.is_some_and(|s| only_skipped_left(&s, state.hashing.skipped_of(id)))
        {
            RERUN_SKIPPED
        } else {
            RERUN
        };
        let recent = state
            .hashing
            .last_run()
            .get(&id)
            .is_some_and(|t| t.elapsed() < rerun);
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
                let result = hashing::hash_volume(
                    &s.store,
                    id,
                    &root,
                    Some(settling_window()),
                    &ctx.cancel,
                    &pause,
                    &ctx.progress,
                );
                let report = match result {
                    Ok(r) => r,
                    Err(e) => {
                        refresh(&s, id);
                        return Err(e.to_string());
                    }
                };
                tracing::info!(
                    volume = id.0,
                    quick = report.quick,
                    full = report.full,
                    changed = report.changed,
                    unreadable = report.unreadable,
                    recent = report.recent,
                    interrupted = report.interrupted,
                    "hashing finished"
                );
                // Then the prints of the images (F11), in the same background job.
                if !report.interrupted {
                    // Decoding pictures is limited by the processor: a few threads, each in
                    // background mode like this one (REVIEW D60).
                    let background =
                        || std::mem::forget(prism_win::process::BackgroundMode::enter());
                    let workers = media::Workers {
                        count: media::print_workers(),
                        init: &background,
                    };
                    let prints = media::print_volume(
                        &s.store,
                        id,
                        &root,
                        &workers,
                        &ctx.cancel,
                        &pause,
                        &ctx.progress,
                    );
                    // Prints skipped because their file changed; unknown (0) if a pass did not
                    // end, so the next round comes at the usual pace.
                    let mut skipped_prints = Some(0u64);
                    match prints {
                        Ok(p) => {
                            tracing::info!(
                                volume = id.0,
                                printed = p.printed,
                                unreadable = p.unreadable,
                                changed = p.changed,
                                interrupted = p.interrupted,
                                "image prints finished"
                            );
                            skipped_prints = skipped_prints
                                .filter(|_| !p.interrupted)
                                .map(|n| n + p.changed);
                        }
                        Err(e) => {
                            tracing::warn!(volume = id.0, error = %e, "image prints failed");
                            skipped_prints = None;
                        }
                    }
                    // And of the videos, decoded by Windows (F12, ADR-0013).
                    let videos = media::video::print_videos(
                        &s.store,
                        id,
                        &root,
                        &prism_win::video::MfVideoDecoder,
                        &ctx.cancel,
                        &pause,
                        &ctx.progress,
                    );
                    match videos {
                        Ok(p) => {
                            tracing::info!(
                                volume = id.0,
                                printed = p.printed,
                                unreadable = p.unreadable,
                                changed = p.changed,
                                interrupted = p.interrupted,
                                "video prints finished"
                            );
                            skipped_prints = skipped_prints
                                .filter(|_| !p.interrupted)
                                .map(|n| n + p.changed);
                        }
                        Err(e) => {
                            tracing::warn!(volume = id.0, error = %e, "video prints failed");
                            skipped_prints = None;
                        }
                    }
                    s.hashing.skipped().insert(
                        id,
                        Skipped {
                            files: report.changed + report.unreadable + report.recent,
                            prints: skipped_prints.unwrap_or(0),
                        },
                    );
                }
                refresh(&s, id);
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

/// Milliseconds since 1970, as the catalog stores its times.
fn unix_millis_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Files that stay unreadable (open, changing) do not make a round every 2 minutes; new files
    // or images without a print do.
    #[test]
    fn rounds_with_only_unreadable_files_left_wait_longer() {
        let s = HashStatus {
            total: 100,
            hashed: 97,
            images: 10,
            printed: 10,
            videos: 0,
            videos_printed: 0,
        };
        let files = |files| Skipped { files, prints: 0 };
        assert!(only_skipped_left(&s, files(3)));
        assert!(!only_skipped_left(&s, files(2)), "a new file to read");
        assert!(
            !only_skipped_left(&HashStatus { printed: 9, ..s }, files(3)),
            "an image to print"
        );
        assert!(
            !only_skipped_left(&HashStatus { hashed: 100, ..s }, files(3)),
            "nothing left"
        );
    }

    // Seen on C: of a company PC whose catalog was behind: files and images changed since the
    // catalog was updated, nothing else; the round waits until the catalog catches up.
    #[test]
    fn rounds_with_only_changed_prints_left_wait_longer() {
        let s = HashStatus {
            total: 100,
            hashed: 97,
            images: 10,
            printed: 8,
            videos: 4,
            videos_printed: 3,
        };
        let skipped = Skipped {
            files: 3,
            prints: 3,
        };
        assert!(only_skipped_left(&s, skipped));
        assert!(
            !only_skipped_left(
                &s,
                Skipped {
                    prints: 2,
                    ..skipped
                }
            ),
            "a print to make"
        );
        let hashed = HashStatus { hashed: 100, ..s };
        assert!(
            only_skipped_left(
                &hashed,
                Skipped {
                    files: 0,
                    ..skipped
                }
            ),
            "only prints left, all skipped"
        );
    }
}
