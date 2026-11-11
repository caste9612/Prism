//! Similar images end to end (F11): prints of generated images (resized, recompressed,
//! retouched, unrelated), RAW previews in a TIFF container and in another container, the
//! background pass over a volume and the groups.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Cursor;

use common::{Sandbox, TestCatalog};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use prism_core::cancel::CancelToken;
use prism_core::jobs::JobProgress;
use prism_core::media::bktree::{distance, groups};
use prism_core::media::{MAX_IMAGE_BYTES, Workers, print_image, print_progress, print_volume, raw};
use prism_core::model::{EntryId, EntryMeta};
use prism_core::store::{self, WriteOp};

fn photo(w: u32, h: u32) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let blob = if (fx - 0.3).powi(2) + (fy - 0.4).powi(2) < 0.04 {
            180
        } else {
            0
        };
        Rgb([
            (fx * 200.0) as u8 + 20,
            (fy * 150.0) as u8 + blob / 3,
            ((1.0 - fx) * 120.0) as u8 + blob / 2,
        ])
    })
}

fn other(w: u32, h: u32) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| {
        let v = if (x / 37 + y / 23) % 2 == 0 { 230 } else { 15 };
        Rgb([v, 255 - v, (y % 256) as u8])
    })
}

fn encode(img: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, format).unwrap();
    out.into_inner()
}

fn jpeg(img: &DynamicImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(img)
        .unwrap();
    out
}

/// A minimal little-endian TIFF with the preview listed as JPEGInterchangeFormat, as CR2, NEF,
/// ARW and DNG files do.
fn tiff_raw(preview: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(b"II*\0");
    d.extend_from_slice(&8u32.to_le_bytes());
    let entries: u16 = 2;
    d.extend_from_slice(&entries.to_le_bytes());
    let data_at = 8 + 2 + 12 * u32::from(entries) + 4;
    for (tag, value) in [(0x0201u16, data_at), (0x0202, preview.len() as u32)] {
        d.extend_from_slice(&tag.to_le_bytes());
        d.extend_from_slice(&4u16.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&value.to_le_bytes());
    }
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(preview);
    d.extend_from_slice(&[0u8; 1000]); // raw sensor data would follow
    d
}

#[test]
fn f11_prints_survive_resizing_recompression_and_small_edits() {
    let sb = Sandbox::new();
    let base = DynamicImage::ImageRgb8(photo(640, 480));
    let mut edited = photo(640, 480);
    for x in 600..630 {
        for y in 10..40 {
            edited.put_pixel(x, y, Rgb([255, 255, 255]));
        }
    }
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("base.png", encode(&base, ImageFormat::Png)),
        (
            "small.png",
            encode(
                &base.resize_exact(320, 240, image::imageops::FilterType::Triangle),
                ImageFormat::Png,
            ),
        ),
        ("low.jpg", jpeg(&base, 35)),
        (
            "edited.webp",
            encode(&DynamicImage::ImageRgb8(edited), ImageFormat::WebP),
        ),
        (
            "other.bmp",
            encode(&DynamicImage::ImageRgb8(other(640, 480)), ImageFormat::Bmp),
        ),
        (
            "camera.nef",
            tiff_raw(&jpeg(
                &base.resize_exact(800, 600, image::imageops::FilterType::Triangle),
                85,
            )),
        ),
        // CR3 is an ISO media file: the preview is found by scanning for JPEG streams.
        (
            "camera.cr3",
            [
                b"\0\0\0\x18ftypcrx ".as_slice(),
                &[7u8; 300],
                &jpeg(&base, 80),
            ]
            .concat(),
        ),
    ];
    for (name, data) in &files {
        sb.fs.write(&sb.path(name), data).unwrap();
    }
    let print = |name: &str| print_image(&sb.path(name)).unwrap();
    let b = print("base.png");
    assert_eq!((b.width, b.height), (640, 480));
    // F11-AC1: copies resized, recompressed or slightly retouched stay close.
    for name in [
        "small.png",
        "low.jpg",
        "edited.webp",
        "camera.nef",
        "camera.cr3",
    ] {
        let d = distance(b.dhash, print(name).dhash);
        assert!(d <= 6, "{name}: distance {d}");
    }
    assert!(distance(b.dhash, print("other.bmp").dhash) > 20);
    // F11-AC4, AC3: a RAW reports the size of its largest preview.
    assert_eq!(
        (print("camera.nef").width, print("camera.nef").height),
        (800, 600)
    );
    assert_eq!(print("camera.cr3").width, 640);
    assert!(raw::preview(b"not a raw file at all").is_none());
}

