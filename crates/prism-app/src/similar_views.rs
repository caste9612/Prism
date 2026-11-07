//! Similar images (F11) for the interface: groups of images whose prints are close, on every
//! disk, with resolution and size (F11-AC3); thumbnails are asked one by one.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use prism_core::model::{EntryId, Volume, VolumeId};
use serde::Serialize;
use ts_rs::TS;

/// Groups sent at most (most images first); the total counts all of them.
pub const MAX_GROUPS: usize = 200;
/// The largest distance the interface may ask for (beyond it everything looks alike).
pub const MAX_DISTANCE: u32 = 20;
/// Side of the thumbnails, in pixels.
pub const THUMB_SIDE: u32 = 192;
/// Thumbnails kept in memory.
const THUMB_CACHE: usize = 600;

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SimilarFileView {
    #[ts(type = "number")]
    pub entry_id: i64,
    #[ts(type = "number")]
    pub volume_id: i64,
    pub disk: String,
    pub location: Option<String>,
    /// Path relative to the volume root, name included.
    pub path: String,
    pub full_path: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[ts(type = "number")]
    pub size: u64,
    pub online: bool,
    /// Bits different from the first image of the group (0: same print).
    pub distance: u32,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SimilarGroupView {
    pub key: String,
    pub files: Vec<SimilarFileView>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SimilarView {
    pub groups: Vec<SimilarGroupView>,
    #[ts(type = "number")]
    pub total_groups: u64,
    /// Images not printed yet on the indexed disks: more groups may appear.
    #[ts(type = "number")]
    pub pending_images: u64,
}

/// Disk name, location and mount of a volume.
pub fn volume_labels(v: &Volume, mount: Option<String>) -> (String, Option<String>, String) {
    (
        crate::search_views::disk_name(v),
        v.labels.location.clone().filter(|l| !l.trim().is_empty()),
        mount
            .or_else(|| v.info.unc_path.clone())
            .or_else(|| v.info.last_mount.clone())
            .unwrap_or_default(),
    )
}

/// Entries in the order they were added, and their thumbnails.
type Cache = (Vec<EntryId>, HashMap<EntryId, Vec<u8>>);

/// A small cache of thumbnails, by entry.
#[derive(Default)]
pub struct Thumbnails {
    items: Mutex<Cache>,
}

impl Thumbnails {
    pub fn get(&self, entry: EntryId) -> Option<Vec<u8>> {
        self.items
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
            .get(&entry)
            .cloned()
    }

    pub fn put(&self, entry: EntryId, jpeg: Vec<u8>) {
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        if items.1.insert(entry, jpeg).is_none() {
            items.0.push(entry);
        }
        while items.0.len() > THUMB_CACHE {
            let old = items.0.remove(0);
            items.1.remove(&old);
        }
    }
}

/// Orders the members of a group: the largest resolution first (most likely the original),
/// then by entry.
pub fn order_members(members: &mut [(VolumeId, EntryId, Option<u32>, Option<u32>)]) {
    members.sort_by_key(|(_, e, w, h)| {
        (
            std::cmp::Reverse(u64::from(w.unwrap_or(0)) * u64::from(h.unwrap_or(0))),
            *e,
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_cache_keeps_the_latest() {
        let cache = Thumbnails::default();
        for i in 0..(THUMB_CACHE as i64 + 5) {
            cache.put(EntryId(i), vec![1]);
        }
        assert!(cache.get(EntryId(0)).is_none());
        assert!(cache.get(EntryId(THUMB_CACHE as i64 + 4)).is_some());
    }

    #[test]
    fn largest_image_first() {
        let mut m = vec![
            (VolumeId(1), EntryId(5), Some(100), Some(100)),
            (VolumeId(1), EntryId(3), Some(400), Some(300)),
            (VolumeId(2), EntryId(1), None, None),
        ];
        order_members(&mut m);
        assert_eq!(m.iter().map(|x| x.1.0).collect::<Vec<_>>(), [3, 5, 1]);
    }
}
