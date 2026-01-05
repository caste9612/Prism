//! Scanning functionality commands

use crate::scanner::{IncrementalProgress, ScanConfig, ScanProgress, Scanner};
use crate::AppState;
use serde::Deserialize;
use std::collections::HashMap;
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

                // Optimize database after scan for better query performance
                if let Err(e) = db.optimize() {
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
    });

    Ok(initial_progress)
}

/// Auto-scan all drives on startup (local and network)
/// Uses smart scan: incremental for previously scanned drives, full for new ones
#[tauri::command]
pub async fn auto_scan_drives(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    info!("Starting auto-scan of all drives (smart mode)");

    // Get all ready drives (local and network)
    let drives = get_available_drives().await?;
    let paths: Vec<String> = drives
        .into_iter()
        .filter(|d| d.is_ready && (d.drive_type == "Local Disk" || d.drive_type == "Network Drive"))
        .map(|d| d.path)
        .collect();

    if paths.is_empty() {
        info!("No drives to scan");
        return Ok("No drives to scan".to_string());
    }

    info!("Auto-scanning {} drives: {:?}", paths.len(), paths);

    // Create scan request
    let request = ScanRequest {
        paths,
        exclude_hidden: Some(true),
        exclude_system: Some(true),
        exclude_patterns: None,
        min_file_size: Some(0),
    };

    // Use smart scan: incremental for known drives, full for new ones
    start_smart_scan(app_handle, state, request).await
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

    // Incremental scan for previously scanned drives
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
        .map_err(|e| format!("Failed to create scan: {}", e))?;

    // Run incremental scan
    let scanner = Scanner::new(config);
    let result = scanner
        .scan_incremental_with_events(&db, scan_id, Some(&app_handle))
        .map_err(|e| format!("Incremental scan failed: {}", e))?;

    // Optimize database if there were changes
    if result.files_new > 0 || result.files_updated > 0 || result.files_deleted > 0 {
        if let Err(e) = db.optimize() {
            error!("Failed to optimize database: {}", e);
        }
    }

    // Emit stats update
    let _ = app_handle.emit("stats-updated", ());

    info!(
        "Incremental scan completed: {} new, {} updated, {} deleted, {} unchanged",
        result.files_new, result.files_updated, result.files_deleted, result.files_unchanged
    );

    Ok(result)
}
