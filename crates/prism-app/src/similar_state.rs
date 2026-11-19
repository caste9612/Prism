//! Similar images (F11) and videos (F12) in the app: groups from the prints in the catalog,
//! thumbnails of the connected files on demand.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use prism_core::duplicates;
use prism_core::media::{self, bktree, video};
use prism_core::model::{EntryId, VolumeId, VolumeKind};
use prism_core::names;
use prism_core::store::{self, StoreError};

use crate::dto::ApiError;
use crate::duplicates_views::DupFilterInput;
use crate::similar_views::{
    self, MAX_DISTANCE, MAX_FILES, MAX_GROUPS, SimilarFileView, SimilarGroupView, SimilarView,
    THUMB_SIDE,
};
use crate::state::{AppState, access_path};

/// Groups found at one distance are up to date this long, unless disks are removed before: new
/// prints, and the files the catalog updates meanwhile (moved, changed, deleted outside Prism),
/// show in them after. Not made again at every new print: on the company PC the analysis of the
/// system disk prints new images at every round (REVIEW L36).
const KEEP_GROUPS: Duration = Duration::from_secs(30 * 60);

/// The space the copies of duplicates take is up to date this long, unless files are deleted
/// from Prism before: the analysis finds new copies meanwhile (REVIEW L37).
const KEEP_TOTAL: Duration = Duration::from_secs(10 * 60);

/// The figures out of date are made again in the background while a screen asked for them
/// within this long (REVIEW L38).
const KEEP_UP_WHILE_ASKED: Duration = Duration::from_secs(2 * 3600);

/// The figures the catalog screen asks for (`CatalogView.svelte`): made right after the start.
const CATALOG_DISTANCE: u32 = 6;
const CATALOG_MIN_COPY: u64 = 1_000_000;

/// A figure taking longer than this to make goes to the log.
const SLOW_FIGURE: Duration = Duration::from_secs(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

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
    /// Positions in `items`, the centre first.
    groups: Vec<Vec<usize>>,
}

impl ImageGroups {
    /// The same groups without the images `gone`; the groups left with one image go. A group
    /// whose centre went is measured from its next image.
    fn without(&self, gone: &HashSet<EntryId>) -> Self {
        let mut items = Vec::with_capacity(self.items.len());
        let mut hashes = Vec::with_capacity(self.items.len());
        let mut groups = Vec::with_capacity(self.groups.len());
        for g in &self.groups {
            let left: Vec<usize> = g
                .iter()
                .copied()
                .filter(|&i| !gone.contains(&self.items[i].entry))
                .collect();
            if left.len() < 2 {
                continue;
            }
            let mut positions = Vec::with_capacity(left.len());
            for i in left {
                positions.push(items.len());
                items.push(self.items[i].clone());
                hashes.push(self.hashes[i]);
            }
            groups.push(positions);
        }
        Self {
            generation: self.generation,
            max: self.max,
            at: self.at,
            items,
            hashes,
            groups,
        }
    }
}

/// The space the copies of duplicates take.
struct DuplicateSpace {
    generation: u64,
    at: Instant,
    min_size: u64,
    space: u64,
    /// Files were deleted since.
    outdated: bool,
}

/// Figures of the whole catalog kept between screens: the last groups of similar images
/// (REVIEW L34) and the space the copies of duplicates take (REVIEW L37). Each takes seconds on
/// a large catalog (on the company PC 21 s and 9 s), so the screens never wait for them but the
/// first time (REVIEW L38): a figure out of date — [`KEEP_GROUPS`] or [`KEEP_TOTAL`] old,
/// counted before Prism deleted files or forgot disks — is given at once, and the figures
/// thread ([`AppState::keep_figures`]) makes it again; it makes those of the catalog screen
/// right after the start. One grouping and one count at a time: who asks meanwhile waits and
/// takes the result. Files deleted from Prism leave the groups at once.
#[derive(Default)]
pub struct KeptResults {
    /// Changed when disks are removed or the catalog emptied.
    generation: AtomicU64,
    images: Mutex<Option<Arc<ImageGroups>>>,
    grouping: Mutex<()>,
    space: Mutex<Option<DuplicateSpace>>,
    counting: Mutex<()>,
    /// When a screen last asked for a figure.
    asked: Mutex<Option<Instant>>,
    /// Set, with the thread woken, when a figure is out of date.
    stale: Mutex<bool>,
    wake: Condvar,
}

