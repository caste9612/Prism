//! Background threads of the application:
//!
//! - the **volume monitor** detects connected volumes every 2 seconds (F01-AC2), records them
//!   in the catalog (new volumes with status `new`, waiting for the user, F02-AC1), updates an
//!   indexed volume when it comes back (F05-AC2) and, while Prism runs, applies the NTFS change
//!   journal every 15 seconds (F05-AC1);
//! - the **job relay** forwards job changes to the interface as `jobs-changed` events, at most
//!   about 4 per second (ADR-0009), and refreshes the volume list when a job ends;
//! - the **network probe** checks that mapped shares answer and reads their free space
//!   (`netprobe`, REVIEW L5): a share counts as connected only once its server has answered;
//! - **folder watchers** keep the volumes without a usable journal up to date (`watchers`,
//!   REVIEW L6), started and stopped by the volume monitor.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use prism_core::model::{VolumeId, VolumeKind, VolumeStatus};
use prism_core::store;
use prism_core::volumes::{self, DetectedVolume, VolumeMatch, VolumeSource, match_volumes};
use prism_win::volumes::WinVolumeSource;
use tauri::{AppHandle, Emitter};

use crate::netprobe::{NetProbe, NetStatus};
use crate::state::{AppState, IndexKind, auto_update, job_view};
use crate::watchers::Watchers;

const DETECT_EVERY: Duration = Duration::from_secs(2);
const JOURNAL_EVERY: Duration = Duration::from_secs(15);

pub fn spawn(app: &AppHandle, state: &Arc<AppState>) {
    let (a, s) = (app.clone(), Arc::clone(state));
    let _ = std::thread::Builder::new()
        .name("prism-volume-monitor".into())
        .spawn(move || volume_monitor(&a, &s));
    let (a, s) = (app.clone(), Arc::clone(state));
    let _ = std::thread::Builder::new()
        .name("prism-job-relay".into())
        .spawn(move || job_relay(&a, &s));
    let net = Arc::clone(&state.net);
    let _ = std::thread::Builder::new()
        .name("prism-net-probes".into())
        .spawn(move || net.run());
}

fn volume_monitor(app: &AppHandle, state: &Arc<AppState>) {
    let mut last_journal = Instant::now();
    let mut watchers = Watchers::default();
    loop {
        if let Err(e) = detect_once(app, state) {
            tracing::warn!(error = %e, "volume detection failed");
        }
        watchers.sync(state);
        if last_journal.elapsed() >= JOURNAL_EVERY {
            last_journal = Instant::now();
            journal_tick(state);
        }
        std::thread::sleep(DETECT_EVERY);
    }
}

/// Network shares are connected only when their server answers; their capacity and free space
/// come from the probe. Local volumes pass unchanged.
fn apply_probe(detected: Vec<DetectedVolume>, net: &NetProbe) -> Vec<DetectedVolume> {
    net.want(
        detected
            .iter()
            .filter(|d| d.info.kind == VolumeKind::Network)
            .filter_map(|d| d.info.unc_path.as_deref()),
    );
    detected
        .into_iter()
        .filter_map(|mut d| {
            if d.info.kind != VolumeKind::Network {
                return Some(d);
            }
            match net.status(d.info.unc_path.as_deref()?) {
                NetStatus::Reachable { capacity, free } => {
                    d.info.capacity = Some(capacity);
                    d.info.free_space = Some(free);
                    Some(d)
                }
                NetStatus::Unknown | NetStatus::Unreachable => None,
            }
        })
        .collect()
}

fn detect_once(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let detected = WinVolumeSource.detect().map_err(|e| e.to_string())?;
    let detected = apply_probe(detected, &state.net);
    let known = state
        .store
        .read(store::volumes)
        .map_err(|e| e.to_string())?;
    let matches = match_volumes(&detected, &known);
    let previous: HashMap<VolumeId, Option<String>> = state
        .online()
        .iter()
        .map(|(id, d)| (*id, d.info.last_mount.clone()))
        .collect();
    let mut now = HashMap::new();
    let mut changed = false;
    let mut came_back = Vec::new();
    for (d, m) in detected.into_iter().zip(matches) {
        match &m {
            VolumeMatch::Known(id) => {
                let id = *id;
                let reconnected = !previous.contains_key(&id);
                let moved = previous
                    .get(&id)
                    .is_some_and(|mount| *mount != d.info.last_mount);
                if reconnected || moved {
                    volumes::record_detected(&state.store, &d, &m).map_err(|e| e.to_string())?;
                    changed = true;
                    if reconnected && known.iter().any(|v| v.id == id && auto_update(v.status)) {
                        came_back.push(id);
                    }
                }
                now.insert(id, d);
            }
            VolumeMatch::New => {
                if let Some(id) =
                    volumes::record_detected(&state.store, &d, &m).map_err(|e| e.to_string())?
                {
                    tracing::info!(volume = id.0, mount = ?d.info.last_mount, "new volume");
                    now.insert(id, d);
                    changed = true;
                }
            }
            VolumeMatch::Ambiguous(ids) => {
                // The user must choose (F02-AC3); not recorded until then (REVIEW D13).
                tracing::warn!(?ids, mount = ?d.info.last_mount, "ambiguous volume identity");
            }
        }
    }
    if now.len() != previous.len() {
        changed = true;
    }
    *state.online() = now;
    // F05-AC2: an indexed volume that comes back is brought up to date.
    for id in came_back {
        if let Err(e) = state.submit_index(id, IndexKind::Update) {
            tracing::warn!(volume = id.0, code = %e.code, "cannot update the reconnected volume");
        }
    }
    if changed {
        let _ = app.emit("volumes-changed", ());
    }
    Ok(())
}

/// F05-AC1: while Prism runs, indexed NTFS volumes are updated from their change journal.
fn journal_tick(state: &AppState) {
    if !state.elevated {
        return; // the journal needs administrator rights (development builds)
    }
    let Ok(known) = state.store.read(store::volumes) else {
        return;
    };
    let candidates: Vec<VolumeId> = {
        let online = state.online();
        known
            .iter()
            .filter(|v| v.status == VolumeStatus::Indexed && v.usn.is_some())
            .filter(|v| {
                online.get(&v.id).is_some_and(|d| {
                    d.info.kind != VolumeKind::Network
                        && d.info
                            .fs_type
                            .as_deref()
                            .is_some_and(|f| f.eq_ignore_ascii_case("NTFS"))
                })
            })
            .map(|v| v.id)
            .collect()
    };
    for id in candidates {
        if state.engine.active_for(id).is_empty() {
            let _ = state.submit_index(id, IndexKind::Journal);
        }
    }
}

fn job_relay(app: &AppHandle, state: &AppState) {
    let events = state.engine.subscribe();
    loop {
        // Wait for a change, or poll progress while something runs (~4 per second).
        let event = events.recv_timeout(Duration::from_millis(250));
        let jobs = state.engine.jobs();
        let running = jobs.iter().any(|j| !j.state.is_finished());
        if event.is_ok() || running {
            let views: Vec<_> = jobs.iter().map(job_view).collect();
            let _ = app.emit("jobs-changed", views);
        }
        if let Ok(job) = event
            && job.state.is_finished()
        {
            let _ = app.emit("volumes-changed", ());
        }
    }
}
