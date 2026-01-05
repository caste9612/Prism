//! Directory walker for filesystem traversal - Highly optimized for maximum throughput
//!
//! Network drives use producer-consumer pattern (1 producer + 16 consumers)
//! Local drives use jwalk's parallel traversal

use super::{DriveProgress, FileMetadata, ScanProgress};
use crate::database::Database;
use anyhow::Result;
use crossbeam_channel::{bounded, Sender};
use jwalk::WalkDir as JWalkDir;
use parking_lot::RwLock;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

/// Per-drive progress counters
struct DriveCounters {
    files_scanned: AtomicU64,
    total_size: AtomicU64,
    is_scanning: AtomicBool,
    is_done: AtomicBool,
    /// Estimated total size for progress calculation
    estimated_total: AtomicU64,
}

impl Default for DriveCounters {
    fn default() -> Self {
        Self {
            files_scanned: AtomicU64::new(0),
            total_size: AtomicU64::new(0),
            is_scanning: AtomicBool::new(false),
            is_done: AtomicBool::new(false),
            estimated_total: AtomicU64::new(0),
        }
    }
}

/// Configuration for scanning
#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub root_paths: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub exclude_hidden: bool,
    pub exclude_system: bool,
    pub min_file_size: u64,
    pub max_file_size: Option<u64>,
    pub hash_threshold: u64,
    pub batch_size: usize,
    pub num_threads: usize,
}

impl Default for ScanConfig {
    fn default() -> Self {
        // Use all available CPU cores for maximum performance
        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        Self {
            root_paths: vec![],
            exclude_patterns: vec![
                "node_modules".to_string(),
                ".git".to_string(),
                "$RECYCLE.BIN".to_string(),
                "System Volume Information".to_string(),
                "Windows".to_string(),
                "Program Files".to_string(),
                "Program Files (x86)".to_string(),
                "ProgramData".to_string(),
            ],
            exclude_hidden: true,
            exclude_system: true,
            min_file_size: 0,
            max_file_size: None,
            hash_threshold: 1024,
            batch_size: 10000, // Balanced between DB performance and UI responsiveness
            num_threads: num_cpus,
        }
    }
}

/// Filesystem scanner - optimized for maximum throughput with parallel traversal
pub struct Scanner {
    config: ScanConfig,
    files_scanned: Arc<AtomicU64>,
    total_size: Arc<AtomicU64>,
    /// Per-drive progress counters
    drive_counters: Arc<RwLock<HashMap<String, Arc<DriveCounters>>>>,
}

