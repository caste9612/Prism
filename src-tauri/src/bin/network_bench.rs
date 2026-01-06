//! Network drive scanning benchmark tool
//!
//! Usage: cargo run --bin network_bench [drive_path]
//! Default: X:\
//!
//! Tests different scanning strategies and measures performance.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let drive_path = std::env::args().nth(1).unwrap_or_else(|| "X:\\".to_string());

    println!("====================================");
    println!("Network Drive Scanning Benchmark");
    println!("====================================");
    println!("Target: {}", drive_path);
    println!();

    // Verify drive is accessible
    if !Path::new(&drive_path).exists() {
        eprintln!("ERROR: Drive {} is not accessible", drive_path);
        return;
    }

    // Run benchmarks
    println!("Running benchmarks...\n");

    // 1. jwalk with default settings
    bench_jwalk(&drive_path, "jwalk (default)", 4, false);

    // 2. jwalk with more threads
    bench_jwalk(&drive_path, "jwalk (8 threads)", 8, false);

    // 3. jwalk with 2 threads (lower contention)
    bench_jwalk(&drive_path, "jwalk (2 threads)", 2, false);

    // 4. jwalk single-threaded
    bench_jwalk(&drive_path, "jwalk (1 thread)", 1, false);

    // 5. std::fs recursive (baseline)
    bench_std_fs(&drive_path, "std::fs recursive");

    // 6. walkdir crate
    bench_walkdir(&drive_path, "walkdir");

    println!("\n====================================");
    println!("Benchmark complete!");
    println!("====================================");
}

fn bench_jwalk(drive_path: &str, name: &str, num_threads: usize, skip_hidden: bool) {
    println!("--- {} ---", name);

    let start = Instant::now();
    let file_count = AtomicU64::new(0);
    let dir_count = AtomicU64::new(0);
    let error_count = AtomicU64::new(0);
    let total_size = AtomicU64::new(0);

    let parallelism = if num_threads == 1 {
        jwalk::Parallelism::Serial
    } else {
        jwalk::Parallelism::RayonNewPool(num_threads)
    };

    let walker = jwalk::WalkDir::new(drive_path)
        .skip_hidden(skip_hidden)
        .parallelism(parallelism)
        .process_read_dir(|_depth, _path, _state, children| {
            // Skip problematic directories
            children.retain(|entry| {
                if let Ok(e) = entry {
                    if e.file_type().is_dir() {
                        let name = e.file_name().to_string_lossy().to_uppercase();
                        if name == "$RECYCLE.BIN" || name == "SYSTEM VOLUME INFORMATION" {
                            return false;
                        }
                    }
                }
                true
            });
        });

    for entry in walker {
        match entry {
            Ok(entry) => {
                if entry.file_type().is_file() {
                    file_count.fetch_add(1, Ordering::Relaxed);
                    if let Ok(meta) = entry.metadata() {
                        total_size.fetch_add(meta.len(), Ordering::Relaxed);
                    }
                } else if entry.file_type().is_dir() {
                    dir_count.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(_) => {
                error_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    let elapsed = start.elapsed();
    let files = file_count.load(Ordering::Relaxed);
    let dirs = dir_count.load(Ordering::Relaxed);
    let errors = error_count.load(Ordering::Relaxed);
    let size = total_size.load(Ordering::Relaxed);
    let fps = files as f64 / elapsed.as_secs_f64();

    println!("  Files:  {} ({:.2} GB)", files, size as f64 / 1_073_741_824.0);
    println!("  Dirs:   {}", dirs);
    println!("  Errors: {}", errors);
    println!("  Time:   {:.2}s", elapsed.as_secs_f64());
    println!("  Speed:  {:.0} files/sec", fps);
    println!();
}

fn bench_std_fs(drive_path: &str, name: &str) {
    println!("--- {} ---", name);

    let start = Instant::now();
    let mut file_count = 0u64;
    let mut dir_count = 0u64;
    let mut error_count = 0u64;
    let mut total_size = 0u64;

    fn walk_dir(path: &Path, file_count: &mut u64, dir_count: &mut u64, error_count: &mut u64, total_size: &mut u64) {
        let entries = match fs::read_dir(path) {
            Ok(e) => e,
            Err(_) => {
                *error_count += 1;
                return;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    *error_count += 1;
                    continue;
                }
            };

            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };

            if file_type.is_file() {
                *file_count += 1;
                if let Ok(meta) = entry.metadata() {
                    *total_size += meta.len();
                }
            } else if file_type.is_dir() {
                let name = entry.file_name().to_string_lossy().to_uppercase();
                if name != "$RECYCLE.BIN" && name != "SYSTEM VOLUME INFORMATION" {
                    *dir_count += 1;
                    walk_dir(&path, file_count, dir_count, error_count, total_size);
                }
            }
        }
    }

    walk_dir(Path::new(drive_path), &mut file_count, &mut dir_count, &mut error_count, &mut total_size);

    let elapsed = start.elapsed();
    let fps = file_count as f64 / elapsed.as_secs_f64();

    println!("  Files:  {} ({:.2} GB)", file_count, total_size as f64 / 1_073_741_824.0);
    println!("  Dirs:   {}", dir_count);
    println!("  Errors: {}", error_count);
    println!("  Time:   {:.2}s", elapsed.as_secs_f64());
    println!("  Speed:  {:.0} files/sec", fps);
    println!();
}

fn bench_walkdir(drive_path: &str, name: &str) {
    println!("--- {} ---", name);

    let start = Instant::now();
    let mut file_count = 0u64;
    let mut dir_count = 0u64;
    let mut error_count = 0u64;
    let mut total_size = 0u64;

    let walker = walkdir::WalkDir::new(drive_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy().to_uppercase();
            name != "$RECYCLE.BIN" && name != "SYSTEM VOLUME INFORMATION"
        });

    for entry in walker {
        match entry {
            Ok(entry) => {
                if entry.file_type().is_file() {
                    file_count += 1;
                    if let Ok(meta) = entry.metadata() {
                        total_size += meta.len();
                    }
                } else if entry.file_type().is_dir() {
                    dir_count += 1;
                }
            }
            Err(_) => {
                error_count += 1;
            }
        }
    }

    let elapsed = start.elapsed();
    let fps = file_count as f64 / elapsed.as_secs_f64();

    println!("  Files:  {} ({:.2} GB)", file_count, total_size as f64 / 1_073_741_824.0);
    println!("  Dirs:   {}", dir_count);
    println!("  Errors: {}", error_count);
    println!("  Time:   {:.2}s", elapsed.as_secs_f64());
    println!("  Speed:  {:.0} files/sec", fps);
    println!();
}
