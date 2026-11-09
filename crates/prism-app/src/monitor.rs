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
//! - **new-disk notifications** while the window is hidden (`notices`, REVIEW L8);
//! - **folder watchers** keep the volumes without a usable journal up to date (`watchers`,
//!   REVIEW L6), started and stopped by the volume monitor.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use prism_core::model::{Volume, VolumeId, VolumeKind, VolumeStatus};
use prism_core::store;
use prism_core::volumes::{
    self, DetectedVolume, VolumeMatch, VolumeSource, match_volumes, normalize_unc,
};
use prism_win::volumes::WinVolumeSource;
use tauri::{AppHandle, Emitter, Manager};

use crate::identity::{Ambiguous, ambiguity_key};
use crate::netprobe::{NetProbe, NetStatus};
use crate::notices::notice_name;
use crate::state::{AppState, IndexKind, auto_update, job_view};
use crate::watchers::Watchers;

const DETECT_EVERY: Duration = Duration::from_secs(2);
const JOURNAL_EVERY: Duration = Duration::from_secs(15);
const HASH_START: Duration = Duration::from_secs(20);
const HASH_EVERY: Duration = Duration::from_secs(30);
const HEALTH_START: Duration = Duration::from_secs(10);
const HEALTH_EVERY: Duration = Duration::from_secs(3600);
/// How often the health thread looks for a disk plugged in or removed.
const DISKS_CHANGED_CHECK: Duration = Duration::from_secs(5);
/// How soon a NAS whose last reading failed is read again.
const NAS_RETRY: Duration = Duration::from_secs(300);

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
    // Content hashes in the background (F06), after the start has settled.
    let (a, s) = (app.clone(), Arc::clone(state));
    let _ = std::thread::Builder::new()
        .name("prism-hashing".into())
        .spawn(move || {
            std::thread::sleep(HASH_START);
            loop {
                if crate::hash_jobs::tick(&s) {
                    let _ = a.emit("volumes-changed", ());
                }
                std::thread::sleep(HASH_EVERY);
            }
        });
    // Health of the disks (F17): read soon after the start, then every hour, and at once when a
    // disk is plugged in (its card shows its health and its physical disk, REVIEW D64).
    let (a, s) = (app.clone(), Arc::clone(state));
    let _ = std::thread::Builder::new()
        .name("prism-health".into())
        .spawn(move || {
            std::thread::sleep(HEALTH_START);
            loop {
                let disks = crate::health_state::local_disks(&s);
                let (worse, changed) = crate::health_state::tick(&s);
                for name in worse {
                    let n = s.notices().health_notice(&name);
                    if let Err(e) = prism_win::notify::show_toast(&s.app_id, &n.title, &n.body) {
                        tracing::warn!(error = %e, "health notification not shown");
                    }
                }
                if changed {
                    let _ = a.emit("volumes-changed", ());
                }
                let read_at = Instant::now();
                while read_at.elapsed() < HEALTH_EVERY
                    && crate::health_state::local_disks(&s) == disks
                {
                    std::thread::sleep(DISKS_CHANGED_CHECK);
                }
            }
        });
    // NAS (REVIEW D66): the arrays and disks of the NAS with a login, every hour; every five
    // minutes while the last reading failed (the NAS was off, the network away).
    let (a, s) = (app.clone(), Arc::clone(state));
    let _ = std::thread::Builder::new()
        .name("prism-nas".into())
        .spawn(move || {
            std::thread::sleep(HEALTH_START);
            loop {
                let (problems, changed) = s.nas_tick();
                for name in problems {
                    let n = s.notices().nas_notice(&name);
                    if let Err(e) = prism_win::notify::show_toast(&s.app_id, &n.title, &n.body) {
                        tracing::warn!(error = %e, "NAS notification not shown");
                    }
                }
                if changed {
                    let _ = a.emit("volumes-changed", ());
                }
                std::thread::sleep(if s.nas.any_error() {
                    NAS_RETRY
                } else {
                    HEALTH_EVERY
                });
            }
        });
    // Search indexes (ADR-0010): rebuilt from catalog events, at most every 30 s per volume.
    let s = Arc::clone(state);
    let _ = std::thread::Builder::new()
        .name("prism-search-index".into())
        .spawn(move || {
            static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            s.search.maintain(&s.store, Duration::from_secs(30), &NEVER);
        });
}

fn volume_monitor(app: &AppHandle, state: &Arc<AppState>) {
    let mut last_journal = Instant::now();
    let mut watchers = Watchers::default();
    let mut needs_reindex: HashMap<VolumeId, Instant> = HashMap::new();
    loop {
        if let Err(e) = detect_once(app, state) {
            tracing::warn!(error = %e, "volume detection failed");
        }
        watchers.sync(state);
        if last_journal.elapsed() >= JOURNAL_EVERY {
            last_journal = Instant::now();
            journal_tick(state, &mut needs_reindex);
        }
        std::thread::sleep(DETECT_EVERY);
    }
}