impl KeptResults {
    /// Disks removed or the catalog emptied: every figure is out of date.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.wake();
    }

    /// Files deleted from Prism: they leave the groups of similar images at once (grouping
    /// again would take seconds), and the space of the duplicates is counted again.
    pub fn forget(&self, gone: &HashSet<EntryId>) {
        if gone.is_empty() {
            return;
        }
        {
            let mut images = lock(&self.images);
            if let Some(g) = images.as_ref() {
                *images = Some(Arc::new(g.without(gone)));
            }
        }
        if let Some(s) = lock(&self.space).as_mut() {
            s.outdated = true;
        }
        self.wake();
    }

    fn wake(&self) {
        *lock(&self.stale) = true;
        self.wake.notify_all();
    }

    /// Waits until a figure is out of date, or `timeout`.
    pub fn wait(&self, timeout: Duration) {
        let stale = lock(&self.stale);
        let (mut stale, _) = self
            .wake
            .wait_timeout_while(stale, timeout, |s| !*s)
            .unwrap_or_else(PoisonError::into_inner);
        *stale = false;
    }

    fn asked_now(&self) {
        *lock(&self.asked) = Some(Instant::now());
    }

    fn asked_lately(&self) -> bool {
        lock(&self.asked).is_some_and(|t| t.elapsed() < KEEP_UP_WHILE_ASKED)
    }

    /// The groups kept at distance `max`: any, or only up to date ones.
    fn images_at(&self, max: u32, up_to_date: bool) -> Option<Arc<ImageGroups>> {
        let generation = self.generation.load(Ordering::Relaxed);
        lock(&self.images)
            .as_ref()
            .filter(|g| {
                g.max == max
                    && (!up_to_date || (g.generation == generation && g.at.elapsed() < KEEP_GROUPS))
            })
            .cloned()
    }

    /// The space kept for copies of at least `min_size`: any, or only up to date.
    fn space_of(&self, min_size: u64, up_to_date: bool) -> Option<u64> {
        let generation = self.generation.load(Ordering::Relaxed);
        lock(&self.space)
            .as_ref()
            .filter(|s| {
                s.min_size == min_size
                    && (!up_to_date
                        || (!s.outdated
                            && s.generation == generation
                            && s.at.elapsed() < KEEP_TOTAL))
            })
            .map(|s| s.space)
    }
}

