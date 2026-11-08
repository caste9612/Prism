//! Health of the disks in the app (F17): the physical disks of the connected local volumes are
//! read every hour; a reading is kept when something changed or once a day (F17-AC2); a disk
//! that gets worse, or is critical the first time it is seen, raises a notification (F17-AC3).

use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, MutexGuard, PoisonError};

use prism_core::health::{self, HealthReading};
use prism_core::model::{VolumeId, VolumeKind, unix_millis_now};
use prism_core::store;
use prism_core::volumes::DetectedVolume;
use serde::Serialize;
use ts_rs::TS;

use crate::dto::ApiError;
use crate::state::AppState;

const DAY_MS: i64 = 86_400_000;
/// Readings older than this are removed.
const KEEP_MS: i64 = 400 * DAY_MS;

/// The last reading of a physical disk.
#[derive(Clone, Debug)]
pub struct DiskSnapshot {
    pub key: String,
    pub model: String,
    pub serial: String,
    pub bus: &'static str,
    /// `ssd` or `hdd`, when known, and the speed of a hard disk (REVIEW D65).
    pub media: Option<&'static str>,
    pub rpm: Option<u32>,
    pub reading: Result<HealthReading, &'static str>,
}

#[derive(Default)]
pub struct HealthState {
    disks: Mutex<HashMap<u32, DiskSnapshot>>,
}

impl HealthState {
    fn disks(&self) -> MutexGuard<'_, HashMap<u32, DiskSnapshot>> {
        self.disks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn snapshot(&self, disk: u32) -> Option<DiskSnapshot> {
        self.disks().get(&disk).cloned()
    }
}

/// Whether a new reading differs in what matters for the history (the temperature changes all
/// the time and is not a reason to store one).
fn significant(a: &HealthReading, b: &HealthReading) -> bool {
    (
        a.status,
        a.wear_percent,
        a.reallocated,
        a.pending,
        a.media_errors,
        a.available_spare,
    ) != (
        b.status,
        b.wear_percent,
        b.reallocated,
        b.pending,
        b.media_errors,
        b.available_spare,
    )
}

/// The status shown on a card.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HealthBadgeView {
    /// `good`, `warning`, `critical`, or `unavailable` (F17-AC4).
    pub status: String,
    /// Why it is not available (`usb`, `virtual`, `no_access`, `not_supported`).
    pub unavailable: Option<String>,
    pub temperature: Option<i32>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HealthPointView {
    #[ts(type = "number")]
    pub at: i64,
    pub status: String,
    pub temperature: Option<i32>,
    #[ts(type = "number | null")]
    pub reallocated: Option<u64>,
    #[ts(type = "number | null")]
    pub pending: Option<u64>,
    #[ts(type = "number | null")]
    pub media_errors: Option<u64>,
    pub wear: Option<u32>,
}

/// The health of the disk of a volume, with its history (F17-AC1/AC2).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiskHealthView {
    pub model: String,
    pub serial: String,
    pub bus: String,
    pub media: Option<String>,
    pub rpm: Option<u32>,
    pub badge: HealthBadgeView,
    #[ts(type = "number | null")]
    pub power_on_hours: Option<u64>,
    pub wear: Option<u32>,
    #[ts(type = "number | null")]
    pub reallocated: Option<u64>,
    #[ts(type = "number | null")]
    pub pending: Option<u64>,
    #[ts(type = "number | null")]
    pub media_errors: Option<u64>,
    pub available_spare: Option<u32>,
    /// Why the status is not good (codes translated by the interface).
    pub reasons: Vec<String>,
    pub history: Vec<HealthPointView>,
}

pub fn badge(s: &DiskSnapshot) -> HealthBadgeView {
    match &s.reading {
        Ok(r) => HealthBadgeView {
            status: r.status.map_or("unavailable", |s| s.as_str()).to_owned(),
            unavailable: None,
            temperature: r.temperature_c,
        },
        Err(why) => HealthBadgeView {
            status: "unavailable".to_owned(),
            unavailable: Some((*why).to_owned()),
            temperature: None,
        },
    }
}