/// The network shares of the catalog that no drive letter points to now — a mapping not
/// restored at sign-in, or one that only another session sees (Prism runs elevated, ADR-0016):
/// they are looked for by their network path, so a share is connected whenever its server
/// answers, with or without a letter.
pub(crate) fn known_shares(known: &[Volume], detected: &[DetectedVolume]) -> Vec<DetectedVolume> {
    let mapped: Vec<String> = detected
        .iter()
        .filter_map(|d| d.info.unc_path.as_deref())
        .map(normalize_unc)
        .collect();
    known
        .iter()
        .filter(|v| v.info.kind == VolumeKind::Network && v.status != VolumeStatus::Ignored)
        .filter_map(|v| {
            let unc = v.info.unc_path.clone()?;
            if unc.is_empty() || mapped.contains(&normalize_unc(&unc)) {
                return None;
            }
            let mut info = v.info.clone();
            info.capacity = None;
            info.free_space = None;
            info.last_mount = Some(unc.clone());
            Some(DetectedVolume {
                info,
                mounts: vec![unc],
                disks: Vec::new(),
            })
        })
        .collect()
}

/// The catalog shares whose server no longer answers at its IP address but answers to its
/// name at another one (REVIEW D59): the same share is tried there. A share found this way does
/// not match the catalog by its path, so the user is asked whether it is the one that moved.
pub(crate) fn moved_shares(
    known: &[Volume],
    detected: &[DetectedVolume],
    net: &NetProbe,
    new_addresses: impl Fn(&str) -> Vec<String>,
) -> Vec<DetectedVolume> {
    let mut taken: Vec<String> = known
        .iter()
        .filter_map(|v| v.info.unc_path.as_deref())
        .chain(detected.iter().filter_map(|d| d.info.unc_path.as_deref()))
        .map(normalize_unc)
        .collect();
    let mut found = Vec::new();
    for v in known
        .iter()
        .filter(|v| v.info.kind == VolumeKind::Network && v.status != VolumeStatus::Ignored)
    {
        let Some(unc) = v.info.unc_path.as_deref() else {
            continue;
        };
        if net.status(unc) != NetStatus::Unreachable {
            continue;
        }
        // `\\server\share\...`: the server is replaced, the rest kept as written.
        let Some((_, share_path)) = unc.strip_prefix(r"\\").and_then(|r| r.split_once('\\')) else {
            continue;
        };
        let Some((server, _)) = volumes::unc_parts(unc) else {
            continue;
        };
        for address in new_addresses(&server) {
            let candidate = format!(r"\\{address}\{share_path}");
            let key = normalize_unc(&candidate);
            if taken.contains(&key) {
                continue;
            }
            taken.push(key);
            let mut info = v.info.clone();
            info.unc_path = Some(candidate.clone());
            info.last_mount = Some(candidate.clone());
            info.capacity = None;
            info.free_space = None;
            found.push(DetectedVolume {
                info,
                mounts: vec![candidate],
                disks: Vec::new(),
            });
        }
    }
    found
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
    let mut detected = WinVolumeSource.detect().map_err(|e| e.to_string())?;
    if let Ok(known) = state.store.read(store::volumes) {
        let shares = known_shares(&known, &detected);
        detected.extend(shares);
        let moved = moved_shares(&known, &detected, &state.net, |server| {
            state.names.moved(server)
        });
        detected.extend(moved);
    }
    let detected = apply_probe(detected, &state.net);
    // The names of the servers that answer, to find them again if their address changes.
    for d in detected
        .iter()
        .filter(|d| d.info.kind == VolumeKind::Network)
    {
        if let Some((server, _)) = d.info.unc_path.as_deref().and_then(volumes::unc_parts) {
            state.names.reached(&server);
        }
    }
    for (address, name) in state.names.take_learned() {
        if let Err(e) = state
            .store
            .write(move |w| store::set_share_server(w, &address, &name))
        {
            tracing::warn!(error = %e, "network server name not saved");
        }
    }
    let (changed, came_back) = apply_detection(state, detected)?;
    // F05-AC2: an indexed volume that comes back is brought up to date.
    for id in came_back {
        if let Err(e) = state.submit_index(id, IndexKind::Update) {
            tracing::warn!(volume = id.0, code = %e.code, "cannot update the reconnected volume");
        }
    }
    if changed {
        let _ = app.emit("volumes-changed", ());
    }
    announce_new_volumes(app, state);
    Ok(())
}

