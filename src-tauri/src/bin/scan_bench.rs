//! Network Drive Scan Benchmark Tool v2
//!
//! Tests multiple strategies to find optimal network drive scanning approach

use anyhow::Result;
use crossbeam_channel::bounded;
use rayon::prelude::*;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::thread;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
struct BenchConfig {
    path: String,
    threads: usize,
    file_limit: Option<u64>,
    time_limit_secs: u64,
}

struct BenchResult {
    files_scanned: u64,
    total_bytes: u64,
    elapsed: Duration,
    files_per_sec: f64,
    bytes_per_sec: f64,
    strategy: String,
    threads: usize,
}

fn is_network_path(path: &str) -> bool {
    if path.starts_with("\\\\") {
        return true;
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDriveTypeW(root_path: *const u16) -> u32;
        }
        const DRIVE_REMOTE: u32 = 4;
        if path.len() >= 2 && path.chars().nth(1) == Some(':') {
            let drive_root = format!("{}\\", &path[..2]);
            let mut path_wide: Vec<u16> = drive_root.encode_utf16().collect();
            path_wide.push(0);
            let drive_type = unsafe { GetDriveTypeW(path_wide.as_ptr()) };
            return drive_type == DRIVE_REMOTE;
        }
    }
    false
}

/// Strategy 1: Sequential walkdir (baseline)
fn bench_sequential_walkdir(config: &BenchConfig, stop_flag: Arc<AtomicBool>) -> BenchResult {
    let mut files_scanned = 0u64;
    let mut total_bytes = 0u64;
    let start = Instant::now();

    println!("\n[SEQUENTIAL-WALKDIR] Single-threaded baseline...");

    for entry in WalkDir::new(&config.path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        if let Some(limit) = config.file_limit {
            if files_scanned >= limit {
                break;
            }
        }
        if let Ok(meta) = entry.metadata() {
            files_scanned += 1;
            total_bytes += meta.len();
        }
    }

    let elapsed = start.elapsed();
    let secs = elapsed.as_secs_f64().max(0.001);

    BenchResult {
        files_scanned,
        total_bytes,
        elapsed,
        files_per_sec: files_scanned as f64 / secs,
        bytes_per_sec: total_bytes as f64 / secs,
        strategy: "sequential-walkdir".to_string(),
        threads: 1,
    }
}

