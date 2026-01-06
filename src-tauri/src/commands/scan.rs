//! Scanning functionality commands

use crate::scanner::{IncrementalProgress, ScanConfig, ScanProgress, Scanner};
use crate::AppState;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};
use tracing::{error, info};

use super::drives::get_available_drives;

/// Scan request from frontend
#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    pub paths: Vec<String>,
    pub exclude_hidden: Option<bool>,
    pub exclude_system: Option<bool>,
    pub exclude_patterns: Option<Vec<String>>,
    pub min_file_size: Option<u64>,
}

/// Start a new filesystem scan with real-time progress events
#[tauri::command]
pub async fn start_scan(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    request: ScanRequest,
) -> Result<ScanProgress, String> {
    info!("Starting scan for paths: {:?}", request.paths);

    if request.paths.is_empty() {
        return Err("No paths specified for scanning".to_string());
    }

    // Create scan configuration with settings from frontend
    let mut config = ScanConfig {
        root_paths: request.paths.clone(),
        exclude_hidden: request.exclude_hidden.unwrap_or(true),
        exclude_system: request.exclude_system.unwrap_or(true),
        min_file_size: request.min_file_size.unwrap_or(0),
        ..Default::default()
    };

    // Override exclude patterns if provided
    if let Some(patterns) = request.exclude_patterns {
        config.exclude_patterns = patterns;
    }

    // Create scan record in database
    let db = state.db.lock().await;
    let scan_id = db
        .create_scan(&request.paths)
        .map_err(|e| format!("Failed to create scan: {}", e))?;
    drop(db); // Release lock before spawning

    // Initial progress
    let initial_progress = ScanProgress {
        scan_id,
        files_scanned: 0,
        total_size: 0,
        current_path: "Starting...".to_string(),
        files_per_second: 0.0,
        is_complete: false,
        error: None,
        drives: std::collections::HashMap::new(),
        indexing: None,
    };

    // Emit initial progress
    let _ = app_handle.emit("scan-progress", &initial_progress);

    // Clone for background task
    let db_clone = state.db.clone();
    let app_handle_clone = app_handle.clone();
    let is_scanning = state.is_scanning.clone();

    // Mark scan as starting
    is_scanning.store(true, Ordering::SeqCst);

    // Get drive info for known_drives (volume names and types)
    let drive_info: HashMap<String, (String, String)> = match get_available_drives().await {
        Ok(drives) => drives
            .into_iter()
            .map(|d| (d.path.to_uppercase(), (d.name, d.drive_type)))
            .collect(),
        Err(_) => HashMap::new(),
    };

    // Spawn the scan in a background task
    tokio::spawn(async move {
        let db = db_clone.lock().await;
        let scanner = Scanner::new(config);

        match scanner.scan_with_events(&db, scan_id, Some(&app_handle_clone)) {
            Ok(progress) => {
                info!("Scan completed successfully: {} files", progress.files_scanned);

                // Save known drives for offline detection
                for (drive_path, drive_progress) in &progress.drives {
                    let normalized_path = drive_path.to_uppercase();
                    let (volume_name, drive_type) = drive_info
                        .get(&normalized_path)
                        .cloned()
                        .unwrap_or((drive_path.clone(), "Unknown".to_string()));

                    if let Err(e) = db.upsert_known_drive(
                        drive_path,
                        Some(&volume_name),
                        &drive_type,
                        drive_progress.files_scanned as i64,
                        drive_progress.total_size as i64,
                    ) {
                        error!("Failed to save known drive {}: {}", drive_path, e);
                    } else {
                        info!(
                            "Saved known drive: {} ({} files, {} bytes)",
                            drive_path, drive_progress.files_scanned, drive_progress.total_size
                        );
                    }
                }

                // Optimize database after scan (full scan typically has many changes)
                let total_files: u64 = progress.drives.values()
                    .map(|d| d.files_scanned)
                    .sum();
                if let Err(e) = db.optimize_if_needed(total_files) {
                    error!("Failed to optimize database: {}", e);
                }

                // Emit stats update
                let _ = app_handle_clone.emit("stats-updated", ());
            }
            Err(e) => {
                error!("Scan failed: {}", e);
                let error_progress = ScanProgress {
                    scan_id,
                    files_scanned: 0,
                    total_size: 0,
                    current_path: String::new(),
                    files_per_second: 0.0,
                    is_complete: true,
                    error: Some(e.to_string()),
                    drives: std::collections::HashMap::new(),
                    indexing: None,
                };
                let _ = app_handle_clone.emit("scan-progress", error_progress);
                let _ = app_handle_clone.emit("scan-error", e.to_string());
            }
        }

        // Mark scan as complete
        is_scanning.store(false, Ordering::SeqCst);
    });

    Ok(initial_progress)
}

