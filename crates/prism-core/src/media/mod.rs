//! Similar images (F11): a 64-bit perceptual print of each image, computed in the background
//! like the content hashes and valid for one size and modification time of the file, and the
//! groups of images whose prints are within a Hamming distance.
//!
//! The print is a *difference hash*: the image is reduced to 9×8 grey levels (averaging areas,
//! so resized and recompressed copies give almost the same values) and each bit says whether a
//! pixel is brighter than its right neighbour. Camera RAW files are printed from the JPEG
//! preview they contain (`raw`). The groups use a BK-tree (`bktree`, F11-AC2).

pub mod bktree;
pub mod raw;

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use image::{DynamicImage, ImageFormat, ImageReader};

use crate::cancel::CancelToken;
use crate::jobs::JobProgress;
use crate::model::{EntryId, VolumeId, unix_millis_now};
use crate::names;
use crate::scan::disk_meta;
use crate::space;
use crate::store::{self, PrintRecord, Store, StoreError};

/// Extensions decoded directly (F11-AC4).
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "webp", "bmp", "dib", "tif", "tiff",
];
/// Camera RAW formats, printed from their embedded preview (F11-AC4).
pub const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "dng", "orf", "rw2", "raf", "pef",
    "srw", "3fr", "iiq", "rwl", "erf", "kdc", "mrw", "x3f",
];
/// Larger files are not decoded (a print of a 2 GB TIFF is not worth the memory).
pub const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;

/// Whether a stored file name is an image Prism prints.
#[must_use]
pub fn is_printable(stored_name: &str) -> bool {
    let ext = space::extension(stored_name);
    IMAGE_EXTENSIONS.contains(&ext.as_str()) || RAW_EXTENSIONS.contains(&ext.as_str())
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("cannot decode the image: {0}")]
    Decode(String),
    #[error("no preview in the RAW file")]
    NoPreview,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// The print and the size of an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImagePrint {
    pub dhash: u64,
    pub width: u32,
    pub height: u32,
}

/// The difference hash of an image (see the module documentation).
#[must_use]
pub fn dhash(img: &DynamicImage) -> u64 {
    let small = img.thumbnail_exact(9, 8).to_luma8();
    let mut bits = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            bits <<= 1;
            if small.get_pixel(x, y)[0] < small.get_pixel(x + 1, y)[0] {
                bits |= 1;
            }
        }
    }
    bits
}

/// The difference hash of a grey frame (one byte per pixel, top row first), as the video
/// frames are decoded (F12).
#[must_use]
pub fn dhash_luma(width: u32, height: u32, luma: &[u8]) -> Option<u64> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    if width == 0 || height == 0 || luma.len() < pixels {
        return None;
    }
    let img = image::GrayImage::from_raw(width, height, luma[..pixels].to_vec())?;
    Some(dhash(&DynamicImage::ImageLuma8(img)))
}

fn decode(data: &[u8], format: Option<ImageFormat>) -> Result<DynamicImage, MediaError> {
    let mut reader = ImageReader::new(Cursor::new(data));
    match format {
        Some(f) => reader.set_format(f),
        None => {
            reader = reader
                .with_guessed_format()
                .map_err(|e| MediaError::Decode(e.to_string()))?;
        }
    }
    reader
        .decode()
        .map_err(|e| MediaError::Decode(e.to_string()))
}

/// Prints the image at `path` (by its content for ordinary images, by the embedded preview for
/// RAW files).
///
/// # Errors
/// [`MediaError`] when the file cannot be read or decoded.
pub fn print_image(path: &Path) -> Result<ImagePrint, MediaError> {
    print_bytes(&std::fs::read(path)?, path)
}

/// Prints the content of the image at `path`, already read (the analysis prints the images it
/// reads whole for their hashes from the same bytes, REVIEW L31). `path` gives the format.
///
/// # Errors
/// [`MediaError`] when the content cannot be decoded.
pub fn print_bytes(data: &[u8], path: &Path) -> Result<ImagePrint, MediaError> {
    let (img, (width, height)) = load_bytes(data, path)?;
    Ok(ImagePrint {
        dhash: dhash(&img),
        width,
        height,
    })
}

/// The decoded image (the preview for a RAW file) and the size to show for it.
fn load(path: &Path) -> Result<(DynamicImage, (u32, u32)), MediaError> {
    load_bytes(&std::fs::read(path)?, path)
}

