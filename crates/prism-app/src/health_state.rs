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
    let numbers: BTreeSet<u32> = state
        .online()
        .values()
        .filter(|d| d.info.kind != VolumeKind::Network)
        .flat_map(|d| d.disks.iter().copied())
        .collect();
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
        let snapshot = DiskSnapshot {
            key,
            model: disk.identity.model,
            serial: disk.identity.serial,
            bus: disk.identity.bus.as_str(),
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
        let history = self.store.read(|c| store::health_history(c, &s.key, 30))?;
        let r = s.reading.as_ref().ok();
        Ok(Some(DiskHealthView {
            model: s.model.clone(),
            serial: s.serial.clone(),
            bus: s.bus.to_owned(),
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
