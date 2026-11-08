//! Image prints with one or more threads (multithreading of the content analysis, REVIEW D60).
//!
//! Reads the pictures of a folder — read-only — and prints them as the background job does,
//! every thread in Windows background mode. A first pass reads every file once, so the runs
//! measure the decoding and not the disk; the time of that first pass is reported too.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use prism_core::media;

const EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];

fn pictures(folder: &Path, limit: usize, out: &mut Vec<PathBuf>) {
    let Ok(dir) = std::fs::read_dir(folder) else {
        return;
    };
    let mut subfolders = Vec::new();
    for entry in dir.filter_map(Result::ok) {
        if out.len() >= limit {
            return;
        }
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            subfolders.push(path);
        } else if path
            .extension()
            .is_some_and(|e| EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str()))
        {
            out.push(path);
        }
    }
    for f in subfolders {
        pictures(&f, limit, out);
    }
}

pub fn run(folder: &Path, limit: usize, threads: &[usize]) -> Result<(), String> {
    let mut files = Vec::new();
    pictures(folder, limit, &mut files);
    if files.is_empty() {
        return Err(format!("no pictures in {}", folder.display()));
    }
    let start = Instant::now();
    let bytes: u64 = files
        .iter()
        .filter_map(|f| std::fs::read(f).ok())
        .map(|d| d.len() as u64)
        .sum();
    println!(
        "{} pictures, {:.1} MB, first read {:.2} s (cores: {})",
        files.len(),
        bytes as f64 / 1e6,
        start.elapsed().as_secs_f64(),
        std::thread::available_parallelism().map_or(0, std::num::NonZero::get)
    );
    println!("| Threads | Time | Pictures/s | Speed-up |");
    println!("|---|---|---|---|");
    let mut base = None;
    for &t in threads {
        let next = AtomicUsize::new(0);
        let failed = AtomicUsize::new(0);
        let start = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..t.max(1) {
                s.spawn(|| {
                    let _background = prism_win::process::BackgroundMode::enter();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(f) = files.get(i) else { break };
                        if media::print_image(f).is_err() {
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        let secs = start.elapsed().as_secs_f64();
        let base_secs = *base.get_or_insert(secs);
        println!(
            "| {t} | {secs:.2} s | {:.1} | {:.2}× |",
            files.len() as f64 / secs,
            base_secs / secs
        );
        if failed.load(Ordering::Relaxed) > 0 {
            println!("  ({} not decoded)", failed.load(Ordering::Relaxed));
        }
    }
    Ok(())
}