/// As [`load`], from the content of the file at `path`.
fn load_bytes(data: &[u8], path: &Path) -> Result<(DynamicImage, (u32, u32)), MediaError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if RAW_EXTENSIONS.contains(&ext.as_str()) {
        let preview = raw::preview(data).ok_or(MediaError::NoPreview)?;
        let img = decode(preview.data, Some(ImageFormat::Jpeg))?;
        let size = raw::largest_size(data).unwrap_or((img.width(), img.height()));
        return Ok((img, size));
    }
    let img = decode(data, None)?;
    let size = (img.width(), img.height());
    Ok((img, size))
}

/// A JPEG thumbnail of the image that fits in `max` × `max` pixels, for the side-by-side
/// previews of the similar images (F11-AC3).
///
/// # Errors
/// [`MediaError`] when the file cannot be read or decoded.
pub fn thumbnail_jpeg(path: &Path, max: u32) -> Result<Vec<u8>, MediaError> {
    let (img, _) = load(path)?;
    let small = img.thumbnail(max, max).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .encode_image(&small)
        .map_err(|e| MediaError::Decode(e.to_string()))?;
    Ok(out)
}

/// A JPEG thumbnail of a decoded 32-bit frame (B, G, R, unused), for the similar videos.
///
/// # Errors
/// [`MediaError::Decode`] when the frame is malformed or cannot be encoded.
pub fn thumbnail_bgrx(
    width: u32,
    height: u32,
    bgrx: &[u8],
    max: u32,
) -> Result<Vec<u8>, MediaError> {
    let pixels = width as usize * height as usize;
    if width == 0 || height == 0 || bgrx.len() < pixels * 4 {
        return Err(MediaError::Decode("empty frame".to_owned()));
    }
    let rgb: Vec<u8> = bgrx
        .chunks_exact(4)
        .take(pixels)
        .flat_map(|p| [p[2], p[1], p[0]])
        .collect();
    let img = image::RgbImage::from_raw(width, height, rgb)
        .ok_or_else(|| MediaError::Decode("bad frame size".to_owned()))?;
    let small = DynamicImage::ImageRgb8(img).thumbnail(max, max).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .encode_image(&small)
        .map_err(|e| MediaError::Decode(e.to_string()))?;
    Ok(out)
}

/// What a printing run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrintReport {
    pub printed: u64,
    /// Not decodable (kept as such until the file changes).
    pub unreadable: u64,
    /// Changed or gone since the catalog was updated: tried again after the next update.
    pub changed: u64,
    pub interrupted: bool,
}

/// Whether a file of the catalog is an image Prism prints: printable and not too large.
fn wants_print(s: &store::PrintState) -> bool {
    is_image_to_print(&s.name, s.size)
}

/// Whether a file (stored name and size) is an image Prism prints: printable and not too large.
#[must_use]
pub fn is_image_to_print(stored_name: &str, size: u64) -> bool {
    is_printable(stored_name) && size <= MAX_IMAGE_BYTES
}

/// Images of a volume Prism prints, with and without a valid print (decoded or not). Images
/// too large to decode are not counted: counted as left, they made a round every 2 minutes on
/// a share of virtual slides, each doing nothing.
///
/// # Errors
/// SQLite errors.
pub fn print_progress(
    conn: &rusqlite::Connection,
    volume: VolumeId,
) -> Result<(u64, u64), StoreError> {
    store::count_prints(conn, volume, "image_prints", &is_image_to_print)
}

/// Prints the images of a volume reached at `root` that have no valid print, with the worker
/// threads (REVIEW D60), saving them in batches; `pause` is asked between small groups of files.
/// Progress is counted in files.
///
/// # Errors
/// [`MediaError::Store`] if the catalog cannot be read or written.
pub fn print_volume(
    store: &Store,
    volume: VolumeId,
    root: &Path,
    workers: &Workers<'_>,
    cancel: &CancelToken,
    pause: &dyn Fn() -> bool,
    progress: &JobProgress,
) -> Result<PrintReport, MediaError> {
    let todo: Vec<store::PrintState> = store
        .read(|c| store::print_states(c, volume))?
        .into_iter()
        .filter(|s| !s.printed && wants_print(s))
        .collect();
    let compute = |path: &Path| print_image(path).map_err(|e| e.to_string());
    let one = |s: &store::PrintState, path: &Path| print_one(s, path, cancel, &compute);
    run_pass(
        store,
        root,
        &todo,
        "prints",
        cancel,
        pause,
        progress,
        workers.count.max(1) * 4,
        &|items| in_parallel(items, workers, &one),
        &|batch: Vec<(store::PrintState, Option<ImagePrint>)>| {
            let records: Vec<PrintRecord> = batch
                .into_iter()
                .map(|(s, p)| PrintRecord {
                    entry: s.entry,
                    size: s.size,
                    modified: s.modified,
                    dhash: p.map(|p| p.dhash),
                    width: p.map(|p| p.width),
                    height: p.map(|p| p.height),
                })
                .collect();
            store.write(move |w| store::set_prints(w, &records, unix_millis_now()))
        },
    )
}