/// Matches the detected volumes with the catalog and records what changed: known volumes that
/// came back or moved, new volumes, ambiguous ones waiting for the user (F02-AC3) or resolved
/// by a choice. Returns whether the list changed and the volumes that came back.
pub(crate) fn apply_detection(
    state: &AppState,
    detected: Vec<DetectedVolume>,
) -> Result<(bool, Vec<VolumeId>), String> {
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
    let mut waiting = Vec::new();
    for (d, m) in detected.into_iter().zip(matches) {
        // An ambiguous volume the user has decided about becomes known or new.
        let m = match m {
            VolumeMatch::Ambiguous(ids) => {
                let key = ambiguity_key(&d.info);
                match state.identities.take_choice(&key) {
                    Some(Some(id)) if ids.contains(&id) && !now.contains_key(&id) => {
                        tracing::info!(volume = id.0, "ambiguous volume identified by the user");
                        VolumeMatch::Known(id)
                    }
                    Some(None) => {
                        tracing::info!(mount = ?d.info.last_mount, "ambiguous volume is a new disk");
                        VolumeMatch::New
                    }
                    other => {
                        if let Some(choice) = other {
                            // Chosen volume taken by another connected disk: ask again later.
                            state.identities.keep_choice(key.clone(), choice);
                        }
                        VolumeMatch::Ambiguous(ids)
                    }
                }
            }
            m => m,
        };
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
                    state.notices().push(notice_name(&d.info), Instant::now());
                    now.insert(id, d);
                    changed = true;
                }
            }
            VolumeMatch::Ambiguous(ids) => {
                // The user must choose (F02-AC3); not recorded until then (REVIEW D13).
                waiting.push(Ambiguous {
                    key: ambiguity_key(&d.info),
                    detected: d,
                    candidates: ids.clone(),
                });
            }
        }
    }
    if now.len() != previous.len() {
        changed = true;
    }
    *state.online() = now;
    let mut pending = state.identities.pending();
    let keys = |list: &[Ambiguous]| list.iter().map(|a| a.key.clone()).collect::<Vec<_>>();
    if keys(&pending) != keys(&waiting) {
        for a in waiting
            .iter()
            .filter(|a| !pending.iter().any(|p| p.key == a.key))
        {
            tracing::warn!(ids = ?a.candidates, mount = ?a.detected.info.last_mount, "ambiguous volume identity");
        }
        changed = true;
    }
    *pending = waiting;
    Ok((changed, came_back))
}

/// F02-AC1: while the window is hidden, new disks are announced with a Windows notification;
/// when it is visible, its own panel shows them.
fn announce_new_volumes(app: &AppHandle, state: &AppState) {
    let visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    let notice = {
        let mut notices = state.notices();
        if visible {
            notices.clear();
            return;
        }
        notices.take_due(Instant::now())
    };
    if let Some(n) = notice
        && let Err(e) = prism_win::notify::show_toast(&state.app_id, &n.title, &n.body)
    {
        tracing::warn!(error = %e, "cannot show the new-disk notification");
    }
}

/// How long a connected NTFS volume stays `reindex_needed` before Prism reads it again by
/// itself (seen in a diagnostic package: C: stayed so for hours, not updated by the journal).
const REINDEX_AFTER: Duration = Duration::from_secs(30 * 60);

/// F05-AC1: while Prism runs, indexed NTFS volumes are updated from their change journal.
fn journal_tick(state: &AppState, needs_reindex: &mut HashMap<VolumeId, Instant>) {
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
        if state.foreground_jobs(id).is_empty() {
            let _ = state.submit_index(id, IndexKind::Journal);
        }
    }
    // A connected NTFS volume left needing a reindex (a journal update failed or was stopped)
    // is read again from its MFT, which takes seconds, instead of waiting for the next start of
    // Prism; not at once, so a disk the user just stopped is not restarted under their eyes.
    let stale: Vec<VolumeId> = {
        let online = state.online();
        known
            .iter()
            .filter(|v| v.status == VolumeStatus::ReindexNeeded && v.last_indexed_at.is_some())
            .filter(|v| online.get(&v.id).is_some_and(is_local_ntfs))
            .map(|v| v.id)
            .collect()
    };
    needs_reindex.retain(|id, _| stale.contains(id));
    for id in stale {
        let since = *needs_reindex.entry(id).or_insert_with(Instant::now);
        if since.elapsed() >= REINDEX_AFTER && state.foreground_jobs(id).is_empty() {
            tracing::info!(volume = id.0, "reading again a volume that needs it");
            if state.submit_index(id, IndexKind::Update).is_ok() {
                needs_reindex.insert(id, Instant::now());
            }
        }
    }
}

