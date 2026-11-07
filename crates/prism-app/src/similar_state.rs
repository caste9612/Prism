//! Similar images in the app (F11): groups from the prints in the catalog, thumbnails of the
//! connected files on demand.

use std::collections::HashMap;

use prism_core::media::{self, bktree};
use prism_core::model::{EntryId, VolumeId};
use prism_core::names;
use prism_core::store;

use crate::dto::ApiError;
use crate::similar_views::{
    self, MAX_DISTANCE, MAX_GROUPS, SimilarFileView, SimilarGroupView, SimilarView, THUMB_SIDE,
};
use crate::state::{AppState, access_path};

impl AppState {
    /// Groups of images within `max_distance` bits of each other, optionally only those with a
    /// copy on `volume` (F11-AC1/AC2/AC3).
    pub fn similar_images(
        &self,
        max_distance: u32,
        volume: Option<VolumeId>,
    ) -> Result<SimilarView, ApiError> {
        let max_distance = max_distance.min(MAX_DISTANCE);
        let mounts: HashMap<VolumeId, Option<String>> = self
            .online()
            .iter()
            .map(|(id, d)| {
                (
                    *id,
                    access_path(d).map(|p| p.to_string_lossy().into_owned()),
                )
            })
            .collect();
        let (prints, volumes) = self
            .store
            .read(|c| Ok((store::valid_prints(c)?, store::volumes(c)?)))?;
        let hashes: Vec<u64> = prints.iter().map(|p| p.dhash).collect();
        let mut found = bktree::groups(&hashes, max_distance);
        if let Some(v) = volume {
            found.retain(|g| g.iter().any(|&i| prints[i].volume == v));
        }
        let bytes = |g: &Vec<usize>| g.iter().map(|&i| prints[i].size).sum::<u64>();
        found.sort_by(|a, b| b.len().cmp(&a.len()).then(bytes(b).cmp(&bytes(a))));
        let total_groups = found.len() as u64;
        found.truncate(MAX_GROUPS);

        let by_id: HashMap<VolumeId, _> = volumes.iter().map(|v| (v.id, v)).collect();
        let by_entry: HashMap<EntryId, &store::ImagePrint> =
            prints.iter().map(|p| (p.entry, p)).collect();
        let groups = self.store.read(|c| {
            let mut out = Vec::with_capacity(found.len());
            for g in &found {
                let mut members: Vec<_> = g
                    .iter()
                    .map(|&i| {
                        (
                            prints[i].volume,
                            prints[i].entry,
                            prints[i].width,
                            prints[i].height,
                        )
                    })
                    .collect();
                similar_views::order_members(&mut members);
                let first = members
                    .first()
                    .and_then(|m| by_entry.get(&m.1))
                    .map_or(0, |p| p.dhash);
                let mut files = Vec::with_capacity(members.len());
                for (volume, entry, width, height) in members {
                    let Some(p) = by_entry.get(&entry) else {
                        continue;
                    };
                    let rel = store::relative_path(c, entry)?;
                    let path = rel
                        .split('\\')
                        .filter(|s| !s.is_empty())
                        .map(|s| names::decode(s).to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("\\");
                    let (disk, location, mount) = by_id.get(&volume).map_or_else(
                        || (format!("#{}", volume.0), None, String::new()),
                        |v| similar_views::volume_labels(v, mounts.get(&volume).cloned().flatten()),
                    );
                    let full_path = if mount.is_empty() {
                        path.clone()
                    } else {
                        format!("{}\\{}", mount.trim_end_matches('\\'), path)
                    };
                    files.push(SimilarFileView {
                        entry_id: entry.0,
                        volume_id: volume.0,
                        disk,
                        location,
                        path,
                        full_path,
                        width,
                        height,
                        size: p.size,
                        online: mounts.contains_key(&volume),
                        distance: bktree::distance(first, p.dhash),
                    });
                }
                out.push(SimilarGroupView {
                    key: files
                        .first()
                        .map(|f| f.entry_id.to_string())
                        .unwrap_or_default(),
                    files,
                });
            }
            Ok(out)
        })?;
        Ok(SimilarView {
            groups,
            total_groups,
            pending_images: self.hashing.pending_images(),
        })
    }

    /// A JPEG thumbnail of a connected image (F11-AC3), kept in a small cache.
    pub fn image_thumbnail(&self, volume: VolumeId, entry: EntryId) -> Result<Vec<u8>, ApiError> {
        if let Some(t) = self.thumbnails.get(entry) {
            return Ok(t);
        }
        let path = self.entry_path(volume, entry)?;
        let jpeg = media::thumbnail_jpeg(&path, THUMB_SIDE)
            .map_err(|e| ApiError::new("thumbnail_failed", e))?;
        self.thumbnails.put(entry, jpeg.clone());
        Ok(jpeg)
    }
}
