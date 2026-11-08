//! Volumes whose identity is ambiguous (F02-AC3, REVIEW D13, L7): a connected volume that could
//! be one of several catalog volumes (cloned disks with the same serial number, or a candidate
//! with another capacity). The user says which one it is, or that it is another disk; the
//! volume monitor applies the choice at the next detection. Once applied, the catalog volume has
//! the GUID of the connected one, so the next detections recognise it by themselves.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use prism_core::model::{VolumeId, VolumeInfo, VolumeKind};
use prism_core::store;
use prism_core::volumes::{DetectedVolume, normalize_unc};
use serde::Serialize;
use ts_rs::TS;

use crate::dto::ApiError;
use crate::search_views::disk_name;
use crate::state::AppState;

/// A connected volume waiting for the user's choice.
#[derive(Clone, Debug)]
pub struct Ambiguous {
    pub key: String,
    pub detected: DetectedVolume,
    pub candidates: Vec<VolumeId>,
}

#[derive(Default)]
pub struct Identities {
    pending: Mutex<Vec<Ambiguous>>,
    /// Choices not applied yet: the catalog volume, or `None` for a new disk.
    choices: Mutex<HashMap<String, Option<VolumeId>>>,
}

impl Identities {
    pub fn pending(&self) -> MutexGuard<'_, Vec<Ambiguous>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn choices(&self) -> MutexGuard<'_, HashMap<String, Option<VolumeId>>> {
        self.choices.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The choice for a volume, removed once taken by the monitor.
    pub fn take_choice(&self, key: &str) -> Option<Option<VolumeId>> {
        self.choices().remove(key)
    }

    /// Puts back a choice that could not be applied yet.
    pub fn keep_choice(&self, key: String, choice: Option<VolumeId>) {
        self.choices().insert(key, choice);
    }
}

/// What tells the connected volume apart while it waits: its network path for a share (REVIEW
/// D59), its GUID on this machine, or its mount, serial number and capacity.
#[must_use]
pub fn ambiguity_key(info: &VolumeInfo) -> String {
    if info.kind == VolumeKind::Network
        && let Some(unc) = info.unc_path.as_deref()
    {
        return format!("net|{}", normalize_unc(unc));
    }
    info.volume_guid.clone().unwrap_or_else(|| {
        format!(
            "{}|{:?}|{:?}",
            info.last_mount.as_deref().unwrap_or(""),
            info.fs_serial,
            info.capacity
        )
    })
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CandidateView {
    #[ts(type = "number")]
    pub volume_id: i64,
    pub name: String,
    pub location: Option<String>,
    pub mount: Option<String>,
    /// Its network path, for a network share.
    pub path: Option<String>,
    #[ts(type = "number | null")]
    pub capacity: Option<u64>,
    #[ts(type = "number | null")]
    pub last_seen_at: Option<i64>,
    #[ts(type = "number | null")]
    pub last_indexed_at: Option<i64>,
}

/// A connected disk Prism cannot tell apart, with the catalog disks it could be (F02-AC3).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AmbiguousView {
    pub key: String,
    /// A network share with the name of a catalog share, at another address (REVIEW D59).
    pub network: bool,
    /// Its network path, for a network share.
    pub path: Option<String>,
    pub label: Option<String>,
    pub mount: Option<String>,
    pub fs_type: Option<String>,
    #[ts(type = "number | null")]
    pub capacity: Option<u64>,
    pub candidates: Vec<CandidateView>,
}

impl AppState {
    /// The connected disks waiting for the user's choice.
    pub fn ambiguous_volumes(&self) -> Result<Vec<AmbiguousView>, ApiError> {
        let pending = self.identities.pending().clone();
        if pending.is_empty() {
            return Ok(Vec::new());
        }
        let volumes = self.store.read(store::volumes)?;
        Ok(pending
            .into_iter()
            .map(|a| AmbiguousView {
                network: a.detected.info.kind == VolumeKind::Network,
                path: a.detected.info.unc_path.clone(),
                label: a.detected.info.os_label.clone(),
                mount: a.detected.info.last_mount.clone(),
                fs_type: a.detected.info.fs_type.clone(),
                capacity: a.detected.info.capacity,
                candidates: a
                    .candidates
                    .iter()
                    .filter_map(|id| volumes.iter().find(|v| v.id == *id))
                    .map(|v| CandidateView {
                        volume_id: v.id.0,
                        name: disk_name(v),
                        location: v.labels.location.clone().filter(|l| !l.trim().is_empty()),
                        mount: v.info.last_mount.clone(),
                        path: v.info.unc_path.clone(),
                        capacity: v.info.capacity,
                        last_seen_at: v.last_seen_at,
                        last_indexed_at: v.last_indexed_at,
                    })
                    .collect(),
                key: a.key,
            })
            .collect())
    }

    /// Records the user's choice for an ambiguous disk: one of its candidates, or `None` for a
    /// disk that is not in the catalog. The monitor applies it at the next detection.
    pub fn resolve_ambiguous(&self, key: &str, choice: Option<VolumeId>) -> Result<(), ApiError> {
        let pending = self.identities.pending();
        let Some(a) = pending.iter().find(|a| a.key == key) else {
            return Err(ApiError::new("volume_not_found", key));
        };
        if let Some(id) = choice
            && !a.candidates.contains(&id)
        {
            return Err(ApiError::new("volume_not_found", id.0));
        }
        drop(pending);
        self.identities.keep_choice(key.to_owned(), choice);
        Ok(())
    }
}