#[test]
fn f11_background_prints_and_groups() {
    let sb = Sandbox::new();
    let base = DynamicImage::ImageRgb8(photo(400, 300));
    sb.fs
        .write(&sb.path("a.png"), &encode(&base, ImageFormat::Png))
        .unwrap();
    sb.fs.write(&sb.path("b.jpg"), &jpeg(&base, 50)).unwrap();
    sb.fs
        .write(
            &sb.path("c.gif"),
            &encode(&DynamicImage::ImageRgb8(other(400, 300)), ImageFormat::Gif),
        )
        .unwrap();
    sb.fs
        .write(&sb.path("broken.jpg"), b"\xFF\xD8\xFF garbage")
        .unwrap();
    sb.fs.write(&sb.path("notes.txt"), b"not an image").unwrap();
    let cat = TestCatalog::new();
    let volume = cat.add_volume(&sb.root().to_string_lossy());
    cat.index(volume, sb.root());

    let progress = JobProgress::default();
    // Three threads (REVIEW D60): every worker starts with `init`.
    let started = std::sync::atomic::AtomicUsize::new(0);
    let init = || {
        started.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    };
    let workers = Workers {
        count: 3,
        init: &init,
    };
    let report = print_volume(
        &cat.store,
        volume,
        sb.root(),
        &workers,
        &CancelToken::new(),
        &|| false,
        &progress,
    )
    .unwrap();
    assert_eq!(
        (report.printed, report.unreadable, report.changed),
        (3, 1, 0)
    );
    assert_eq!(started.load(std::sync::atomic::Ordering::Relaxed), 3);
    assert_eq!(progress.done.load(std::sync::atomic::Ordering::Relaxed), 4);
    assert_eq!(
        cat.store.read(|c| print_progress(c, volume)).unwrap(),
        (4, 4)
    );
    // Nothing left for a second run.
    let again = print_volume(
        &cat.store,
        volume,
        sb.root(),
        &Workers::one(),
        &CancelToken::new(),
        &|| false,
        &progress,
    )
    .unwrap();
    assert_eq!(again.printed + again.unreadable, 0);

    let prints = cat.store.read(store::valid_prints).unwrap();
    assert_eq!(prints.len(), 3, "the broken file has no print");
    let hashes: Vec<u64> = prints.iter().map(|p| p.dhash).collect();
    let found = groups(&hashes, 8);
    assert_eq!(found.len(), 1);
    let names: Vec<String> = found[0]
        .iter()
        .map(|&i| {
            cat.store
                .read(|c| store::entry(c, prints[i].entry))
                .unwrap()
                .unwrap()
                .name
        })
        .collect();
    assert_eq!(names, ["a.png", "b.jpg"]);

    // A changed image loses its print until it is printed again (like the hashes, F06-AC3).
    sb.fs
        .write(
            &sb.path("a.png"),
            &encode(&DynamicImage::ImageRgb8(other(200, 100)), ImageFormat::Png),
        )
        .unwrap();
    sb.fs
        .set_modified(&sb.path("a.png"), common::mtime(999))
        .unwrap();
    cat.index(volume, sb.root());
    assert_eq!(
        cat.store.read(|c| print_progress(c, volume)).unwrap(),
        (4, 3)
    );
    assert_eq!(cat.store.read(store::valid_prints).unwrap().len(), 2);
}

// Images too large to decode are neither printed nor counted as left: counted, they made a
// round every 2 minutes on a share of virtual slides, each doing nothing.
#[test]
fn images_too_large_to_decode_are_not_counted() {
    let cat = TestCatalog::new();
    let volume = cat.add_volume("slides");
    let first = cat.store.allocate_entry_ids(3);
    let insert = |i: i64, name: &str, is_dir: bool, size: u64| WriteOp::Insert {
        id: EntryId(first.0 + i),
        volume,
        parent: (i > 0).then_some(first),
        name: name.into(),
        is_dir,
        meta: EntryMeta {
            size,
            ..EntryMeta::default()
        },
    };
    let session = cat.store.session();
    session
        .submit(
            volume,
            vec![
                insert(0, "", true, 0),
                insert(1, "slide.tif", false, MAX_IMAGE_BYTES + 1),
                insert(2, "photo.jpg", false, 1000),
            ],
        )
        .unwrap();
    session.flush().unwrap();
    assert_eq!(
        cat.store.read(|c| print_progress(c, volume)).unwrap(),
        (1, 0),
        "only the photo is left"
    );
}

