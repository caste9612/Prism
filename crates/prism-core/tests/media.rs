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
use prism_core::media::{print_image, print_progress, print_volume, raw};
use prism_core::store;

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
    let report = print_volume(
        &cat.store,
        volume,
        sb.root(),
        &CancelToken::new(),
        &|| false,
        &progress,
    )
    .unwrap();
    assert_eq!(
        (report.printed, report.unreadable, report.changed),
        (3, 1, 0)
    );
    assert_eq!(
        cat.store.read(|c| print_progress(c, volume)).unwrap(),
        (4, 4)
    );
    // Nothing left for a second run.
    let again = print_volume(
        &cat.store,
        volume,
        sb.root(),
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