impl Scanner {
    pub fn new(config: ScanConfig) -> Self {
        // Configure Rayon thread pool for maximum parallelism
        rayon::ThreadPoolBuilder::new()
            .num_threads(config.num_threads)
            .build_global()
            .ok(); // Ignore if already initialized

        Self {
            config,
            files_scanned: Arc::new(AtomicU64::new(0)),
            total_size: Arc::new(AtomicU64::new(0)),
            drive_counters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Extract drive letter from path (e.g., "C:\" from "C:\Users\...")
    /// Always normalizes to uppercase with trailing backslash for consistency
    fn get_drive_letter(path: &str) -> String {
        // Handle drive letter paths (C:, C:\, C:\Users, etc.)
        if path.len() >= 2 && path.chars().nth(1) == Some(':') {
            // Always normalize to "X:\" format
            let drive_letter = path.chars().next().unwrap().to_uppercase().next().unwrap();
            format!("{}:\\", drive_letter)
        } else if path.starts_with("\\\\") {
            // UNC path - extract server\share and normalize
            let parts: Vec<&str> = path.trim_start_matches("\\\\").splitn(3, '\\').collect();
            if parts.len() >= 2 {
                format!("\\\\{}\\{}\\", parts[0].to_uppercase(), parts[1].to_uppercase())
            } else {
                path.to_uppercase()
            }
        } else {
            path.to_uppercase()
        }
    }

    /// Check if a path is a network path (UNC or mapped network drive)
    fn is_network_path(path: &str) -> bool {
        // UNC paths start with \\
        if path.starts_with("\\\\") {
            return true;
        }
        // Check if it's a mapped network drive using Windows API
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

    /// Get the used space on a drive (for progress estimation)
    #[cfg(windows)]
    fn get_drive_used_space(path: &str) -> u64 {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                directory: *const u16,
                free_bytes_available: *mut u64,
                total_bytes: *mut u64,
                total_free_bytes: *mut u64,
            ) -> i32;
        }

        // Normalize path: ensure it ends with backslash
        let normalized = if path.ends_with('\\') {
            path.to_string()
        } else if path.len() == 2 && path.ends_with(':') {
            // "C:" -> "C:\"
            format!("{}\\", path)
        } else if path.starts_with("\\\\") && !path.ends_with('\\') {
            // UNC path needs trailing backslash
            format!("{}\\", path)
        } else {
            path.to_string()
        };

        debug!("Getting disk space for: {} (normalized: {})", path, normalized);

        let mut path_wide: Vec<u16> = normalized.encode_utf16().collect();
        path_wide.push(0);

        let mut free_bytes: u64 = 0;
        let mut total_bytes: u64 = 0;
        let mut _total_free: u64 = 0;

        let result = unsafe {
            GetDiskFreeSpaceExW(
                path_wide.as_ptr(),
                &mut free_bytes,
                &mut total_bytes,
                &mut _total_free,
            )
        };

        if result != 0 {
            let used = total_bytes.saturating_sub(free_bytes);
            debug!(
                "Disk space for {}: total={}, free={}, used={}",
                path, total_bytes, free_bytes, used
            );
            used
        } else {
            // For network drives that fail, estimate based on a reasonable default
            // This happens when the network share doesn't support space queries
            warn!(
                "GetDiskFreeSpaceExW failed for {}, using default estimate",
                path
            );
            // Return 0 - we'll handle this in the UI
            0
        }
    }

    #[cfg(not(windows))]
    fn get_drive_used_space(_path: &str) -> u64 {
        0
    }

    /// Get or create drive counters for a path
    fn get_drive_counters(&self, path: &str) -> Arc<DriveCounters> {
        let drive = Self::get_drive_letter(path);
        let counters = self.drive_counters.read();
        if let Some(c) = counters.get(&drive) {
            return Arc::clone(c);
        }
        drop(counters);

        let mut counters = self.drive_counters.write();
        counters
            .entry(drive)
            .or_insert_with(|| Arc::new(DriveCounters::default()))
            .clone()
    }

    /// Build drives progress map for the current state
    fn build_drives_progress(&self) -> HashMap<String, DriveProgress> {
        let counters = self.drive_counters.read();
        counters
            .iter()
            .map(|(drive, c)| {
                let is_done = c.is_done.load(Ordering::Relaxed);
                let is_scanning = c.is_scanning.load(Ordering::Relaxed);
                let files = c.files_scanned.load(Ordering::Relaxed);
                let scanned = c.total_size.load(Ordering::Relaxed);
                let estimated = c.estimated_total.load(Ordering::Relaxed);

                let status = if is_done {
                    "done"
                } else if is_scanning {
                    "scanning"
                } else if files > 0 {
                    // Has files but not marked as scanning - treat as scanning
                    "scanning"
                } else {
                    "waiting"
                };

                // Calculate progress percentage - use file count if no size estimate
                let progress_percent = if status == "done" {
                    100.0
                } else if estimated > 0 && scanned > 0 {
                    // Progress based on scanned bytes vs estimated total
                    ((scanned as f64 / estimated as f64) * 100.0).min(99.0)
                } else if files > 0 {
                    // Fallback: estimate based on typical file counts
                    // Network drives typically have 100k-1M files, show indeterminate progress
                    ((files as f64 / 500_000.0) * 100.0).min(95.0)
                } else {
                    0.0
                };

                // Drive key is already normalized by get_drive_letter
                (
                    drive.clone(),
                    DriveProgress {
                        drive: drive.clone(),
                        files_scanned: files,
                        total_size: scanned,
                        status: status.to_string(),
                        progress_percent,
                        estimated_total: estimated,
                    },
                )
            })
            .collect()
    }

    /// Get current scan progress
    pub fn get_progress(&self, scan_id: i64, start_time: Instant, current_path: &str) -> ScanProgress {
        let files_scanned = self.files_scanned.load(Ordering::Relaxed);
        let elapsed = start_time.elapsed().as_secs_f64();
        let files_per_second = if elapsed > 0.0 {
            files_scanned as f64 / elapsed
        } else {
            0.0
        };

        ScanProgress {
            scan_id,
            files_scanned,
            total_size: self.total_size.load(Ordering::Relaxed),
            current_path: current_path.to_string(),
            files_per_second,
            is_complete: false,
            error: None,
            drives: self.build_drives_progress(),
            indexing: None,
        }
    }

    /// Scan a single root path - chooses optimal strategy based on drive type
    fn scan_root(
        &self,
        root_path: &str,
        sender: &Sender<FileMetadata>,
    ) -> Result<()> {
        let root = Path::new(root_path);
        if !root.exists() {
            warn!("Path does not exist: {}", root_path);
            return Ok(());
        }

        let is_network = Self::is_network_path(root_path);

        if is_network {
            // Use producer-consumer pattern for network drives (20-30% faster)
            self.scan_root_network(root_path, sender)
        } else {
            // Use jwalk parallel traversal for local drives
            self.scan_root_local(root_path, sender)
        }
    }

    /// Scan network drive using producer-consumer pattern
    /// Benchmarked: producer-consumer-8 is optimal (197% faster than sequential)
    fn scan_root_network(
        &self,
        root_path: &str,
        output_sender: &Sender<FileMetadata>,
    ) -> Result<()> {
        const NUM_CONSUMERS: usize = 8; // Optimal based on benchmarks (producer-consumer-8)

        let drive_key = Self::get_drive_letter(root_path);
        info!(
            "Scanning network drive with producer-consumer pattern: {} (key: {}, {} consumers)",
            root_path, drive_key, NUM_CONSUMERS
        );

        let drive_counters = self.get_drive_counters(root_path);
        drive_counters.is_scanning.store(true, Ordering::SeqCst);

        debug!(
            "Drive {} counters initialized: is_scanning=true",
            drive_key
        );

        let exclude_patterns = self.config.exclude_patterns.clone();
        let exclude_hidden = self.config.exclude_hidden;
        let exclude_system = self.config.exclude_system;
        let min_size = self.config.min_file_size;
        let max_size = self.config.max_file_size;

        // Channel for paths from producer to consumers
        let (path_tx, path_rx) = bounded::<std::path::PathBuf>(10000);

        // Spawn producer thread - sequential directory walk
        let producer_path = root_path.to_string();
        let producer_patterns = exclude_patterns.clone();
        let producer = thread::spawn(move || {
            for entry in WalkDir::new(&producer_path)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    // Skip excluded directories
                    if e.file_type().is_dir() {
                        let name = e.file_name().to_string_lossy();
                        for pattern in &producer_patterns {
                            if name.eq_ignore_ascii_case(pattern) {
                                return false;
                            }
                        }
                    }
                    true
                })
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                if path_tx.send(entry.path().to_path_buf()).is_err() {
                    break;
                }
            }
        });

