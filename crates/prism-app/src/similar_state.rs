//! Similar images (F11) and videos (F12) in the app: groups from the prints in the catalog,
//! thumbnails of the connected files on demand.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use prism_core::media::{self, bktree, video};
use prism_core::model::{EntryId, VolumeId};
use prism_core::names;
use prism_core::store::{self, StoreError};

use crate::dto::ApiError;
use crate::similar_views::{
    self, MAX_DISTANCE, MAX_FILES, MAX_GROUPS, SimilarFileView, SimilarGroupView, SimilarView,
    THUMB_SIDE,
};
use crate::state::{AppState, access_path};

/// Groups found at one distance are kept this long, unless the prints change before: the files
/// the catalog updates meanwhile (moved, changed, deleted outside Prism) show in them until then.
const KEEP_GROUPS: Duration = Duration::from_secs(10 * 60);

/// The print of an image without detail: at 9×8 pixels no pixel is darker than the one on its
/// right, as in an image all of one colour (a blank or transparent picture). On a real catalog
/// 10,022 images had it; they are not similar photos, and they joined everything else.
const BLANK_PRINT: u64 = 0;

/// The groups of similar images at one distance, with the images in them only.
struct ImageGroups {
    generation: u64,
    max: u32,
    at: Instant,
    items: Vec<Item>,
    hashes: Vec<u64>,
    /// Positions in `items`.
    groups: Vec<Vec<usize>>,
}

/// The last groups of similar images (REVIEW L34). Grouping every image of a large catalog takes
/// seconds: the screens that ask at the same time wait for one grouping, and ask again only when
/// the prints changed or the groups are [`KEEP_GROUPS`] old.
#[derive(Default)]
pub struct SimilarCache {
    /// Changed when prints are added or images deleted.
    generation: AtomicU64,
    images: Mutex<Option<Arc<ImageGroups>>>,
}