/// The path of a catalog file through `root`, with a cache of the folder paths.
fn file_path(
    store: &Store,
    root: &Path,
    folders: &mut HashMap<EntryId, PathBuf>,
    s: &store::PrintState,
) -> Result<Option<PathBuf>, StoreError> {
    let Some(parent) = s.parent else {
        return Ok(None);
    };
    let folder = match folders.get(&parent) {
        Some(p) => p.clone(),
        None => {
            let mut p = root.to_path_buf();
            for part in store
                .read(|c| store::relative_path(c, parent))?
                .split('\\')
                .filter(|p| !p.is_empty())
            {
                p.push(names::decode(part));
            }
            folders.insert(parent, p.clone());
            p
        }
    };
    Ok(Some(folder.join(names::decode(&s.name))))
}

/// Files with their print, or `None` when they could not be decoded.
type Batch<T> = Vec<(store::PrintState, Option<T>)>;

/// The threads that print the images and how each one starts (REVIEW D60).
pub struct Workers<'a> {
    pub count: usize,
    /// Run first on every worker thread: the app puts it in Windows background mode.
    pub init: &'a (dyn Fn() + Sync),
}

fn nothing() {}

impl Workers<'static> {
    /// Everything on the calling thread.
    #[must_use]
    pub fn one() -> Self {
        Self {
            count: 1,
            init: &nothing,
        }
    }
}

/// The threads the image prints use on this machine: a quarter of the cores, from 1 to 4.
/// Decoding is limited by the processor and scales with the threads (×3.8 with 4 on 300 photos,
/// docs/benchmarks/2026-11-08-prints-threads.md); a quarter keeps the PC responsive.
#[must_use]
pub fn print_workers() -> usize {
    std::thread::available_parallelism().map_or(1, |n| (n.get() / 4).clamp(1, 4))
}

/// What became of one file of a printing pass.
enum Outcome<T> {
    Printed(T),
    Unreadable(String),
    /// Changed on the disk since it was indexed: left for after the next update.
    Changed,
    /// Not started: the pass was cancelled.
    Stopped,
}

/// Prints one file if it has the size and date of the catalog before and after being read.
fn print_one<T>(
    s: &store::PrintState,
    path: &Path,
    cancel: &CancelToken,
    compute: &dyn Fn(&Path) -> Result<T, String>,
) -> Outcome<T> {
    if cancel.is_cancelled() {
        return Outcome::Stopped;
    }
    let unchanged = |p: &Path| {
        disk_meta(p)
            .is_ok_and(|(is_dir, m)| !is_dir && m.size == s.size && m.modified == s.modified)
    };
    if !unchanged(path) {
        return Outcome::Changed;
    }
    let result = compute(path);
    if !unchanged(path) {
        return Outcome::Changed;
    }
    match result {
        Ok(v) => Outcome::Printed(v),
        Err(e) => Outcome::Unreadable(e),
    }
}

