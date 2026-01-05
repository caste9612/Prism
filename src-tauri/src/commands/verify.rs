//! Cross-disk file verification commands
//!
//! Verifies if all files from a source drive exist on target drives.
//! Useful for backup validation and migration verification.

use crate::AppState;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::{Emitter, State};
use tracing::info;

/// Verification mode for comparing files
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerificationMode {
    /// Compare by file hash (exact content match)
    Hash,
    /// Compare by name and size (faster, less accurate)
    NameSize,
    /// Compare by relative path and size
    PathSize,
}

/// Result of verifying a single file
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVerificationResult {
    /// Path on source drive
    pub source_path: String,
    /// File name
    pub name: String,
    /// File size
    pub size: i64,
    /// Whether the file was found on target drives
    pub found: bool,
    /// Which target drive(s) have this file (if found)
    pub found_on: Vec<String>,
    /// Matched path on target (if found)
    pub matched_path: Option<String>,
}

/// Progress update for verification
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationProgress {
    /// Current phase: "loading_source", "loading_targets", "comparing", "complete"
    pub phase: String,
    /// Files processed so far
    pub files_processed: i64,
    /// Total files to process
    pub total_files: i64,
    /// Progress percentage (0-100)
    pub percentage: f64,
    /// Current status message
    pub message: String,
}

/// Summary of cross-disk verification
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationSummary {
    /// Source drive path
    pub source_drive: String,
    /// Target drives checked
    pub target_drives: Vec<String>,
    /// Total files on source
    pub total_files: i64,
    /// Files found on targets
    pub files_found: i64,
    /// Files missing from targets
    pub files_missing: i64,
    /// Percentage of files backed up
    pub backup_percentage: f64,
    /// Total size of source files
    pub total_size: i64,
    /// Size of files found on targets
    pub size_found: i64,
    /// Size of missing files
    pub size_missing: i64,
    /// List of missing files (limited to first N)
    pub missing_files: Vec<FileVerificationResult>,
    /// Verification time in milliseconds
    pub verification_time_ms: u64,
}