/// Reads the disks of the connected local volumes and records what changed; returns the names
/// of the disks that got worse (for the notification) and whether any card changes.
pub fn tick(state: &AppState) -> (Vec<String>, bool) {
    let numbers = local_disks(state);
    // The volumes on each disk (by their first one), to remember their physical disk.
    let volumes_on: Vec<(VolumeId, u32)> = state
        .online()
        .iter()
        .filter(|(_, d)| d.info.kind != VolumeKind::Network)
        .filter_map(|(id, d)| d.disks.first().map(|n| (*id, *n)))
        .collect();
    let known_disks = state.store.read(store::volume_disks).unwrap_or_default();
    let known_media = state.store.read(store::disk_media).unwrap_or_default();
    let now = unix_millis_now();
    let mut worse = Vec::new();
    let mut changed = false;
    for n in numbers {
        let Some(disk) = prism_win::health::read_disk(n) else {
            continue;
        };
        let key = disk.identity.key();
        if let Ok(r) = &disk.reading {
            let last = state
                .store
                .read(|c| store::health_history(c, &key, 1))
                .ok()
                .and_then(|h| h.into_iter().next());
            let alarm = match &last {
                Some(l) => health::worsening(&l.reading(), r),
                None => (r.status == Some(health::HealthStatus::Critical)).then_some("status"),
            };
            if let Some(reason) = alarm {
                tracing::warn!(disk = %disk.identity.model, reason, "disk health worse");
                worse.push(disk.identity.model.clone());
            }
            let keep = last
                .as_ref()
                .is_none_or(|l| significant(&l.reading(), r) || now - l.at >= DAY_MS);
            if keep {
                let (k, r) = (key.clone(), r.clone());
                if let Err(e) = state
                    .store
                    .write(move |w| store::add_health(w, &k, now, &r))
                {
                    tracing::warn!(error = %e, "disk health not saved");
                }
            }
        }
        // REVIEW D65: SSD or hard disk. A speed read once (with the rights IDENTIFY needs) is
        // kept when a later reading only knows that the disk spins.
        if let Some(m) = disk.identity.media.as_str() {
            let before = known_media.get(&key);
            let media = store::DiskMedia {
                media: m.to_owned(),
                rpm: disk
                    .identity
                    .media
                    .rpm()
                    .or_else(|| before.filter(|b| b.media == m).and_then(|b| b.rpm)),
            };
            if before != Some(&media) {
                let k = key.clone();
                match state
                    .store
                    .write(move |w| store::set_disk_media(w, &k, &media))
                {
                    Ok(()) => changed = true,
                    Err(e) => tracing::warn!(error = %e, "kind of a disk not saved"),
                }
            }
        }
        // REVIEW D64: the partitions of one disk are shown together, also when it is away.
        for (volume, _) in volumes_on.iter().filter(|(_, d)| *d == n) {
            if known_disks.get(volume) != Some(&key) {
                let (v, k) = (*volume, key.clone());
                match state.store.write(move |w| store::set_volume_disk(w, v, &k)) {
                    Ok(()) => changed = true,
                    Err(e) => tracing::warn!(error = %e, "physical disk of a volume not saved"),
                }
            }
        }
        let snapshot = DiskSnapshot {
            key,
            model: disk.identity.model,
            serial: disk.identity.serial,
            bus: disk.identity.bus.as_str(),
            media: disk.identity.media.as_str(),
            rpm: disk.identity.media.rpm(),
            reading: disk.reading,
        };
        let mut disks = state.health.disks();
        let before = disks.get(&n).map(badge).map(|b| (b.status, b.temperature));
        changed |= before != Some((badge(&snapshot).status, badge(&snapshot).temperature));
        disks.insert(n, snapshot);
    }
    let _ = state
        .store
        .write(move |w| store::prune_health(w, now - KEEP_MS));
    (worse, changed)
}

/// The physical disks of the connected local volumes: when they change (a disk plugged in), the
/// health is read at once instead of at the next hourly round.
pub fn local_disks(state: &AppState) -> BTreeSet<u32> {
    state
        .online()
        .values()
        .filter(|d| d.info.kind != VolumeKind::Network)
        .flat_map(|d| d.disks.iter().copied())
        .collect()
}

impl AppState {
    /// The physical disk of a connected local volume (the first one when it spans several).
    fn health_of(&self, volume: VolumeId) -> Option<DiskSnapshot> {
        let disk = self
            .online()
            .get(&volume)
            .filter(|d| d.info.kind != VolumeKind::Network)
            .and_then(|d| d.disks.first().copied())?;
        self.health.snapshot(disk)
    }

    /// The badge of a connected volume's card, when its disk has been read (takes the detected
    /// volume: the caller may hold the lock of the connected volumes).
    pub fn health_badge(&self, detected: &DetectedVolume) -> Option<HealthBadgeView> {
        if detected.info.kind == VolumeKind::Network {
            return None;
        }
        let disk = detected.disks.first().copied()?;
        self.health.snapshot(disk).as_ref().map(badge)
    }

    /// The health of the disk of a connected volume with its history.
    pub fn disk_health(&self, volume: VolumeId) -> Result<Option<DiskHealthView>, ApiError> {
        let Some(s) = self.health_of(volume) else {
            return Ok(None);
        };
        let (history, saved) = self.store.read(|c| {
            Ok((
                store::health_history(c, &s.key, 30)?,
                store::disk_media(c)?.remove(&s.key),
            ))
        })?;
        let r = s.reading.as_ref().ok();
        Ok(Some(DiskHealthView {
            model: s.model.clone(),
            serial: s.serial.clone(),
            bus: s.bus.to_owned(),
            // As saved: it keeps a speed that this reading could not ask for.
            media: saved
                .as_ref()
                .map(|m| m.media.clone())
                .or_else(|| s.media.map(str::to_owned)),
            rpm: s.rpm.or_else(|| saved.and_then(|m| m.rpm)),
            badge: badge(&s),
            power_on_hours: r.and_then(|r| r.power_on_hours),
            wear: r.and_then(|r| r.wear_percent),
            reallocated: r.and_then(|r| r.reallocated),
            pending: r.and_then(|r| r.pending),
            media_errors: r.and_then(|r| r.media_errors),
            available_spare: r.and_then(|r| r.available_spare),
            reasons: r
                .map(|r| r.reasons.iter().map(|s| (*s).to_owned()).collect())
                .unwrap_or_default(),
            history: history
                .into_iter()
                .map(|h| HealthPointView {
                    at: h.at,
                    status: h.status.as_str().to_owned(),
                    temperature: h.temperature_c,
                    reallocated: h.reallocated,
                    pending: h.pending,
                    media_errors: h.media_errors,
                    wear: h.wear_percent,
                })
                .collect(),
        }))
    }
}
