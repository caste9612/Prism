//! Reading of the files of a folder as the content analysis does it, with 1, 2, 4… files at
//! the same time (REVIEW L31). Read-only.
//!
//! The files are dealt in turn to the runs, so no file is read twice and no cache helps a
//! later run: the first run reads as Prism 2.1 did (one file at a time, the full hash, then the
//! file opened again by name for its size and date), the others as 2.2 does (one opening,
//! small files read whole, size and date from the open file), every thread in Windows
//! background mode.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use prism_core::cancel::CancelToken;
use prism_core::hashing;
use prism_core::scan::disk_meta;

/// Reads one file (path, size) and adds the bytes read; false if it could not be read.
type ReadOne<'a> = &'a (dyn Fn(&Path, u64, &AtomicU64) -> bool + Sync);

/// Files of `folder` with their size, folders first in the order they are listed.
fn files(folder: &Path, limit: usize, out: &mut Vec<(PathBuf, u64)>) {
    let Ok(dir) = std::fs::read_dir(folder) else {
        return;
    };
    let mut subfolders = Vec::new();
    for entry in dir.filter_map(Result::ok) {
        if out.len() >= limit {
            return;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            subfolders.push(entry.path());
        } else if kind.is_file()
            && let Ok(meta) = entry.metadata()
            && meta.len() > 0
        {
            out.push((entry.path(), meta.len()));
        }
    }
    for f in subfolders {
        if out.len() >= limit {
            return;
        }
        files(&f, limit, out);
    }
}

/// Reads `files` with `threads` threads, each file as `one` says; returns the bytes read and
/// the files that could not be read.
fn read_all(files: &[&(PathBuf, u64)], threads: usize, one: ReadOne<'_>) -> (u64, u64) {
    let next = AtomicUsize::new(0);
    let bytes = AtomicU64::new(0);
    let failed = AtomicU64::new(0);
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| {
                let _background = prism_win::process::BackgroundMode::enter();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((path, size)) = files.get(i) else {
                        break;
                    };
                    if !one(path, *size, &bytes) {
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    (bytes.into_inner(), failed.into_inner())
}

pub fn run(folder: &Path, limit: usize, readers: &[usize]) -> Result<(), String> {
    let start = Instant::now();
    let mut all = Vec::new();
    files(folder, limit, &mut all);
    if all.is_empty() {
        return Err(format!("no files in {}", folder.display()));
    }
    let runs = readers.len() + 1;
    let total: u64 = all.iter().map(|(_, s)| s).sum();
    println!(
        "{} files, {:.1} MB, listed in {:.1} s; each run reads about 1/{runs} of them",
        all.len(),
        total as f64 / 1e6,
        start.elapsed().as_secs_f64()
    );
    println!("| Reading | Files | MB | Time | Files/s | MB/s | Not read |");
    println!("|---|---|---|---|---|---|---|");
    let cancel = CancelToken::new();
    let old = |path: &Path, size: u64, bytes: &AtomicU64| {
        hashing::full_hash(path, &cancel, bytes).is_ok()
            && disk_meta(path).is_ok_and(|(_, m)| m.size == size)
    };
    let new = |path: &Path, size: u64, bytes: &AtomicU64| {
        let count = |n: u64| {
            bytes.fetch_add(n, Ordering::Relaxed);
        };
        hashing::read_file(path, size, true, false, &|s, _| s == size, &cancel, &count)
            .is_ok_and(|r| r.full.is_some() && r.size == size)
    };
    for run in 0..runs {
        let slice: Vec<&(PathBuf, u64)> = all.iter().skip(run).step_by(runs).collect();
        let (label, threads, one): (String, usize, ReadOne<'_>) = if run == 0 {
            ("2.1: one at a time, opened twice".to_owned(), 1, &old)
        } else {
            let r = readers[run - 1];
            (format!("2.2: {r} at a time, opened once"), r, &new)
        };
        let t = Instant::now();
        let (bytes, failed) = read_all(&slice, threads, one);
        let secs = t.elapsed().as_secs_f64().max(1e-9);
        println!(
            "| {label} | {} | {:.1} | {secs:.1} s | {:.1} | {:.1} | {failed} |",
            slice.len(),
            bytes as f64 / 1e6,
            slice.len() as f64 / secs,
            bytes as f64 / 1e6 / secs
        );
    }
    Ok(())
}