/// Verify if all files from source drive exist on target drives
///
/// This optimized version loads all target files into memory for O(1) lookups
/// instead of querying the database for each file.
#[tauri::command]
pub async fn verify_cross_disk(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    source_drive: String,
    target_drives: Vec<String>,
    max_missing_files: Option<i64>,
) -> Result<VerificationSummary, String> {
    // Check if a scan is currently running
    if state.is_scanning.load(Ordering::SeqCst) {
        return Err("Cannot verify while a scan is in progress. Please wait for the scan to complete.".to_string());
    }

    let start = std::time::Instant::now();
    let max_missing = max_missing_files.unwrap_or(100);

    info!(
        "Starting cross-disk verification: {} -> {:?}",
        source_drive, target_drives
    );

    if target_drives.is_empty() {
        return Err("At least one target drive is required".to_string());
    }

    // Emit initial progress
    let _ = app.emit("verify-progress", VerificationProgress {
        phase: "loading_source".to_string(),
        files_processed: 0,
        total_files: 0,
        percentage: 0.0,
        message: "Loading source files...".to_string(),
    });

    let db = state.db.lock().await;
    let conn = db.connection();

    // Normalize source drive path
    let source_pattern = if source_drive.ends_with('\\') {
        format!("{}%", source_drive.to_uppercase())
    } else {
        format!("{}\\%", source_drive.to_uppercase())
    };

    // Get all files from source drive with their hashes
    let mut source_stmt = conn
        .prepare(
            "SELECT path, name, size, partial_hash
             FROM files
             WHERE UPPER(path) LIKE ?1
             ORDER BY size DESC",
        )
        .map_err(|e| format!("Failed to prepare source query: {}", e))?;

    let source_files: Vec<(String, String, i64, Option<Vec<u8>>)> = source_stmt
        .query_map([&source_pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<Vec<u8>>>(3)?,
            ))
        })
        .map_err(|e| format!("Failed to query source files: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    let total_files = source_files.len() as i64;
    let total_size: i64 = source_files.iter().map(|(_, _, size, _)| size).sum();

    info!("Found {} files on source drive ({} bytes)", total_files, total_size);

    // Emit progress after loading source
    let _ = app.emit("verify-progress", VerificationProgress {
        phase: "loading_targets".to_string(),
        files_processed: 0,
        total_files,
        percentage: 5.0,
        message: format!("Loaded {} source files, loading target files...", total_files),
    });

    // Build target drive patterns
    let target_patterns: Vec<String> = target_drives
        .iter()
        .map(|d| {
            if d.ends_with('\\') {
                format!("{}%", d.to_uppercase())
            } else {
                format!("{}\\%", d.to_uppercase())
            }
        })
        .collect();

    // OPTIMIZATION: Load ALL target files into memory for O(1) lookups
    // HashMap by hash -> (drive_index, path)
    let mut hash_map: HashMap<(i64, Vec<u8>), (usize, String)> = HashMap::new();
    // HashMap by (name, size) -> (drive_index, path)
    let mut name_size_map: HashMap<(String, i64), (usize, String)> = HashMap::new();

    for (drive_idx, pattern) in target_patterns.iter().enumerate() {
        let mut target_stmt = conn
            .prepare(
                "SELECT path, name, size, partial_hash
                 FROM files
                 WHERE UPPER(path) LIKE ?1",
            )
            .map_err(|e| format!("Failed to prepare target query: {}", e))?;

        let target_files = target_stmt
            .query_map([pattern], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<Vec<u8>>>(3)?,
                ))
            })
            .map_err(|e| format!("Failed to query target files: {}", e))?;

        for file in target_files.filter_map(|r| r.ok()) {
            let (path, name, size, hash) = file;

            // Index by hash if available
            if let Some(h) = hash {
                hash_map.entry((size, h)).or_insert((drive_idx, path.clone()));
            }

            // Index by name + size
            name_size_map.entry((name.to_lowercase(), size)).or_insert((drive_idx, path));
        }
    }

    info!(
        "Loaded {} hash entries and {} name+size entries from target drives",
        hash_map.len(),
        name_size_map.len()
    );

    // Emit progress after loading targets
    let _ = app.emit("verify-progress", VerificationProgress {
        phase: "comparing".to_string(),
        files_processed: 0,
        total_files,
        percentage: 20.0,
        message: "Comparing files...".to_string(),
    });

    // Now compare source files against the in-memory indexes
    let mut files_found = 0i64;
    let mut size_found = 0i64;
    let mut missing_files = Vec::new();
    let progress_interval = (total_files / 20).max(100); // Update every 5% or 100 files

    for (idx, (source_path, name, size, hash)) in source_files.iter().enumerate() {
        let mut found = false;
        let mut found_on = Vec::new();

        // First try to match by hash if available (most accurate)
        if let Some(ref h) = hash {
            if let Some((drive_idx, _path)) = hash_map.get(&(*size, h.clone())) {
                found = true;
                found_on.push(target_drives[*drive_idx].clone());
            }
        }

        // If not found by hash, try by name + size
        if !found {
            if let Some((drive_idx, _path)) = name_size_map.get(&(name.to_lowercase(), *size)) {
                found = true;
                found_on.push(target_drives[*drive_idx].clone());
            }
        }

        if found {
            files_found += 1;
            size_found += size;
        } else {
            // Only add to missing list if under limit
            if missing_files.len() < max_missing as usize {
                missing_files.push(FileVerificationResult {
                    source_path: source_path.clone(),
                    name: name.clone(),
                    size: *size,
                    found: false,
                    found_on: vec![],
                    matched_path: None,
                });
            }
        }

        // Emit progress periodically
        if idx as i64 % progress_interval == 0 || idx == source_files.len() - 1 {
            let percentage = 20.0 + (idx as f64 / total_files as f64) * 80.0;
            let _ = app.emit("verify-progress", VerificationProgress {
                phase: "comparing".to_string(),
                files_processed: idx as i64 + 1,
                total_files,
                percentage,
                message: format!("Comparing files... {}/{}", idx + 1, total_files),
            });
        }
    }

    let files_missing = total_files - files_found;
    let size_missing = total_size - size_found;
    let backup_percentage = if total_files > 0 {
        (files_found as f64 / total_files as f64) * 100.0
    } else {
        100.0
    };

    let verification_time_ms = start.elapsed().as_millis() as u64;

    info!(
        "Verification complete: {}/{} files found ({:.1}%), {} missing, took {}ms",
        files_found, total_files, backup_percentage, files_missing, verification_time_ms
    );

    // Emit completion
    let _ = app.emit("verify-progress", VerificationProgress {
        phase: "complete".to_string(),
        files_processed: total_files,
        total_files,
        percentage: 100.0,
        message: format!(
            "Complete: {:.1}% backed up ({} of {} files)",
            backup_percentage, files_found, total_files
        ),
    });

    Ok(VerificationSummary {
        source_drive,
        target_drives,
        total_files,
        files_found,
        files_missing,
        backup_percentage,
        total_size,
        size_found,
        size_missing,
        missing_files,
        verification_time_ms,
    })
}

/// Quick check to see what percentage of a drive is backed up
#[tauri::command]
pub async fn quick_backup_check(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    source_drive: String,
    target_drives: Vec<String>,
) -> Result<f64, String> {
    let result = verify_cross_disk(app, state, source_drive, target_drives, Some(0)).await?;
    Ok(result.backup_percentage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verification_result_serialization() {
        let result = FileVerificationResult {
            source_path: "C:\\test.txt".to_string(),
            name: "test.txt".to_string(),
            size: 1024,
            found: true,
            found_on: vec!["D:\\".to_string()],
            matched_path: Some("D:\\backup\\test.txt".to_string()),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"sourcePath\""));
        assert!(json.contains("\"foundOn\""));
    }
}