/// Strategy 2: Collect paths then parallel metadata fetch
fn bench_collect_then_parallel(config: &BenchConfig, stop_flag: Arc<AtomicBool>) -> BenchResult {
    let start = Instant::now();
    let threads = config.threads;

    println!("\n[COLLECT-PARALLEL] Collect paths, then parallel metadata ({} threads)...", threads);

    // Phase 1: Collect paths sequentially (fast on network)
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(&config.path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        if let Some(limit) = config.file_limit {
            if paths.len() >= limit as usize {
                break;
            }
        }
        paths.push(entry.path().to_path_buf());
    }

    let collect_time = start.elapsed();
    println!("  Phase 1: Collected {} paths in {:.2}s", paths.len(), collect_time.as_secs_f64());

    if stop_flag.load(Ordering::Relaxed) {
        return BenchResult {
            files_scanned: paths.len() as u64,
            total_bytes: 0,
            elapsed: start.elapsed(),
            files_per_sec: 0.0,
            bytes_per_sec: 0.0,
            strategy: format!("collect-parallel-{}", threads),
            threads,
        };
    }

    // Phase 2: Parallel metadata fetch
    let files_scanned = Arc::new(AtomicU64::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let stop = Arc::clone(&stop_flag);

    // Configure rayon with specific thread count
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();

    pool.install(|| {
        paths.par_iter().for_each(|path| {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if let Ok(meta) = fs::metadata(path) {
                files_scanned.fetch_add(1, Ordering::Relaxed);
                total_bytes.fetch_add(meta.len(), Ordering::Relaxed);
            }
        });
    });

    let elapsed = start.elapsed();
    let files = files_scanned.load(Ordering::SeqCst);
    let bytes = total_bytes.load(Ordering::SeqCst);
    let secs = elapsed.as_secs_f64().max(0.001);

    BenchResult {
        files_scanned: files,
        total_bytes: bytes,
        elapsed,
        files_per_sec: files as f64 / secs,
        bytes_per_sec: bytes as f64 / secs,
        strategy: format!("collect-parallel-{}", threads),
        threads,
    }
}

/// Strategy 3: Chunked parallel - process in batches
fn bench_chunked_parallel(config: &BenchConfig, stop_flag: Arc<AtomicBool>, chunk_size: usize) -> BenchResult {
    let start = Instant::now();
    let threads = config.threads;

    println!("\n[CHUNKED-PARALLEL] Batch processing (chunk={}, threads={})...", chunk_size, threads);

    let files_scanned = Arc::new(AtomicU64::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();

    let mut chunk: Vec<PathBuf> = Vec::with_capacity(chunk_size);
    let mut total_collected = 0u64;

    for entry in WalkDir::new(&config.path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        if let Some(limit) = config.file_limit {
            if total_collected >= limit {
                break;
            }
        }

        chunk.push(entry.path().to_path_buf());
        total_collected += 1;

        if chunk.len() >= chunk_size {
            // Process chunk in parallel
            let fs = Arc::clone(&files_scanned);
            let tb = Arc::clone(&total_bytes);
            let stop = Arc::clone(&stop_flag);
            let batch = std::mem::replace(&mut chunk, Vec::with_capacity(chunk_size));

            pool.install(|| {
                batch.par_iter().for_each(|path| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Ok(meta) = fs::metadata(path) {
                        fs.fetch_add(1, Ordering::Relaxed);
                        tb.fetch_add(meta.len(), Ordering::Relaxed);
                    }
                });
            });
        }
    }

    // Process remaining
    if !chunk.is_empty() {
        let fs = Arc::clone(&files_scanned);
        let tb = Arc::clone(&total_bytes);
        pool.install(|| {
            chunk.par_iter().for_each(|path| {
                if let Ok(meta) = fs::metadata(path) {
                    fs.fetch_add(1, Ordering::Relaxed);
                    tb.fetch_add(meta.len(), Ordering::Relaxed);
                }
            });
        });
    }

    let elapsed = start.elapsed();
    let files = files_scanned.load(Ordering::SeqCst);
    let bytes = total_bytes.load(Ordering::SeqCst);
    let secs = elapsed.as_secs_f64().max(0.001);

    BenchResult {
        files_scanned: files,
        total_bytes: bytes,
        elapsed,
        files_per_sec: files as f64 / secs,
        bytes_per_sec: bytes as f64 / secs,
        strategy: format!("chunked-{}-t{}", chunk_size, threads),
        threads,
    }
}

/// Strategy 4: Producer-Consumer with channel
fn bench_producer_consumer(config: &BenchConfig, stop_flag: Arc<AtomicBool>, num_consumers: usize) -> BenchResult {
    let start = Instant::now();

    println!("\n[PRODUCER-CONSUMER] 1 producer, {} consumers...", num_consumers);

    let (tx, rx) = bounded::<PathBuf>(10000);
    let files_scanned = Arc::new(AtomicU64::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));

    // Spawn producer
    let path = config.path.clone();
    let file_limit = config.file_limit;
    let stop = Arc::clone(&stop_flag);
    let producer = thread::spawn(move || {
        for entry in WalkDir::new(&path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            if let Some(limit) = file_limit {
                // Approximate limit check
                if tx.len() > limit as usize {
                    break;
                }
            }
            if tx.send(entry.path().to_path_buf()).is_err() {
                break;
            }
        }
    });

    // Spawn consumers
    let mut consumers = Vec::new();
    for _ in 0..num_consumers {
        let rx = rx.clone();
        let fs = Arc::clone(&files_scanned);
        let tb = Arc::clone(&total_bytes);
        let stop = Arc::clone(&stop_flag);

        consumers.push(thread::spawn(move || {
            while let Ok(path) = rx.recv() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok(meta) = fs::metadata(&path) {
                    fs.fetch_add(1, Ordering::Relaxed);
                    tb.fetch_add(meta.len(), Ordering::Relaxed);
                }
            }
        }));
    }

    // Drop our rx so consumers can exit when producer is done
    drop(rx);

    // Wait for all
    producer.join().ok();
    for c in consumers {
        c.join().ok();
    }

    let elapsed = start.elapsed();
    let files = files_scanned.load(Ordering::SeqCst);
    let bytes = total_bytes.load(Ordering::SeqCst);
    let secs = elapsed.as_secs_f64().max(0.001);

    BenchResult {
        files_scanned: files,
        total_bytes: bytes,
        elapsed,
        files_per_sec: files as f64 / secs,
        bytes_per_sec: bytes as f64 / secs,
        strategy: format!("producer-consumer-{}", num_consumers),
        threads: num_consumers + 1,
    }
}