/// Prints `items` with the worker threads, the results in the order of `items`.
fn in_parallel<T: Send>(
    items: &[(&store::PrintState, PathBuf)],
    workers: &Workers<'_>,
    one: &(dyn Fn(&store::PrintState, &Path) -> Outcome<T> + Sync),
) -> Vec<Outcome<T>> {
    if workers.count <= 1 || items.len() <= 1 {
        return items.iter().map(|(s, p)| one(s, p)).collect();
    }
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Outcome<T>>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers.count.min(items.len()) {
            scope.spawn(|| {
                (workers.init)();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((s, p)) = items.get(i) else {
                        break;
                    };
                    let outcome = one(s, p);
                    *slots[i].lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|m| {
            m.into_inner()
                .unwrap_or_else(PoisonError::into_inner)
                .unwrap_or(Outcome::Stopped)
        })
        .collect()
}

/// Prints a group of files (in parallel or not), the results in their order.
type Process<'a, T> = &'a dyn Fn(&[(&store::PrintState, PathBuf)]) -> Vec<Outcome<T>>;

/// One printing pass over `todo` (images or videos), `chunk` files at a time: each file is
/// printed only if it still has the size and date of the catalog before and after being read
/// (otherwise it is left for after the next update); results are saved in batches through
/// `save`, an undecodable file with `None`. `pause` is asked between chunks.
#[allow(clippy::too_many_arguments)]
fn run_pass<T>(
    store: &Store,
    root: &Path,
    todo: &[store::PrintState],
    phase: &str,
    cancel: &CancelToken,
    pause: &dyn Fn() -> bool,
    progress: &JobProgress,
    chunk: usize,
    process: Process<'_, T>,
    save: &dyn Fn(Batch<T>) -> Result<(), StoreError>,
) -> Result<PrintReport, MediaError> {
    progress.set_phase(phase);
    progress.total.store(todo.len() as u64, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    let mut report = PrintReport::default();
    let mut folders: HashMap<EntryId, PathBuf> = HashMap::new();
    let mut pending: Batch<T> = Vec::new();
    for slice in todo.chunks(chunk.max(1)) {
        if cancel.is_cancelled() || pause() {
            report.interrupted = true;
            break;
        }
        let mut items = Vec::with_capacity(slice.len());
        for s in slice {
            if let Some(path) = file_path(store, root, &mut folders, s)? {
                items.push((s, path));
            }
        }
        for ((s, path), outcome) in items.iter().zip(process(&items)) {
            match outcome {
                Outcome::Printed(v) => {
                    report.printed += 1;
                    pending.push(((*s).clone(), Some(v)));
                }
                Outcome::Unreadable(e) => {
                    tracing::debug!(path = %path.display(), error = %e, "not printed");
                    report.unreadable += 1;
                    pending.push(((*s).clone(), None));
                }
                Outcome::Changed => report.changed += 1,
                Outcome::Stopped => {
                    report.interrupted = true;
                    continue;
                }
            }
            progress.done.fetch_add(1, Ordering::Relaxed);
        }
        if pending.len() >= 100 {
            save(std::mem::take(&mut pending))?;
        }
        if report.interrupted {
            break;
        }
    }
    if !pending.is_empty() {
        save(pending)?;
    }
    Ok(report)
}

/// Videos (F12): extensions, prints and groups.
pub mod video {
    use std::path::Path;

    use super::{MediaError, PrintReport, print_one, run_pass};
    use crate::cancel::CancelToken;
    use crate::jobs::JobProgress;
    use crate::model::{VolumeId, unix_millis_now};
    use crate::space;
    use crate::store::{self, Store, StoreError, VideoPrintRecord};

    /// Containers Prism tries to decode; what Windows cannot decode is marked as such
    /// (ADR-0013). `.ts` is left out: on a developer's disk it is mostly TypeScript.
    pub const VIDEO_EXTENSIONS: &[&str] = &[
        "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "asf", "mpg", "mpeg", "m2ts", "mts",
        "3gp", "3g2",
    ];
    /// The points of the timeline whose frames make the print.
    pub const POINTS: [f64; 5] = [0.1, 0.3, 0.5, 0.7, 0.9];

    /// The print of a video: its duration, size and one print per sampled frame.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct VideoPrint {
        pub duration_ms: u64,
        pub width: u32,
        pub height: u32,
        pub frames: Vec<u64>,
    }

    /// Decodes the frames of a video at [`POINTS`] and prints them (Media Foundation in the app,
    /// a fake in the tests).
    pub trait VideoDecoder {
        /// # Errors
        /// A message when the video cannot be decoded.
        fn print(&self, path: &Path) -> Result<VideoPrint, String>;
    }

    #[must_use]
    pub fn is_video(stored_name: &str) -> bool {
        VIDEO_EXTENSIONS.contains(&space::extension(stored_name).as_str())
    }

    /// Videos of a volume with and without a valid print (decoded or not).
    ///
    /// # Errors
    /// SQLite errors.
    pub fn video_progress(
        conn: &rusqlite::Connection,
        volume: VolumeId,
    ) -> Result<(u64, u64), StoreError> {
        store::count_prints(conn, volume, "video_prints", &|name, _| is_video(name))
    }

    /// Prints the videos of a volume reached at `root` that have no valid print.
    ///
    /// # Errors
    /// [`MediaError::Store`] if the catalog cannot be read or written.
    pub fn print_videos(
        store: &Store,
        volume: VolumeId,
        root: &Path,
        decoder: &dyn VideoDecoder,
        cancel: &CancelToken,
        pause: &dyn Fn() -> bool,
        progress: &JobProgress,
    ) -> Result<PrintReport, MediaError> {
        let todo: Vec<store::PrintState> = store
            .read(|c| store::video_print_states(c, volume))?
            .into_iter()
            .filter(|s| !s.printed && is_video(&s.name))
            .collect();
        // One video at a time, on this thread: the decoder is Windows Media Foundation.
        let compute = |path: &Path| decoder.print(path);
        run_pass(
            store,
            root,
            &todo,
            "videoPrints",
            cancel,
            pause,
            progress,
            1,
            &|items| {
                items
                    .iter()
                    .map(|(s, p)| print_one(s, p, cancel, &compute))
                    .collect()
            },
            &|batch: Vec<(store::PrintState, Option<VideoPrint>)>| {
                let records: Vec<VideoPrintRecord> = batch
                    .into_iter()
                    .map(|(s, p)| VideoPrintRecord {
                        entry: s.entry,
                        size: s.size,
                        modified: s.modified,
                        duration_ms: p.as_ref().map(|p| p.duration_ms),
                        width: p.as_ref().map(|p| p.width),
                        height: p.as_ref().map(|p| p.height),
                        frames: p.map(|p| p.frames),
                    })
                    .collect();
                store.write(move |w| store::set_video_prints(w, &records, unix_millis_now()))
            },
        )
    }

    /// Mean Hamming distance of the frame prints, when both have the same number of frames.
    #[must_use]
    pub fn mean_distance(a: &[u64], b: &[u64]) -> Option<f64> {
        if a.is_empty() || a.len() != b.len() {
            return None;
        }
        let total: u32 = a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum();
        Some(f64::from(total) / a.len() as f64)
    }

    /// Whether two durations are close enough for the same content: 2 s or 3 %.
    #[must_use]
    pub fn close_durations(a: u64, b: u64) -> bool {
        a.abs_diff(b) <= (a.max(b) * 3 / 100).max(2_000)
    }

    /// Groups of videos with close durations and frame prints within `max_mean` bits on average
    /// (F12-AC1), joined transitively; positions in `videos` (duration, frame prints).
    #[must_use]
    pub fn groups(videos: &[(u64, &[u64])], max_mean: f64) -> Vec<Vec<usize>> {
        let mut order: Vec<usize> = (0..videos.len()).collect();
        order.sort_by_key(|&i| videos[i].0);
        let mut parent: Vec<usize> = (0..videos.len()).collect();
        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        for (k, &i) in order.iter().enumerate() {
            for &j in &order[k + 1..] {
                if !close_durations(videos[i].0, videos[j].0) {
                    break;
                }
                if mean_distance(videos[i].1, videos[j].1).is_some_and(|d| d <= max_mean) {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    if a != b {
                        parent[a.max(b)] = a.min(b);
                    }
                }
            }
        }
        let mut by_root: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();
        for i in 0..videos.len() {
            let r = find(&mut parent, i);
            by_root.entry(r).or_default().push(i);
        }
        let mut out: Vec<Vec<usize>> = by_root.into_values().filter(|g| g.len() > 1).collect();
        out.sort_by_key(|g| g[0]);
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn durations_and_groups() {
            assert!(close_durations(60_000, 61_500));
            assert!(close_durations(600_000, 617_000));
            assert!(!close_durations(60_000, 63_000));
            let a = [1u64, 2, 3, 4, 5];
            let near = [1u64, 3, 3, 4, 5]; // 1 bit in one frame
            let far = [!1u64, !2, !3, !4, !5];
            let videos = [
                (100_000, &a[..]),
                (100_900, &near[..]),
                (100_000, &far[..]),
                (300_000, &a[..]), // same frames, very different length
            ];
            assert_eq!(groups(&videos, 1.0), [vec![0, 1]]);
            assert_eq!(mean_distance(&a, &near), Some(0.2));
            assert_eq!(mean_distance(&a, &a[..3]), None);
        }
    }
}