// F11-AC3: the side-by-side previews are small JPEG thumbnails, also of RAW files.
#[test]
fn f11_thumbnails_fit_the_requested_size() {
    let sb = Sandbox::new();
    let base = DynamicImage::ImageRgb8(photo(640, 480));
    sb.fs
        .write(&sb.path("a.png"), &encode(&base, ImageFormat::Png))
        .unwrap();
    sb.fs
        .write(&sb.path("b.dng"), &tiff_raw(&jpeg(&base, 85)))
        .unwrap();
    for name in ["a.png", "b.dng"] {
        let thumb = prism_core::media::thumbnail_jpeg(&sb.path(name), 160).unwrap();
        let img = image::load_from_memory_with_format(&thumb, ImageFormat::Jpeg).unwrap();
        assert_eq!((img.width(), img.height()), (160, 120), "{name}");
    }
    sb.fs.write(&sb.path("c.jpg"), b"not an image").unwrap();
    assert!(prism_core::media::thumbnail_jpeg(&sb.path("c.jpg"), 160).is_err());
}

/// Decodes "videos" written by the test: `VID<seed>` gives frames derived from the seed.
struct FakeDecoder;

impl prism_core::media::video::VideoDecoder for FakeDecoder {
    fn print(
        &self,
        path: &std::path::Path,
    ) -> Result<prism_core::media::video::VideoPrint, String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        let seed = data
            .strip_prefix(b"VID")
            .and_then(|r| r.first())
            .ok_or("not a video")?;
        let base = u64::from(*seed) * 0x0101_0101_0101_0101;
        Ok(prism_core::media::video::VideoPrint {
            duration_ms: 90_000,
            width: 1280,
            height: 720,
            frames: (0..5).map(|i| base.rotate_left(i * 7)).collect(),
        })
    }
}

// F12-AC1/AC3: the background pass prints the videos (not other files), keeps the undecodable
// ones marked, and the prints group the same content.
#[test]
fn f12_background_video_prints_and_groups() {
    use prism_core::media::video::{groups, print_videos, video_progress};

    let sb = Sandbox::new();
    sb.fs.write(&sb.path("a.mp4"), b"VID1 original").unwrap();
    sb.fs
        .write(&sb.path("b.mkv"), b"VID1 copy, other size")
        .unwrap();
    sb.fs.write(&sb.path("c.avi"), b"VID9").unwrap();
    sb.fs.write(&sb.path("broken.mp4"), b"garbage").unwrap();
    sb.fs
        .write(&sb.path("app.ts"), b"VID1 but TypeScript")
        .unwrap();
    let cat = TestCatalog::new();
    let volume = cat.add_volume(&sb.root().to_string_lossy());
    cat.index(volume, sb.root());

    let progress = JobProgress::default();
    let report = print_videos(
        &cat.store,
        volume,
        sb.root(),
        &FakeDecoder,
        &CancelToken::new(),
        &|| false,
        &progress,
    )
    .unwrap();
    assert_eq!((report.printed, report.unreadable), (3, 1));
    assert_eq!(
        cat.store.read(|c| video_progress(c, volume)).unwrap(),
        (4, 4)
    );
    let videos = cat.store.read(store::valid_video_prints).unwrap();
    assert_eq!(videos.len(), 3);
    let input: Vec<(u64, &[u64])> = videos
        .iter()
        .map(|v| (v.duration_ms, v.frames.as_slice()))
        .collect();
    let found = groups(&input, 2.0);
    assert_eq!(found.len(), 1);
    let names: Vec<String> = found[0]
        .iter()
        .map(|&i| {
            cat.store
                .read(|c| store::entry(c, videos[i].entry))
                .unwrap()
                .unwrap()
                .name
        })
        .collect();
    assert_eq!(names, ["a.mp4", "b.mkv"]);

    // A paused pass stops at once and saves nothing more.
    sb.fs.write(&sb.path("d.mov"), b"VID5").unwrap();
    cat.index(volume, sb.root());
    let paused = print_videos(
        &cat.store,
        volume,
        sb.root(),
        &FakeDecoder,
        &CancelToken::new(),
        &|| true,
        &progress,
    )
    .unwrap();
    assert!(paused.interrupted);
    assert_eq!(
        cat.store.read(|c| video_progress(c, volume)).unwrap(),
        (5, 4)
    );
}