/// A printed image or video, whatever its print.
#[derive(Clone)]
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
    /// from another. With `centred`, the first of each group is its centre and the distances
    /// are from it (images, REVIEW L37); otherwise from the largest of the group.
    fn similar_view(
        &self,
        items: &[Item],
        mut found: Vec<Vec<usize>>,
        volume: Option<VolumeId>,
        distance: &dyn Fn(usize, usize) -> u32,
        centred: bool,
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
                let first = if centred { g[0] } else { members[0] };
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
                        permanent: by_id
                            .get(&it.volume)
                            .is_none_or(|v| v.info.kind != VolumeKind::Fixed),
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

    /// The groups of similar images within `max` bits for a screen: those kept, even out of date
    /// (the figures thread makes them again), or made now.
    fn image_groups(&self, max: u32) -> Result<Arc<ImageGroups>, ApiError> {
        self.kept.asked_now();
        if let Some(g) = self.kept.images_at(max, false) {
            if self.kept.images_at(max, true).is_none() {
                self.kept.wake();
            }
            return Ok(g);
        }
        self.group_images(max, false)
    }

    /// Groups the images within `max` bits, unless groups (up to date ones with `up_to_date`)
    /// are kept: one grouping at a time, who asks meanwhile waits and takes its groups.
    fn group_images(&self, max: u32, up_to_date: bool) -> Result<Arc<ImageGroups>, ApiError> {
        let _one = lock(&self.kept.grouping);
        if let Some(g) = self.kept.images_at(max, up_to_date) {
            return Ok(g);
        }
        let generation = self.kept.generation.load(Ordering::Relaxed);
        let started = Instant::now();
        let prints: Vec<store::ImagePrint> = self
            .store
            .read(store::valid_prints)?
            .into_iter()
            .filter(|p| p.dhash != BLANK_PRINT)
            .collect();
        let read_ms = started.elapsed().as_millis();
        let hashes: Vec<u64> = prints.iter().map(|p| p.dhash).collect();
        let (found, stats) = bktree::groups_with_stats(&hashes, max);
        // Only the images in a group are kept.
        let mut items = Vec::new();
        let mut kept_hashes = Vec::new();
        let mut groups = Vec::with_capacity(found.len());
        for g in found {
            let mut positions = Vec::with_capacity(g.items.len());
            // The centre first.
            let others = g.items.iter().copied().filter(|&i| i != g.centre);
            for i in std::iter::once(g.centre).chain(others) {
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
            in_groups = groups.iter().map(Vec::len).sum::<usize>(),
            distinct = stats.distinct,
            linked = stats.linked,
            read_ms,
            linked_ms = stats.linked_ms,
            grouping_ms = stats.grouping_ms,
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
        *lock(&self.kept.images) = Some(Arc::clone(&made));
        Ok(made)
    }

    /// The space the copies of duplicates of at least `min_size` take on every disk: the figure
    /// of the catalog screen, kept (REVIEW L37, L38). Counting it reads every full hash of the
    /// catalog: 9 to 17 s on the company PC.
    pub fn duplicate_space(&self, min_size: u64) -> Result<u64, ApiError> {
        self.kept.asked_now();
        if let Some(space) = self.kept.space_of(min_size, false) {
            if self.kept.space_of(min_size, true).is_none() {
                self.kept.wake();
            }
            return Ok(space);
        }
        self.count_duplicate_space(min_size, false)
    }

    /// Counts the space of the duplicates, unless it is kept (up to date with `up_to_date`).
    fn count_duplicate_space(&self, min_size: u64, up_to_date: bool) -> Result<u64, ApiError> {
        let _one = lock(&self.kept.counting);
        if let Some(space) = self.kept.space_of(min_size, up_to_date) {
            return Ok(space);
        }
        let generation = self.kept.generation.load(Ordering::Relaxed);
        let started = Instant::now();
        let filter = DupFilterInput {
            min_size,
            ..DupFilterInput::default()
        }
        .to_filter();
        let space = self
            .store
            .read(|c| duplicates::find(c, &HashSet::new(), &filter, 0))?
            .total_recoverable;
        *lock(&self.kept.space) = Some(DuplicateSpace {
            generation,
            at: Instant::now(),
            min_size,
            space,
            outdated: false,
        });
        if started.elapsed() >= SLOW_FIGURE {
            tracing::info!(
                min_size,
                space,
                ms = started.elapsed().as_millis(),
                "duplicate space counted"
            );
        }
        Ok(space)
    }

    /// The figures thread (REVIEW L38): at the start, the figures of the catalog screen; then
    /// those out of date, while a screen asked for them in the last [`KEEP_UP_WHILE_ASKED`].
    pub fn keep_figures(&self, at_start: bool) {
        if !at_start && !self.kept.asked_lately() {
            return;
        }
        let max = lock(&self.kept.images)
            .as_ref()
            .map_or(CATALOG_DISTANCE, |g| g.max);
        if let Err(e) = self.group_images(max, true) {
            tracing::warn!(error = ?e, "similar images not grouped");
        }
        let min_size = lock(&self.kept.space)
            .as_ref()
            .map_or(CATALOG_MIN_COPY, |s| s.min_size);
        if let Err(e) = self.count_duplicate_space(min_size, true) {
            tracing::warn!(error = ?e, "duplicate space not counted");
        }
    }

    /// How many groups of similar images there are within `max_distance` bits, on every disk:
    /// the figure of the catalog screen, without the files of the groups.
    pub fn similar_image_groups(&self, max_distance: u32) -> Result<u64, ApiError> {
        Ok(self
            .image_groups(max_distance.min(MAX_DISTANCE))?
            .groups
            .len() as u64)
    }

    /// Groups of images within `max_distance` bits of the centre of their group, optionally
    /// only those with a copy on `volume` (F11-AC1/AC2/AC3). Images without detail are left
    /// out.
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
            true,
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
            false,
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
