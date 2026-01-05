//! Application statistics commands

use crate::database::ScanInfo;
use crate::AppState;
use serde::Serialize;
use tauri::State;

/// Application statistics
#[derive(Debug, Serialize)]
pub struct AppStats {
    pub total_files: i64,
    pub total_size: i64,
    pub duplicate_count: i64,
    pub last_scan: Option<ScanInfo>,
}

/// Get application statistics
#[tauri::command]
pub async fn get_stats(state: State<'_, AppState>) -> Result<AppStats, String> {
    let db = state.db.lock().await;

    let total_files = db.get_file_count().map_err(|e| e.to_string())?;
    let total_size = db.get_total_size().map_err(|e| e.to_string())?;
    let duplicate_count = db.get_duplicate_count().map_err(|e| e.to_string())?;
    let last_scan = db
        .get_recent_scans(1)
        .map_err(|e| e.to_string())?
        .into_iter()
        .next();

    Ok(AppStats {
        total_files,
        total_size,
        duplicate_count,
        last_scan,
    })
}

/// Get recent scans
#[tauri::command]
pub async fn get_recent_scans(state: State<'_, AppState>) -> Result<Vec<ScanInfo>, String> {
    let db = state.db.lock().await;
    db.get_recent_scans(10).map_err(|e| e.to_string())
}