/// Auto-scan all drives on startup (local first, then network)
/// Uses smart scan: incremental for previously scanned drives, full for new ones
#[tauri::command]
pub async fn auto_scan_drives(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    info!("Starting auto-scan of all drives (smart mode)");

    // Get all ready drives
    let drives = get_available_drives().await?;

    // Separate local and network drives - scan local first to avoid network blocking
    let local_paths: Vec<String> = drives
        .iter()
        .filter(|d| d.is_ready && d.drive_type == "Local Disk")
        .map(|d| d.path.clone())
        .collect();

    let network_paths: Vec<String> = drives
        .iter()
        .filter(|d| d.is_ready && d.drive_type == "Network Drive")
        .map(|d| d.path.clone())
        .collect();

    if local_paths.is_empty() && network_paths.is_empty() {
        info!("No drives to scan");
        return Ok("No drives to scan".to_string());
    }

    let mut results = Vec::new();
    let has_network = !network_paths.is_empty();

    // Emit auto-scan phase: starting local drives
    if !local_paths.is_empty() {
        info!("Emitting auto-scan-phase: local (has_network: {})", has_network);
        let _ = app_handle.emit("auto-scan-phase", serde_json::json!({
            "phase": "local",
            "has_network": has_network,
            "paths": local_paths
        }));
    }

    // Scan local drives first (fast)
    if !local_paths.is_empty() {
        info!("Auto-scanning {} local drives: {:?}", local_paths.len(), local_paths);
        let local_request = ScanRequest {
            paths: local_paths.clone(),
            exclude_hidden: Some(true),
            exclude_system: Some(true),
            exclude_patterns: None,
            min_file_size: Some(0),
        };
        match start_smart_scan(app_handle.clone(), state.clone(), local_request).await {
            Ok(r) => results.push(format!("Local drives: {}", r)),
            Err(e) => error!("Local drive scan error: {}", e),
        }
    }

    // Emit auto-scan phase: starting network drives
    if !network_paths.is_empty() {
        info!("Emitting auto-scan-phase: network");
        let _ = app_handle.emit("auto-scan-phase", serde_json::json!({
            "phase": "network",
            "has_network": true,
            "paths": network_paths
        }));

        info!("Auto-scanning {} network drives: {:?}", network_paths.len(), network_paths);
        let network_request = ScanRequest {
            paths: network_paths.clone(),
            exclude_hidden: Some(true),
            exclude_system: Some(true),
            exclude_patterns: None,
            min_file_size: Some(0),
        };
        match start_smart_scan(app_handle.clone(), state, network_request).await {
            Ok(r) => results.push(format!("Network drives: {}", r)),
            Err(e) => error!("Network drive scan error: {}", e),
        }
    }

    // Emit auto-scan complete
    let _ = app_handle.emit("auto-scan-complete", serde_json::json!({
        "results": results.join("; ")
    }));

    Ok(results.join("; "))
}