// REVIEW L31: the analysis reads each image once. With several readers, the images read whole
// for their hashes are printed from the same bytes, with the prints the print pass gives, and
// the print pass has nothing left; the hashes are those of a run that reads one file at a time.
#[test]
fn images_are_printed_from_the_bytes_read_for_their_hashes() {
    use prism_core::hashing::{
        HashOptions, HashProgress, SMALL_FILE, hash_volume, hash_volume_with,
    };
    let sb = Sandbox::new();
    let base = DynamicImage::ImageRgb8(photo(400, 300));
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("a.png", encode(&base, ImageFormat::Png)),
        ("b.jpg", jpeg(&base, 50)),
        ("copy/b.jpg", jpeg(&base, 50)),
        (
            "c.gif",
            encode(&DynamicImage::ImageRgb8(other(400, 300)), ImageFormat::Gif),
        ),
        ("broken.jpg", b"\xFF\xD8\xFF garbage".to_vec()),
        ("notes.txt", b"not an image".to_vec()),
        (
            "big.bin",
            (0..2 * SMALL_FILE + 5).map(|i| (i % 249) as u8).collect(),
        ),
    ];
    sb.fs.create_dir_all(&sb.path("copy")).unwrap();
    for (rel, data) in &files {
        sb.fs.write(&sb.path(rel), data).unwrap();
    }
    let state = |cat: &TestCatalog, volume| {
        let prints = cat.store.read(store::valid_prints).unwrap();
        files
            .iter()
            .map(|(rel, _)| {
                let id = cat
                    .store
                    .read(|c| store::resolve_relative_path(c, volume, &rel.replace('/', "\\")))
                    .unwrap()
                    .unwrap()
                    .id;
                let hashes = cat
                    .store
                    .read(|c| store::valid_hashes(c, id))
                    .unwrap()
                    .unwrap();
                let print = prints.iter().find(|p| p.entry == id).map(|p| p.dhash);
                (hashes.full, print)
            })
            .collect::<Vec<_>>()
    };

    // As before: one file at a time, then the print pass.
    let before = TestCatalog::new();
    let v1 = before.add_volume(&sb.root().to_string_lossy());
    before.index(v1, sb.root());
    let cancel = CancelToken::new();
    hash_volume(
        &before.store,
        v1,
        sb.root(),
        None,
        &cancel,
        &|| false,
        &HashProgress::default(),
    )
    .unwrap();
    print_volume(
        &before.store,
        v1,
        sb.root(),
        &Workers::one(),
        &cancel,
        &|| false,
        &JobProgress::default(),
    )
    .unwrap();

    let now = TestCatalog::new();
    let v2 = now.add_volume(&sb.root().to_string_lossy());
    now.index(v2, sb.root());
    let started = std::sync::atomic::AtomicUsize::new(0);
    let init = || {
        started.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    };
    let options = HashOptions {
        readers: 4,
        decoders: 2,
        init: &init,
    };
    let report = hash_volume_with(
        &now.store,
        v2,
        sb.root(),
        None,
        &cancel,
        &|| false,
        &HashProgress::default(),
        &options,
    )
    .unwrap();
    assert_eq!(report.full, 7);
    assert_eq!((report.printed, report.not_printed), (4, 1));
    let total: u64 = files.iter().map(|(_, d)| d.len() as u64).sum();
    assert!(report.bytes >= total, "{} < {total}", report.bytes);
    assert!(started.load(std::sync::atomic::Ordering::Relaxed) >= 4);
    let again = print_volume(
        &now.store,
        v2,
        sb.root(),
        &Workers::one(),
        &cancel,
        &|| false,
        &JobProgress::default(),
    )
    .unwrap();
    assert_eq!(again.printed + again.unreadable + again.changed, 0);
    assert_eq!(now.store.read(|c| print_progress(c, v2)).unwrap(), (5, 5));
    assert_eq!(state(&now, v2), state(&before, v1));
}