impl SimilarCache {
    /// The groups kept are out of date (new prints, files deleted, disks removed).
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

/// A printed image or video, whatever its print.
struct Item {
    entry: EntryId,
    volume: VolumeId,
    width: Option<u32>,
    height: Option<u32>,
    size: u64,
    duration_ms: Option<u64>,
}

impl AppState {
    /// Mount of each connected volume.
    fn mounts(&self) -> HashMap<VolumeId, Option<String>> {
        self.online()
            .iter()
            .map(|(id, d)| {
                (
                    *id,
                    access_path(d).map(|p| p.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    /// The view of groups of items (positions in `items`); `distance` says how far an item is
    /// from another, for the first of each group.
    fn similar_view(
        &self,
        items: &[Item],
        mut found: Vec<Vec<usize>>,
        volume: Option<VolumeId>,
        distance: &dyn Fn(usize, usize) -> u32,
        pending: u64,
    ) -> Result<SimilarView, ApiError> {
        if let Some(v) = volume {
            found.retain(|g| g.iter().any(|&i| items[i].volume == v));
        }
        let started = Instant::now();
        let bytes = |g: &Vec<usize>| g.iter().map(|&i| items[i].size).sum::<u64>();
        found.sort_by(|a, b| b.len().cmp(&a.len()).then(bytes(b).cmp(&bytes(a))));
        let total_groups = found.len() as u64;
        found.truncate(MAX_GROUPS);
        let mounts = self.mounts();
        let volumes = self.store.read(store::volumes)?;
        let by_id: HashMap<VolumeId, _> = volumes.iter().map(|v| (v.id, v)).collect();
        let groups = self.store.read(|c| {
            let mut out = Vec::with_capacity(found.len());
            for g in &found {
                let mut members: Vec<usize> = g.clone();
                // The largest resolution first (most likely the original), then by entry.
                members.sort_by_key(|&i| {
                    let it = &items[i];
                    (
                        std::cmp::Reverse(
                            u64::from(it.width.unwrap_or(0)) * u64::from(it.height.unwrap_or(0)),
                        ),
                        it.entry,
                    )
                });
                let first = members[0];
                let count = members.len() as u64;
                members.truncate(MAX_FILES);
                let mut files = Vec::with_capacity(members.len());
                for &i in &members {
                    let it = &items[i];
                    // A file gone from the catalog since the groups were made is left out.
                    let rel = match store::relative_path(c, it.entry) {
                        Ok(rel) => rel,
                        Err(StoreError::NotFound(_)) => continue,
                        Err(e) => return Err(e),
                    };
                    let path = rel
                        .split('\\')
                        .filter(|s| !s.is_empty())
                        .map(|s| names::decode(s).to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("\\");
                    let (disk, location, mount) = by_id.get(&it.volume).map_or_else(
                        || (format!("#{}", it.volume.0), None, String::new()),
                        |v| {
                            similar_views::volume_labels(
                                v,
                                mounts.get(&it.volume).cloned().flatten(),
                            )
                        },
                    );
                    let full_path = if mount.is_empty() {
                        path.clone()
                    } else {
                        format!("{}\\{}", mount.trim_end_matches('\\'), path)
                    };
                    files.push(SimilarFileView {
                        entry_id: it.entry.0,
                        volume_id: it.volume.0,
                        disk,
                        location,
                        path,
                        full_path,
                        width: it.width,
                        height: it.height,
                        size: it.size,
                        duration_ms: it.duration_ms,
                        online: mounts.contains_key(&it.volume),
                        distance: if i == first { 0 } else { distance(first, i) },
                    });
                }
                if files.len() < 2 {
                    continue;
                }
                out.push(SimilarGroupView {
                    key: files
                        .first()
                        .map(|f| f.entry_id.to_string())
                        .unwrap_or_default(),
                    files,
                    count,
                });
            }
            Ok(out)
        })?;
        if started.elapsed() >= Duration::from_secs(1) {
            tracing::info!(
                groups = groups.len(),
                ms = started.elapsed().as_millis(),
                "similar groups described"
            );
        }
        Ok(SimilarView {
            groups,
            total_groups,
            pending,
        })
    }

    /// The groups of similar images within `max` bits, made again when the prints changed or
    /// the groups kept are too old; one grouping at a time, the others wait for it.
    fn image_groups(&self, max: u32) -> Result<Arc<ImageGroups>, ApiError> {
        let mut kept = self
            .similar
            .images
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let generation = self.similar.generation.load(Ordering::Relaxed);
        if let Some(g) = kept
            .as_ref()
            .filter(|g| g.generation == generation && g.max == max && g.at.elapsed() < KEEP_GROUPS)
        {
            return Ok(Arc::clone(g));
        }
        let started = Instant::now();
        let prints: Vec<store::ImagePrint> = self
            .store
            .read(store::valid_prints)?
            .into_iter()
            .filter(|p| p.dhash != BLANK_PRINT)
            .collect();
        let hashes: Vec<u64> = prints.iter().map(|p| p.dhash).collect();
        let found = bktree::groups(&hashes, max);
        // Only the images in a group are kept.
        let mut items = Vec::new();
        let mut kept_hashes = Vec::new();
        let mut groups = Vec::with_capacity(found.len());
        for g in found {
            let mut positions = Vec::with_capacity(g.len());
            for i in g {
                let p = &prints[i];
                positions.push(items.len());
                items.push(Item {
                    entry: p.entry,
                    volume: p.volume,
                    width: p.width,
                    height: p.height,
                    size: p.size,
                    duration_ms: None,
                });
                kept_hashes.push(p.dhash);
            }
            groups.push(positions);
        }
        tracing::info!(
            max,
            prints = prints.len(),
            groups = groups.len(),
            largest = groups.iter().map(Vec::len).max().unwrap_or(0),
            ms = started.elapsed().as_millis(),
            "similar images grouped"
        );
        let made = Arc::new(ImageGroups {
            generation,
            max,
            at: Instant::now(),
            items,
            hashes: kept_hashes,
            groups,
        });
        *kept = Some(Arc::clone(&made));
        Ok(made)
    }

    /// How many groups of similar images there are within `max_distance` bits, on every disk:
    /// the figure of the catalog screen, without the files of the groups.
    pub fn similar_image_groups(&self, max_distance: u32) -> Result<u64, ApiError> {
        Ok(self
            .image_groups(max_distance.min(MAX_DISTANCE))?
            .groups
            .len() as u64)
    }

    /// Groups of images within `max_distance` bits of each other, optionally only those with a
    /// copy on `volume` (F11-AC1/AC2/AC3). Images without detail are left out.
    pub fn similar_images(
        &self,
        max_distance: u32,
        volume: Option<VolumeId>,
    ) -> Result<SimilarView, ApiError> {
        let made = self.image_groups(max_distance.min(MAX_DISTANCE))?;
        self.similar_view(
            &made.items,
            made.groups.clone(),
            volume,
            &|a, b| bktree::distance(made.hashes[a], made.hashes[b]),
            self.hashing.pending_images(volume),
        )
    }

    /// Groups of videos with close durations whose frames are within `max_distance` bits on
    /// average (F12-AC1).
    pub fn similar_videos(
        &self,
        max_distance: u32,
        volume: Option<VolumeId>,
    ) -> Result<SimilarView, ApiError> {
        let prints = self.store.read(store::valid_video_prints)?;
        let input: Vec<(u64, &[u64])> = prints
            .iter()
            .map(|p| (p.duration_ms, p.frames.as_slice()))
            .collect();
        let found = video::groups(&input, f64::from(max_distance.min(MAX_DISTANCE)));
        let items: Vec<Item> = prints
            .iter()
            .map(|p| Item {
                entry: p.entry,
                volume: p.volume,
                width: p.width,
                height: p.height,
                size: p.size,
                duration_ms: Some(p.duration_ms),
            })
            .collect();
        self.similar_view(
            &items,
            found,
            volume,
            &|a, b| {
                video::mean_distance(&prints[a].frames, &prints[b].frames)
                    .map_or(64, |d| d.round() as u32)
            },
            self.hashing.pending_videos(volume),
        )
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

    /// A JPEG thumbnail of the middle frame of a connected video (F12), kept in the cache.
    pub fn video_thumbnail(&self, volume: VolumeId, entry: EntryId) -> Result<Vec<u8>, ApiError> {
        if let Some(t) = self.thumbnails.get(entry) {
            return Ok(t);
        }
        let path = self.entry_path(volume, entry)?;
        let (w, h, bgrx) = prism_win::video::frame_bgrx(&path, 0.5)
            .map_err(|e| ApiError::new("thumbnail_failed", e))?;
        let jpeg = media::thumbnail_bgrx(w, h, &bgrx, THUMB_SIDE)
            .map_err(|e| ApiError::new("thumbnail_failed", e))?;
        self.thumbnails.put(entry, jpeg.clone());
        Ok(jpeg)
    }
}
