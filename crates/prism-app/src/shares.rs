//! Network folders added by hand with their path (F02-AC5), and a network share of the catalog
//! moved to another path, by hand (REVIEW D59): the user types
//! the new path (`\\192.168.1.57\Photos`), Prism checks that it answers and how much of its top
//! level matches the catalog, and the user confirms. The volume monitor then finds the share at
//! its new path and brings it up to date like any share that comes back (F05-AC2).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use prism_core::model::{Volume, VolumeId, VolumeInfo, VolumeKind, VolumeStatus};
use prism_core::names;
use prism_core::store;
use prism_core::volumes::{normalize_unc, unc_parts};
use serde::Serialize;
use ts_rs::TS;

use crate::dto::ApiError;
use crate::state::AppState;

/// Longest wait for a share to list its top level: a server that does not answer can block for
/// tens of seconds.
const LIST_TIMEOUT: Duration = Duration::from_secs(8);

/// What Prism found at the new path of a share.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SharePathView {
    /// The path as it will be saved.
    pub path: String,
    /// It answers.
    pub reachable: bool,
    /// Names at the top of the share that the catalog has too…
    #[ts(type = "number")]
    pub matching: u64,
    /// …out of the names at the top of the share in the catalog.
    #[ts(type = "number")]
    pub total: u64,
}

/// The names at the top of a folder, or `None` when it cannot be read in time.
fn top_names(path: PathBuf) -> Option<HashSet<String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("prism-share-check".into())
        .spawn(move || {
            let names = std::fs::read_dir(&path).ok().map(|dir| {
                dir.filter_map(Result::ok)
                    .map(|e| names::fold(&e.file_name().to_string_lossy()))
                    .collect::<HashSet<String>>()
            });
            let _ = tx.send(names);
        })
        .ok()?;
    rx.recv_timeout(LIST_TIMEOUT).ok().flatten()
}

/// A network path as Prism saves it (`\\server\share\…`, backslashes, no final one), or
/// `share_path_invalid`.
fn clean_unc(path: &str) -> Result<String, ApiError> {
    let path = path.trim().trim_end_matches(['\\', '/']).replace('/', "\\");
    if !path.starts_with(r"\\") || path.starts_with(r"\\?\") || unc_parts(&path).is_none() {
        return Err(ApiError::new("share_path_invalid", path));
    }
    Ok(path)
}

impl AppState {
    /// Adds a network folder to the catalog by its path (F02-AC5): one that no drive letter
    /// points to (a mapping that is not persistent, a folder of another PC or NAS). It must
    /// answer; the volume monitor then finds it by its path, like every share of the catalog
    /// (REVIEW D55), and proposes to index it.
    pub fn add_share(&self, path: &str) -> Result<VolumeId, ApiError> {
        let path = clean_unc(path)?;
        let key = normalize_unc(&path);
        let volumes = self.store.read(store::volumes)?;
        if volumes
            .iter()
            .any(|v| v.info.unc_path.as_deref().map(normalize_unc).as_ref() == Some(&key))
        {
            return Err(ApiError::new("share_in_catalog", path));
        }
        if top_names(PathBuf::from(&path)).is_none() {
            return Err(ApiError::new("share_unreachable", path));
        }
        let info = VolumeInfo {
            kind: VolumeKind::Network,
            unc_path: Some(path.clone()),
            ..VolumeInfo::dev_folder(&path)
        };
        let id = self
            .store
            .write(move |w| store::insert_volume(w, &info, VolumeStatus::New))?;
        tracing::info!(volume = id.0, path = %path, "network folder added by hand");
        Ok(id)
    }

    /// The share and the cleaned new path, if the change makes sense.
    fn share_target(&self, volume: VolumeId, path: &str) -> Result<(Volume, String), ApiError> {
        let path = clean_unc(path)?;
        let volumes = self.store.read(store::volumes)?;
        let Some(v) = volumes.iter().find(|v| v.id == volume).cloned() else {
            return Err(ApiError::new("volume_not_found", volume.to_string()));
        };
        if v.info.kind != VolumeKind::Network {
            return Err(ApiError::new("volume_not_network", volume.to_string()));
        }
        let key = normalize_unc(&path);
        if volumes.iter().any(|o| {
            o.id != volume && o.info.unc_path.as_deref().map(normalize_unc).as_ref() == Some(&key)
        }) {
            return Err(ApiError::new("share_in_catalog", path));
        }
        Ok((v, path))
    }

    /// Checks the new path of a share: whether it answers and how much of its top level the
    /// catalog has (the same share should have most of it).
    pub fn check_share_path(
        &self,
        volume: VolumeId,
        path: &str,
    ) -> Result<SharePathView, ApiError> {
        let (_, path) = self.share_target(volume, path)?;
        let catalog: Vec<String> = self.store.read(move |c| {
            let Some(root) = store::root_entry(c, volume)? else {
                return Ok(Vec::new());
            };
            Ok(store::children(c, root.id)?
                .into_iter()
                .map(|e| names::fold(&e.name))
                .collect())
        })?;
        let Some(found) = top_names(PathBuf::from(&path)) else {
            return Ok(SharePathView {
                path,
                reachable: false,
                matching: 0,
                total: catalog.len() as u64,
            });
        };
        Ok(SharePathView {
            path,
            reachable: true,
            matching: catalog.iter().filter(|n| found.contains(*n)).count() as u64,
            total: catalog.len() as u64,
        })
    }

    /// Gives a share its new path after the user confirmed it; it must answer there.
    pub fn move_share(&self, volume: VolumeId, path: &str) -> Result<(), ApiError> {
        let (v, path) = self.share_target(volume, path)?;
        if top_names(PathBuf::from(&path)).is_none() {
            return Err(ApiError::new("share_unreachable", path));
        }
        let saved = path.clone();
        self.store
            .write(move |w| store::set_share_path(w, volume, &saved))?;
        // Found again at the new path by the next detection, as a share that came back.
        self.online().remove(&volume);
        tracing::info!(volume = volume.0, from = ?v.info.unc_path, to = %path, "network share moved");
        Ok(())
    }
}