/// Strategy 5: Parallel directory traversal with sequential file processing
fn bench_parallel_dirs(config: &BenchConfig, stop_flag: Arc<AtomicBool>) -> BenchResult {
    let start = Instant::now();
    let threads = config.threads;

    println!("\n[PARALLEL-DIRS] Parallel directory enumeration ({} threads)...", threads);

    let files_scanned = Arc::new(AtomicU64::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));

    // First, collect all directories
    let mut dirs: Vec<PathBuf> = vec![PathBuf::from(&config.path)];
    let mut all_dirs: Vec<PathBuf> = Vec::new();

    while !dirs.is_empty() && !stop_flag.load(Ordering::Relaxed) {
        let mut new_dirs = Vec::new();
        for dir in &dirs {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    if let Ok(ft) = entry.file_type() {
                        if ft.is_dir() {
                            new_dirs.push(entry.path());
                        }
                    }
                }
            }
        }
        all_dirs.extend(dirs);
        dirs = new_dirs;
    }

    println!("  Found {} directories", all_dirs.len());

    if stop_flag.load(Ordering::Relaxed) {
        return BenchResult {
            files_scanned: 0,
            total_bytes: 0,
            elapsed: start.elapsed(),
            files_per_sec: 0.0,
            bytes_per_sec: 0.0,
            strategy: format!("parallel-dirs-{}", threads),
            threads,
        };
    }

    // Process directories in parallel
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();

    let fs_count = Arc::clone(&files_scanned);
    let tb_count = Arc::clone(&total_bytes);
    let stop = Arc::clone(&stop_flag);
    let limit = config.file_limit;

    pool.install(|| {
        all_dirs.par_iter().for_each(|dir| {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if let Some(l) = limit {
                if fs_count.load(Ordering::Relaxed) >= l {
                    return;
                }
            }

            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    if let Ok(ft) = entry.file_type() {
                        if ft.is_file() {
                            if let Ok(meta) = entry.metadata() {
                                fs_count.fetch_add(1, Ordering::Relaxed);
                                tb_count.fetch_add(meta.len(), Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
        });
    });

    let elapsed = start.elapsed();
    let files = files_scanned.load(Ordering::SeqCst);
    let bytes = total_bytes.load(Ordering::SeqCst);
    let secs = elapsed.as_secs_f64().max(0.001);

    BenchResult {
        files_scanned: files,
        total_bytes: bytes,
        elapsed,
        files_per_sec: files as f64 / secs,
        bytes_per_sec: bytes as f64 / secs,
        strategy: format!("parallel-dirs-{}", threads),
        threads,
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

fn print_result(result: &BenchResult) {
    println!("\n{}", "=".repeat(60));
    println!("Strategy: {}", result.strategy);
    println!("{}", "-".repeat(60));
    println!("Files scanned:    {:>15}", result.files_scanned);
    println!("Total size:       {:>15}", format_bytes(result.total_bytes));
    println!("Time elapsed:     {:>15.2}s", result.elapsed.as_secs_f64());
    println!("Files/sec:        {:>15.0}", result.files_per_sec);
    println!("Throughput:       {:>15}/s", format_bytes(result.bytes_per_sec as u64));
    println!("{}", "=".repeat(60));
}

fn run_benchmarks(config: &BenchConfig) {
    let is_network = is_network_path(&config.path);

    println!("\n{}", "#".repeat(70));
    println!("# NETWORK DRIVE SCAN BENCHMARK v2");
    println!("# Path: {}", config.path);
    println!("# Is Network: {}", is_network);
    println!("# Base threads: {}", config.threads);
    println!("# Time limit: {}s per test", config.time_limit_secs);
    if let Some(limit) = config.file_limit {
        println!("# File limit: {}", limit);
    }
    println!("{}", "#".repeat(70));

    let mut results: Vec<BenchResult> = Vec::new();

    // Helper to create stop flag with timer
    let make_stop_flag = |secs: u64| -> Arc<AtomicBool> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(secs));
            stop_clone.store(true, Ordering::SeqCst);
        });
        stop
    };

    // 1. Sequential baseline
    {
        let stop = make_stop_flag(config.time_limit_secs);
        let result = bench_sequential_walkdir(config, stop);
        print_result(&result);
        results.push(result);
    }

    // 2. Producer-Consumer with different consumer counts
    for consumers in [2, 4, 8, 16] {
        let stop = make_stop_flag(config.time_limit_secs);
        let result = bench_producer_consumer(config, stop, consumers);
        print_result(&result);
        results.push(result);
    }

    // 3. Chunked parallel with different chunk sizes
    for (chunk, threads) in [(1000, 4), (5000, 8), (10000, 16)] {
        let mut cfg = config.clone();
        cfg.threads = threads;
        let stop = make_stop_flag(config.time_limit_secs);
        let result = bench_chunked_parallel(&cfg, stop, chunk);
        print_result(&result);
        results.push(result);
    }

    // 4. Parallel directories
    for threads in [4, 8, 16] {
        let mut cfg = config.clone();
        cfg.threads = threads;
        let stop = make_stop_flag(config.time_limit_secs);
        let result = bench_parallel_dirs(&cfg, stop);
        print_result(&result);
        results.push(result);
    }

    // Print comparison
    println!("\n\n{}", "=".repeat(80));
    println!("COMPARISON SUMMARY");
    println!("{}", "=".repeat(80));
    println!("{:<30} {:>10} {:>15} {:>15}", "Strategy", "Threads", "Files/sec", "Throughput/s");
    println!("{}", "-".repeat(80));

    results.sort_by(|a, b| b.files_per_sec.partial_cmp(&a.files_per_sec).unwrap());

    for (i, r) in results.iter().enumerate() {
        let marker = if i == 0 { " <-- BEST" } else { "" };
        println!(
            "{:<30} {:>10} {:>15.0} {:>15}{}",
            r.strategy,
            r.threads,
            r.files_per_sec,
            format_bytes(r.bytes_per_sec as u64),
            marker
        );
    }

    if let (Some(best), Some(baseline)) = (results.first(), results.iter().find(|r| r.strategy == "sequential-walkdir")) {
        if best.files_per_sec > baseline.files_per_sec {
            let improvement = (best.files_per_sec / baseline.files_per_sec - 1.0) * 100.0;
            println!("\n>>> BEST: {} ({:.0}% faster than sequential)", best.strategy, improvement);
        } else {
            println!("\n>>> Sequential is optimal for this drive");
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Network Drive Scan Benchmark Tool v2");
        println!("\nUsage: {} <path> [options]", args[0]);
        println!("\nOptions:");
        println!("  --threads <n>     Base thread count (default: CPU cores)");
        println!("  --limit <n>       Stop after N files (default: unlimited)");
        println!("  --time <secs>     Time per test in seconds (default: 20)");
        println!("\nExamples:");
        println!("  {} Z:\\ --time 30", args[0]);
        println!("  {} \\\\\\\\server\\\\share --limit 50000", args[0]);
        return Ok(());
    }

    let path = args[1].clone();

    let mut threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let mut file_limit: Option<u64> = None;
    let mut time_limit_secs = 20u64;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--threads" => {
                threads = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(threads);
                i += 2;
            }
            "--limit" => {
                file_limit = args.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "--time" => {
                time_limit_secs = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(20);
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    if !Path::new(&path).exists() {
        eprintln!("ERROR: Path does not exist: {}", path);
        return Ok(());
    }

    let config = BenchConfig {
        path,
        threads,
        file_limit,
        time_limit_secs,
    };

    run_benchmarks(&config);

    Ok(())
}