/// Smart scan - automatically chooses between full scan and incremental scan
/// Uses incremental for previously scanned drives, full scan for new drives
#[tauri::command]
pub async fn start_smart_scan(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    request: ScanRequest,
) -> Result<String, String> {
    info!("Starting smart scan for paths: {:?}", request.paths);

    if request.paths.is_empty() {
        return Err("No paths specified for scanning".to_string());
    }

    // Check which drives need full scan vs incremental
    let db = state.db.lock().await;
    let mut full_scan_paths = Vec::new();
    let mut incremental_paths = Vec::new();

    for path in &request.paths {
        if db.is_drive_known(path).unwrap_or(false) {
            incremental_paths.push(path.clone());
        } else {
            full_scan_paths.push(path.clone());
        }
    }
    drop(db);

    let mut results = Vec::new();

    // Full scan for new drives
    if !full_scan_paths.is_empty() {
        info!("Full scan needed for new drives: {:?}", full_scan_paths);
        let full_request = ScanRequest {
            paths: full_scan_paths.clone(),
            exclude_hidden: request.exclude_hidden,
            exclude_system: request.exclude_system,
            exclude_patterns: request.exclude_patterns.clone(),
            min_file_size: request.min_file_size,
        };
        let _ = start_scan(app_handle.clone(), state.clone(), full_request).await;
        results.push(format!("Full scan: {:?}", full_scan_paths));
    }

    // Incremental scan for previously scanned drives (local and network)
    if !incremental_paths.is_empty() {
        info!("Incremental scan for known drives: {:?}", incremental_paths);
        let inc_request = ScanRequest {
            paths: incremental_paths.clone(),
            exclude_hidden: request.exclude_hidden,
            exclude_system: request.exclude_system,
            exclude_patterns: request.exclude_patterns.clone(),
            min_file_size: request.min_file_size,
        };
        match start_incremental_scan(app_handle, state, inc_request).await {
            Ok(progress) => {
                results.push(format!(
                    "Incremental: {} new, {} updated, {} deleted",
                    progress.files_new, progress.files_updated, progress.files_deleted
                ));
            }
            Err(e) => {
                results.push(format!("Incremental scan error: {}", e));
            }
        }
    }

    Ok(results.join("; "))
}

/// Start an incremental scan - only process changed/new/deleted files
#[tauri::command]
pub async fn start_incremental_scan(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    request: ScanRequest,
) -> Result<IncrementalProgress, String> {
    info!("Starting incremental scan for paths: {:?}", request.paths);

    if request.paths.is_empty() {
        return Err("No paths specified for incremental scan".to_string());
    }

    // Mark scan as starting
    state.is_scanning.store(true, Ordering::SeqCst);

    // Create scan configuration
    let config = ScanConfig {
        root_paths: request.paths.clone(),
        exclude_hidden: request.exclude_hidden.unwrap_or(true),
        exclude_system: request.exclude_system.unwrap_or(true),
        min_file_size: request.min_file_size.unwrap_or(0),
        ..Default::default()
    };

    // Create scan record in database
    let db = state.db.lock().await;
    let scan_id = db
        .create_scan(&request.paths)
        .map_err(|e| {
            state.is_scanning.store(false, Ordering::SeqCst);
            format!("Failed to create scan: {}", e)
        })?;

    // Run incremental scan
    let scanner = Scanner::new(config);
    let result = scanner
        .scan_incremental_with_events(&db, scan_id, Some(&app_handle))
        .map_err(|e| {
            state.is_scanning.store(false, Ordering::SeqCst);
            format!("Incremental scan failed: {}", e)
        })?;

    // Conditionally optimize database based on frequency settings
    let total_changes = result.files_new + result.files_updated + result.files_deleted;
    if total_changes > 0 {
        if let Err(e) = db.optimize_if_needed(total_changes) {
            error!("Failed to optimize database: {}", e);
        }
    }

    // Emit stats update
    let _ = app_handle.emit("stats-updated", ());

    // Mark scan as complete
    state.is_scanning.store(false, Ordering::SeqCst);

    info!(
        "Incremental scan completed: {} new, {} updated, {} deleted, {} unchanged",
        result.files_new, result.files_updated, result.files_deleted, result.files_unchanged
    );

    Ok(result)
}