        // Spawn consumer threads
        let mut consumers = Vec::with_capacity(NUM_CONSUMERS);
        for _ in 0..NUM_CONSUMERS {
            let rx = path_rx.clone();
            let sender = output_sender.clone();
            let files_scanned = Arc::clone(&self.files_scanned);
            let total_size = Arc::clone(&self.total_size);
            let drive_files = Arc::clone(&drive_counters);
            let drive_size = Arc::clone(&drive_counters);

            consumers.push(thread::spawn(move || {
                while let Ok(path) = rx.recv() {
                    // Get metadata
                    let metadata = match fs::metadata(&path) {
                        Ok(m) => m,
                        Err(_) => continue,
                    };

                    let size = metadata.len();

                    // Get Windows attributes
                    #[cfg(windows)]
                    let attrs = {
                        use std::os::windows::fs::MetadataExt;
                        metadata.file_attributes()
                    };
                    #[cfg(not(windows))]
                    let attrs = 0u32;

                    // Check filters
                    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
                    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

                    if exclude_hidden && (attrs & FILE_ATTRIBUTE_HIDDEN) != 0 {
                        continue;
                    }
                    if exclude_system && (attrs & FILE_ATTRIBUTE_SYSTEM) != 0 {
                        continue;
                    }
                    if size < min_size {
                        continue;
                    }
                    if let Some(max) = max_size {
                        if size > max {
                            continue;
                        }
                    }

                    let name = match path.file_name() {
                        Some(n) => n.to_string_lossy().to_string(),
                        None => continue,
                    };

                    let extension = path
                        .extension()
                        .map(|e| e.to_string_lossy().to_lowercase());

                    let modified_at = metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64);

                    let created_at = metadata
                        .created()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64);

                    // Update counters
                    files_scanned.fetch_add(1, Ordering::Relaxed);
                    total_size.fetch_add(size, Ordering::Relaxed);
                    drive_files.files_scanned.fetch_add(1, Ordering::Relaxed);
                    drive_size.total_size.fetch_add(size, Ordering::Relaxed);

                    let file = FileMetadata {
                        path: path.to_string_lossy().to_string(),
                        name,
                        extension,
                        size: size as i64,
                        created_at,
                        modified_at,
                        accessed_at: None,
                        attributes: Some(attrs),
                        partial_hash: None,
                    };

                    let _ = sender.send(file);
                }
            }));
        }

        // Drop our receiver so consumers exit when producer is done
        drop(path_rx);

        // Wait for all threads
        producer.join().ok();
        for c in consumers {
            c.join().ok();
        }

        drive_counters.is_scanning.store(false, Ordering::SeqCst);
        drive_counters.is_done.store(true, Ordering::SeqCst);

        let final_files = drive_counters.files_scanned.load(Ordering::Relaxed);
        let final_size = drive_counters.total_size.load(Ordering::Relaxed);
        info!(
            "Network drive {} scan complete: {} files, {} bytes (is_done=true)",
            Self::get_drive_letter(root_path),
            final_files,
            final_size
        );

        Ok(())
    }

    /// Scan local drive using jwalk's parallel traversal
    fn scan_root_local(
        &self,
        root_path: &str,
        sender: &Sender<FileMetadata>,
    ) -> Result<()> {
        let root = Path::new(root_path);
        let optimal_threads = self.config.num_threads;

        info!(
            "Scanning local drive with jwalk: {} ({} threads)",
            root_path, optimal_threads
        );

        let drive_counters = self.get_drive_counters(root_path);
        drive_counters.is_scanning.store(true, Ordering::Release);

        let exclude_patterns = self.config.exclude_patterns.clone();
        let exclude_hidden = self.config.exclude_hidden;
        let exclude_system = self.config.exclude_system;
        let min_size = self.config.min_file_size;
        let max_size = self.config.max_file_size;
        let files_scanned = Arc::clone(&self.files_scanned);
        let total_size = Arc::clone(&self.total_size);
        let drive_files = Arc::clone(&drive_counters);
        let drive_size = Arc::clone(&drive_counters);

        // jwalk provides parallel directory traversal
        let walker = JWalkDir::new(root)
            .skip_hidden(false)
            .follow_links(false)
            .parallelism(jwalk::Parallelism::RayonNewPool(optimal_threads))
            .process_read_dir(move |_depth, _path, _state, children| {
                children.retain(|entry| {
                    if let Ok(e) = entry {
                        if e.file_type().is_dir() {
                            let name = e.file_name().to_string_lossy();
                            for pattern in &exclude_patterns {
                                if name.eq_ignore_ascii_case(pattern) {
                                    return false;
                                }
                            }
                        }
                    }
                    true
                });
            });

        // Collect and process in parallel
        let batch: Vec<_> = walker
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| !e.file_type().is_dir())
            .collect();

        batch.par_iter().for_each(|entry| {
            let path = entry.path();

            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => return,
            };

            let size = metadata.len();

            #[cfg(windows)]
            let attrs = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes()
            };
            #[cfg(not(windows))]
            let attrs = 0u32;

            const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
            const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

            if exclude_hidden && (attrs & FILE_ATTRIBUTE_HIDDEN) != 0 {
                return;
            }
            if exclude_system && (attrs & FILE_ATTRIBUTE_SYSTEM) != 0 {
                return;
            }
            if size < min_size {
                return;
            }
            if let Some(max) = max_size {
                if size > max {
                    return;
                }
            }

            let name = match path.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => return,
            };

            let extension = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase());

            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);

            let created_at = metadata
                .created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);

            files_scanned.fetch_add(1, Ordering::Relaxed);
            total_size.fetch_add(size, Ordering::Relaxed);
            drive_files.files_scanned.fetch_add(1, Ordering::Relaxed);
            drive_size.total_size.fetch_add(size, Ordering::Relaxed);

            let file = FileMetadata {
                path: path.to_string_lossy().to_string(),
                name,
                extension,
                size: size as i64,
                created_at,
                modified_at,
                accessed_at: None,
                attributes: Some(attrs),
                partial_hash: None,
            };

            let _ = sender.send(file);
        });

        drive_counters.is_scanning.store(false, Ordering::Release);
        drive_counters.is_done.store(true, Ordering::Release);

        info!(
            "Local drive {} scan complete: {} files, {} bytes",
            Self::get_drive_letter(root_path),
            drive_counters.files_scanned.load(Ordering::Relaxed),
            drive_counters.total_size.load(Ordering::Relaxed)
        );

        Ok(())
    }

    /// Start scanning with real-time progress events - PARALLEL VERSION
    pub fn scan_with_events(
        &self,
        db: &Database,
        scan_id: i64,
        app_handle: Option<&AppHandle>,
    ) -> Result<ScanProgress> {
        let start_time = Instant::now();

        info!(
            "Starting PARALLEL scan {} for {} paths with {} threads",
            scan_id,
            self.config.root_paths.len(),
            self.config.num_threads
        );

        // Reset counters
        self.files_scanned.store(0, Ordering::SeqCst);
        self.total_size.store(0, Ordering::SeqCst);

        // Clear old data from drives being scanned
        info!("Clearing old data from drives: {:?}", self.config.root_paths);
        if let Err(e) = db.clear_drives(&self.config.root_paths) {
            warn!("Failed to clear drives: {}", e);
        }

        // Clear and initialize per-drive counters with estimated totals
        {
            let mut counters = self.drive_counters.write();
            counters.clear();
            for path in &self.config.root_paths {
                let drive = Self::get_drive_letter(path);
                let estimated = Self::get_drive_used_space(path);
                info!("Drive {} estimated used space: {} bytes", drive, estimated);

                let counter = DriveCounters::default();
                counter.estimated_total.store(estimated, Ordering::SeqCst);
                counters.insert(drive, Arc::new(counter));
            }
        }

        // Disable FTS triggers for fast bulk insert
        if let Err(e) = db.disable_fts_triggers() {
            warn!("Failed to disable FTS triggers: {}", e);
        }

        // Channel for collecting files from all scanners
        let (sender, receiver) = bounded::<FileMetadata>(100_000);

        // Spawn progress reporter thread
        let progress_handle = if let Some(handle) = app_handle {
            let handle = handle.clone();
            let files_scanned = Arc::clone(&self.files_scanned);
            let total_size = Arc::clone(&self.total_size);
            let drive_counters = Arc::clone(&self.drive_counters);
            let is_running = Arc::new(AtomicBool::new(true));
            let is_running_clone = Arc::clone(&is_running);

            let progress_thread = thread::spawn(move || {
                while is_running_clone.load(Ordering::Relaxed) {
                    let files = files_scanned.load(Ordering::Relaxed);
                    let size = total_size.load(Ordering::Relaxed);
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let fps = if elapsed > 0.0 { files as f64 / elapsed } else { 0.0 };

                    // Build per-drive progress with normalized keys
                    let mut all_done = true;
                    let mut any_scanning = false;
                    let drives: HashMap<String, DriveProgress> = {
                        let counters = drive_counters.read();
                        counters
                            .iter()
                            .map(|(drive, c)| {
                                let files = c.files_scanned.load(Ordering::Relaxed);
                                let is_done = c.is_done.load(Ordering::Relaxed);
                                let is_scanning = c.is_scanning.load(Ordering::Relaxed);

                                let status = if is_done {
                                    "done"
                                } else if is_scanning || files > 0 {
                                    any_scanning = true;
                                    "scanning"
                                } else {
                                    all_done = false;
                                    "waiting"
                                };

                                if !is_done {
                                    all_done = false;
                                }

                                let scanned = c.total_size.load(Ordering::Relaxed);
                                let estimated = c.estimated_total.load(Ordering::Relaxed);

                                let progress_percent = if status == "done" {
                                    100.0
                                } else if estimated > 0 && scanned > 0 {
                                    ((scanned as f64 / estimated as f64) * 100.0).min(99.0)
                                } else if files > 0 {
                                    ((files as f64 / 500_000.0) * 100.0).min(95.0)
                                } else {
                                    0.0
                                };

                                // Drive key is already normalized by get_drive_letter
                                (
                                    drive.clone(),
                                    DriveProgress {
                                        drive: drive.clone(),
                                        files_scanned: files,
                                        total_size: scanned,
                                        status: status.to_string(),
                                        progress_percent,
                                        estimated_total: estimated,
                                    },
                                )
                            })
                            .collect()
                    };

                    // Show appropriate message based on scan state
                    let current_path = if all_done && !drives.is_empty() {
                        "Rebuilding search index...".to_string()
                    } else if any_scanning {
                        format!("Scanning... ({:.0} files/sec)", fps)
                    } else {
                        "Initializing...".to_string()
                    };

                    let progress = ScanProgress {
                        scan_id,
                        files_scanned: files,
                        total_size: size,
                        current_path,
                        files_per_second: fps,
                        is_complete: false,
                        error: None,
                        drives,
                        indexing: None,
                    };

                    let _ = handle.emit("scan-progress", &progress);
                    thread::sleep(Duration::from_millis(250)); // Update 4x per second
                }
            });

            Some((progress_thread, is_running))
        } else {
            None
        };

        // Scan all root paths in parallel
        let paths = self.config.root_paths.clone();
        let sender_clone = sender.clone();

        // Spawn scanner threads for each root path
        thread::scope(|s| {
            // Spawn a thread for each root path for true parallelism across drives
            for path in &paths {
                let sender = sender_clone.clone();
                let path = path.clone();
                s.spawn(move || {
                    if let Err(e) = self.scan_root(&path, &sender) {
                        warn!("Error scanning {}: {}", path, e);
                    }
                });
            }

            // Drop the cloned sender so receiver knows when all senders are done
            // The original sender must also be dropped!
            drop(sender_clone);
            drop(sender);

            // Collect files and insert in batches
            let mut batch = Vec::with_capacity(self.config.batch_size);
            let mut total_inserted = 0u64;

            while let Ok(file) = receiver.recv() {
                batch.push(file);

                if batch.len() >= self.config.batch_size {
                    if let Err(e) = db.insert_files_transaction(&batch, scan_id) {
                        debug!("Batch insert error: {}", e);
                    }
                    total_inserted += batch.len() as u64;
                    batch.clear();
                }
            }

            // Insert remaining files
            if !batch.is_empty() {
                if let Err(e) = db.insert_files_transaction(&batch, scan_id) {
                    debug!("Final batch insert error: {}", e);
                }
                total_inserted += batch.len() as u64;
            }

            info!("Inserted {} files into database", total_inserted);
        });

        // Stop progress reporter BEFORE starting FTS indexing
        // (otherwise it keeps emitting events with indexing: None that overwrite our progress)
        if let Some((thread, is_running)) = progress_handle {
            is_running.store(false, Ordering::Relaxed);
            let _ = thread.join();
        }

        // Rebuild FTS index after all inserts with progress tracking
        // Use atomic counter + separate thread for progress (like during scanning)
        // This prevents emit() from blocking the fast FTS indexing
        info!("Rebuilding search index...");
        let total_files = self.files_scanned.load(Ordering::SeqCst);
        let total_size = self.total_size.load(Ordering::SeqCst);
        let drives_progress = self.build_drives_progress();
        let indexing_start = Instant::now();

        // Atomic counter for progress (updated by FTS, read by progress thread)
        let indexed_counter = Arc::new(AtomicU64::new(0));
        let indexing_done = Arc::new(AtomicBool::new(false));

        // Spawn progress reporter thread (separate from FTS indexing)
        let progress_handle = if let Some(handle) = app_handle {
            let handle = handle.clone();
            let drives_clone = drives_progress.clone();
            let counter = Arc::clone(&indexed_counter);
            let done_flag = Arc::clone(&indexing_done);

            info!("FTS progress thread: Starting (total_files={})", total_files);

            let progress_thread = thread::spawn(move || {
                let mut emit_count = 0u64;
                let mut last_indexed = 0u64;

                while !done_flag.load(Ordering::Relaxed) {
                    let indexed = counter.load(Ordering::Relaxed);
                    let elapsed = indexing_start.elapsed().as_secs_f64();
                    let fps = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };
                    let percent = if total_files > 0 {
                        (indexed as f64 / total_files as f64) * 100.0
                    } else {
                        0.0
                    };

                    emit_count += 1;

                    // Log every 4 emits (1 second) or when progress changes significantly
                    if emit_count <= 4 || emit_count % 4 == 0 || indexed != last_indexed {
                        info!(
                            "FTS progress thread emit #{}: indexed={}/{} ({:.1}%) at {:.0}/s",
                            emit_count, indexed, total_files, percent, fps
                        );
                    }
                    last_indexed = indexed;

                    let progress = ScanProgress {
                        scan_id,
                        files_scanned: total_files,
                        total_size,
                        current_path: format!("Building search index... {:.0}%", percent),
                        files_per_second: 0.0,
                        is_complete: false,
                        error: None,
                        drives: drives_clone.clone(),
                        indexing: Some(super::IndexingProgress {
                            files_indexed: indexed,
                            total_files,
                            percent,
                            files_per_second: fps,
                        }),
                    };
                    let _ = handle.emit("scan-progress", &progress);

                    // Update every 250ms for responsive UI without blocking FTS
                    thread::sleep(Duration::from_millis(250));
                }

                info!("FTS progress thread: Stopped after {} emits", emit_count);
            });

            Some(progress_thread)
        } else {
            warn!("FTS progress thread: No app_handle, progress won't be emitted!");
            None
        };

        // Run FTS indexing (fast, non-blocking callback just updates counter)
        let counter_for_callback = Arc::clone(&indexed_counter);
        let fts_result = db.rebuild_fts_index_with_progress(|indexed, _total| {
            counter_for_callback.store(indexed, Ordering::Relaxed);
        });

        // Stop progress thread
        indexing_done.store(true, Ordering::Relaxed);
        if let Some(thread) = progress_handle {
            let _ = thread.join();
        }

        // Handle FTS indexing result
        if let Err(e) = fts_result {
            let error_msg = format!("FTS indexing failed: {}", e);
            warn!("{}", error_msg);

            // Emit error to frontend
            if let Some(handle) = app_handle {
                let error_progress = ScanProgress {
                    scan_id,
                    files_scanned: total_files,
                    total_size,
                    current_path: error_msg.clone(),
                    files_per_second: 0.0,
                    is_complete: true,
                    error: Some(error_msg),
                    drives: drives_progress.clone(),
                    indexing: None,
                };
                let _ = handle.emit("scan-progress", &error_progress);
                let _ = handle.emit("scan-error", e.to_string());
            }
        } else {
            info!("FTS indexing completed successfully");
            // Emit final 100% progress
            if let Some(handle) = app_handle {
                let elapsed = indexing_start.elapsed().as_secs_f64();
                let fps = if elapsed > 0.0 { total_files as f64 / elapsed } else { 0.0 };
                let final_progress = ScanProgress {
                    scan_id,
                    files_scanned: total_files,
                    total_size,
                    current_path: "Search index complete".to_string(),
                    files_per_second: 0.0,
                    is_complete: false,
                    error: None,
                    drives: drives_progress.clone(),
                    indexing: Some(super::IndexingProgress {
                        files_indexed: total_files,
                        total_files,
                        percent: 100.0,
                        files_per_second: fps,
                    }),
                };
                let _ = handle.emit("scan-progress", &final_progress);
            }
        }

        // Get final stats
        let total_files = self.files_scanned.load(Ordering::SeqCst);
        let total_size = self.total_size.load(Ordering::SeqCst);
        let elapsed = start_time.elapsed();
        let files_per_second = if elapsed.as_secs_f64() > 0.0 {
            total_files as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        info!(
            "Scan complete: {} files, {} bytes in {:.2}s ({:.0} files/sec)",
            total_files,
            total_size,
            elapsed.as_secs_f64(),
            files_per_second
        );

        // Optimize database after large insert
        if total_files > 10000 {
            info!("Optimizing database...");
            let _ = db.optimize();
        }

        // Update scan status
        db.update_scan_status(
            scan_id,
            "completed",
            Some(total_files as i64),
            Some(total_size as i64),
        )?;

        let final_progress = ScanProgress {
            scan_id,
            files_scanned: total_files,
            total_size,
            current_path: String::new(),
            files_per_second,
            is_complete: true,
            error: None,
            drives: self.build_drives_progress(),
            indexing: None,
        };

        // Emit completion
        if let Some(handle) = app_handle {
            let _ = handle.emit("scan-progress", &final_progress);
            let _ = handle.emit("scan-complete", &final_progress);
        }

        Ok(final_progress)
    }

    /// Legacy scan method (without events)
    pub fn scan(&self, db: &Database, scan_id: i64) -> Result<ScanProgress> {
        self.scan_with_events(db, scan_id, None)
    }

    /// Incremental scan - only process changed/new/deleted files
    pub fn scan_incremental_with_events(
        &self,
        db: &Database,
        scan_id: i64,
        app_handle: Option<&AppHandle>,
    ) -> Result<super::IncrementalProgress> {
        use std::collections::HashSet;

        info!("Starting incremental scan for {:?}", self.config.root_paths);
        let start_time = Instant::now();

        // Phase 1: Load existing files from DB
        let mut incremental = super::IncrementalProgress {
            phase: "preparing".to_string(),
            ..Default::default()
        };

        if let Some(handle) = app_handle {
            let _ = handle.emit("incremental-progress", &incremental);
        }

        // Get existing files for this drive
        let drive = self.config.root_paths.first().cloned().unwrap_or_default();
        let existing_files = db.get_drive_files_map(&drive)?;
        incremental.total_files_in_db = existing_files.len() as u64;

        info!("Loaded {} existing files from database", existing_files.len());

        // Track which files we've seen
        let mut seen_paths: HashSet<String> = HashSet::with_capacity(existing_files.len());

        // Phase 2: Scanning
        incremental.phase = "scanning".to_string();
        if let Some(handle) = app_handle {
            let _ = handle.emit("incremental-progress", &incremental);
        }

        let mut files_new: Vec<super::FileMetadata> = Vec::new();
        let mut files_updated: Vec<super::FileMetadata> = Vec::new();
        let batch_size = 1000;

        // Walk the filesystem
        for root_path in &self.config.root_paths {
            let walker = JWalkDir::new(root_path)
                .skip_hidden(self.config.exclude_hidden)
                .parallelism(jwalk::Parallelism::RayonNewPool(self.config.num_threads));

            let exclude_hidden = self.config.exclude_hidden;
            let exclude_system = self.config.exclude_system;

            for entry in walker
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                let path = entry.path();
                let path_str = path.to_string_lossy().to_string();
                let path_upper = path_str.to_uppercase();

                // Get file metadata
                let metadata = match fs::metadata(&path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                if !metadata.is_file() {
                    continue;
                }

                // Check Windows file attributes
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    let attrs = metadata.file_attributes();
                    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
                    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

                    if exclude_hidden && (attrs & FILE_ATTRIBUTE_HIDDEN) != 0 {
                        continue;
                    }
                    if exclude_system && (attrs & FILE_ATTRIBUTE_SYSTEM) != 0 {
                        continue;
                    }
                }

                let size = metadata.len() as i64;
                if size < self.config.min_file_size as i64 {
                    continue;
                }

                incremental.files_checked += 1;
                seen_paths.insert(path_upper.clone());

                let modified_at = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64);

                // Check if file exists in DB
                if let Some(&existing_mtime) = existing_files.get(&path_upper) {
                    // File exists - check if mtime changed
                    if existing_mtime == modified_at {
                        // Unchanged - skip
                        incremental.files_unchanged += 1;
                    } else {
                        // Changed - update
                        incremental.files_updated += 1;
                        files_updated.push(self.create_file_metadata(&path, &metadata));
                    }
                } else {
                    // New file
                    incremental.files_new += 1;
                    files_new.push(self.create_file_metadata(&path, &metadata));
                }

                // Emit progress every 1000 files
                if incremental.files_checked % 1000 == 0 {
                    let total_estimate = (existing_files.len() as f64 * 1.1) as u64;
                    incremental.percent = (incremental.files_checked as f64 / total_estimate.max(1) as f64) * 100.0;
                    incremental.percent = incremental.percent.min(99.0);

                    if let Some(handle) = app_handle {
                        let _ = handle.emit("incremental-progress", &incremental);

                        // Also emit scan-progress for frontend compatibility
                        let scan_progress = super::ScanProgress {
                            scan_id: scan_id,
                            files_scanned: incremental.files_checked,
                            total_size: 0, // Not tracked in incremental
                            current_path: format!("Incremental: {} checked, {} new, {} updated",
                                incremental.files_checked, incremental.files_new, incremental.files_updated),
                            files_per_second: 0.0,
                            is_complete: false,
                            error: None,
                            drives: std::collections::HashMap::new(),
                            indexing: None,
                        };
                        let _ = handle.emit("scan-progress", &scan_progress);
                    }
                }

                // Batch insert new files
                if files_new.len() >= batch_size {
                    db.insert_files_transaction(&files_new, scan_id)?;
                    files_new.clear();
                }

                // Batch update changed files
                if files_updated.len() >= batch_size {
                    for file in &files_updated {
                        let _ = db.update_file(file, scan_id);
                    }
                    files_updated.clear();
                }
            }
        }

        // Insert remaining new files
        if !files_new.is_empty() {
            db.insert_files_transaction(&files_new, scan_id)?;
        }

        // Update remaining changed files
        for file in &files_updated {
            let _ = db.update_file(file, scan_id);
        }

        // Phase 3: Cleanup - find deleted files
        incremental.phase = "cleaning".to_string();
        if let Some(handle) = app_handle {
            let _ = handle.emit("incremental-progress", &incremental);
        }

        let deleted_paths: Vec<String> = existing_files
            .keys()
            .filter(|path| !seen_paths.contains(*path))
            .cloned()
            .collect();

        incremental.files_deleted = deleted_paths.len() as u64;

        if !deleted_paths.is_empty() {
            info!("Deleting {} removed files from database", deleted_paths.len());
            db.delete_files_by_paths(&deleted_paths)?;
        }

        // Phase 4: Rebuild FTS if any changes
        let has_changes = incremental.files_new > 0
            || incremental.files_updated > 0
            || incremental.files_deleted > 0;

        if has_changes {
            incremental.phase = "indexing".to_string();
            if let Some(handle) = app_handle {
                let _ = handle.emit("incremental-progress", &incremental);
            }

            info!("Rebuilding FTS index after incremental changes...");
            if let Err(e) = db.rebuild_fts_index() {
                warn!("Failed to rebuild FTS index: {}", e);
            }
        }

        // Complete
        incremental.phase = "complete".to_string();
        incremental.percent = 100.0;

        let elapsed = start_time.elapsed();
        info!(
            "Incremental scan completed in {:.2}s: {} checked, {} unchanged, {} new, {} updated, {} deleted",
            elapsed.as_secs_f64(),
            incremental.files_checked,
            incremental.files_unchanged,
            incremental.files_new,
            incremental.files_updated,
            incremental.files_deleted
        );

        if let Some(handle) = app_handle {
            let _ = handle.emit("incremental-progress", &incremental);
            let _ = handle.emit("incremental-complete", &incremental);

            // Also emit scan-progress and scan-complete for frontend compatibility
            let final_scan_progress = super::ScanProgress {
                scan_id: scan_id,
                files_scanned: incremental.files_checked,
                total_size: 0,
                current_path: format!("Complete: {} checked, {} unchanged, {} new, {} updated, {} deleted",
                    incremental.files_checked, incremental.files_unchanged,
                    incremental.files_new, incremental.files_updated, incremental.files_deleted),
                files_per_second: 0.0,
                is_complete: true,
                error: None,
                drives: std::collections::HashMap::new(),
                indexing: None,
            };
            let _ = handle.emit("scan-progress", &final_scan_progress);
            let _ = handle.emit("scan-complete", &final_scan_progress);
        }

        Ok(incremental)
    }

    /// Create FileMetadata from path and fs::Metadata
    fn create_file_metadata(&self, path: &Path, metadata: &fs::Metadata) -> super::FileMetadata {
        let name = path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let extension = path.extension()
            .map(|e| e.to_string_lossy().to_string());

        let size = metadata.len() as i64;

        let created_at = metadata.created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        let modified_at = metadata.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        let accessed_at = metadata.accessed()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        #[cfg(windows)]
        let attributes = {
            use std::os::windows::fs::MetadataExt;
            Some(metadata.file_attributes())
        };

        #[cfg(not(windows))]
        let attributes = None;

        super::FileMetadata {
            path: path.to_string_lossy().to_string(),
            name,
            extension,
            size,
            created_at,
            modified_at,
            accessed_at,
            attributes,
            partial_hash: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_config_default() {
        let config = ScanConfig::default();
        assert!(config.exclude_hidden);
        assert!(config.exclude_system);
        assert!(config.num_threads >= 1);
    }
}