/// A connected local NTFS volume (one with a change journal Prism reads).
fn is_local_ntfs(d: &DetectedVolume) -> bool {
    d.info.kind != VolumeKind::Network
        && d.info
            .fs_type
            .as_deref()
            .is_some_and(|f| f.eq_ignore_ascii_case("NTFS"))
}

fn job_relay(app: &AppHandle, state: &AppState) {
    let events = state.engine.subscribe();
    loop {
        // Wait for a change, or poll progress while something runs (~4 per second).
        let event = events.recv_timeout(Duration::from_millis(250));
        let jobs = state.engine.jobs();
        let active = jobs.iter().filter(|j| !j.state.is_finished()).count();
        let running = active > 0;
        crate::tray::set_job_count(app, active);
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

#[cfg(test)]
mod tests {
    use super::known_shares;
    use prism_core::model::{Volume, VolumeId, VolumeInfo, VolumeKind, VolumeLabels, VolumeStatus};
    use prism_core::volumes::DetectedVolume;

    fn share(id: i64, unc: &str, status: VolumeStatus) -> Volume {
        Volume {
            id: VolumeId(id),
            info: VolumeInfo {
                kind: VolumeKind::Network,
                fs_type: None,
                fs_serial: None,
                volume_guid: None,
                unc_path: Some(unc.to_owned()),
                os_label: None,
                capacity: Some(1),
                free_space: Some(1),
                bus: None,
                last_mount: Some(format!(
                    r"{}:\",
                    char::from(b'W' + u8::try_from(id).unwrap())
                )),
            },
            labels: VolumeLabels::default(),
            status,
            first_seen_at: 0,
            last_seen_at: None,
            last_indexed_at: None,
            usn: None,
        }
    }

    // A share mapped to a letter now is not added twice; one that no letter points to is looked
    // for by its network path; an ignored one is not.
    #[test]
    fn known_shares_without_a_letter_are_looked_for_by_their_path() {
        let known = vec![
            share(1, r"\\nas\Foto", VolumeStatus::Indexed),
            share(2, r"\\NAS\archivio", VolumeStatus::New),
            share(3, r"\\nas\vecchio", VolumeStatus::Ignored),
        ];
        let mapped = DetectedVolume {
            info: VolumeInfo {
                unc_path: Some(r"\\NAS\foto\".to_owned()),
                ..known[0].info.clone()
            },
            mounts: vec![r"X:\".to_owned()],
            disks: Vec::new(),
        };
        let found = known_shares(&known, &[mapped]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].mounts, vec![r"\\NAS\archivio".to_owned()]);
        assert_eq!(found[0].info.last_mount.as_deref(), Some(r"\\NAS\archivio"));
        assert_eq!(found[0].info.capacity, None, "the probe says how big it is");
    }

    // REVIEW D59: a share whose server stopped answering at its address is tried where its
    // server's name answers now; not when it still answers, nor at a path of another share.
    #[test]
    fn a_share_gone_from_its_address_is_tried_at_the_new_one() {
        use super::moved_shares;
        use crate::netprobe::{NetProbe, NetStatus, ProbeTiming};
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let known = vec![
            share(1, r"\\192.168.1.65\ZimaOS-HD", VolumeStatus::Indexed),
            share(2, r"\\192.168.1.65\Big-Storage", VolumeStatus::Indexed),
            share(3, r"\\192.168.1.57\Big-Storage", VolumeStatus::New),
            share(4, r"\\192.168.1.70\Media", VolumeStatus::Indexed),
        ];
        let net = Arc::new(NetProbe::new(
            Arc::new(|unc: &str| unc.contains(".70").then_some((1, 1))),
            ProbeTiming::default(),
        ));
        net.want(known.iter().filter_map(|v| v.info.unc_path.as_deref()));
        let start = Instant::now();
        while net.status(r"\\192.168.1.65\ZimaOS-HD") == NetStatus::Unknown
            || net.status(r"\\192.168.1.70\Media") == NetStatus::Unknown
        {
            assert!(start.elapsed() < Duration::from_secs(5));
            net.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        let new_addresses = |server: &str| match server {
            "192.168.1.65" => vec!["192.168.1.57".to_owned()],
            "192.168.1.70" => vec!["192.168.1.71".to_owned()],
            _ => Vec::new(),
        };
        let found = moved_shares(&known, &[], &net, new_addresses);
        // Big-Storage at .57 is already a share of the catalog; Media still answers.
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].info.unc_path.as_deref(),
            Some(r"\\192.168.1.57\ZimaOS-HD")
        );
        assert_eq!(
            found[0].mounts,
            vec![r"\\192.168.1.57\ZimaOS-HD".to_owned()]
        );
        assert_eq!(found[0].info.capacity, None);
    }
}
