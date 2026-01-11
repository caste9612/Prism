//! Utility commands

use crate::AppState;
use std::fs;
use tauri::{AppHandle, Emitter, State};
use tracing::info;

/// Clear all data from the database
#[tauri::command]
pub async fn clear_database(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    info!("Clearing database");
    let db = state.db.lock().await;
    db.clear_all_data().map_err(|e| e.to_string())?;

    // Emit stats update
    let _ = app_handle.emit("stats-updated", ());

    info!("Database cleared successfully");
    Ok(())
}

/// Full reset - clears database AND deletes all logs
/// Use this to simulate a completely fresh installation
#[tauri::command]
pub async fn reset_app(app_handle: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    info!("Full app reset requested");

    // 1. Clear database
    let db = state.db.lock().await;
    db.clear_all_data().map_err(|e| e.to_string())?;
    drop(db);
    info!("Database cleared");

    // 2. Delete log files
    let mut logs_deleted = 0;
    if let Some(local_data) = dirs::data_local_dir() {
        let log_dir = local_data.join("Prism").join("logs");
        if log_dir.exists() {
            if let Ok(entries) = fs::read_dir(&log_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() {
                        if let Ok(_) = fs::remove_file(&path) {
                            logs_deleted += 1;
                        }
                    }
                }
            }
        }
    }
    info!("Deleted {} log files", logs_deleted);

    // Emit stats update
    let _ = app_handle.emit("stats-updated", ());

    let message = format!("Reset complete: database cleared, {} log files deleted. Restart the app for a fresh start.", logs_deleted);
    info!("{}", message);
    Ok(message)
}

/// Open file location in system file explorer
#[tauri::command]
pub async fn open_in_explorer(path: String) -> Result<(), String> {
    info!("Opening in explorer: {}", path);

    let path_obj = std::path::Path::new(&path);

    // Validate path exists
    if !path_obj.exists() {
        return Err("Path does not exist".to_string());
    }

    // Canonicalize to resolve symlinks and get the real path
    // This prevents symlink traversal attacks
    let canonical_path = std::fs::canonicalize(path_obj)
        .map_err(|e| format!("Failed to resolve path: {}", e))?;

    // Log the canonical path for security audit
    if canonical_path.to_string_lossy() != path {
        info!("Path resolved: {} -> {}", path, canonical_path.display());
    }

    let canonical_str = canonical_path.to_string_lossy().to_string();

    let folder_path = if canonical_path.is_file() {
        canonical_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(canonical_str.clone())
    } else {
        canonical_str.clone()
    };

    #[cfg(target_os = "windows")]
    {
        if canonical_path.is_file() {
            std::process::Command::new("explorer")
                .args(["/select,", &canonical_str])
                .spawn()
                .map_err(|e| format!("Failed to open explorer: {}", e))?;
        } else {
            std::process::Command::new("explorer")
                .arg(&folder_path)
                .spawn()
                .map_err(|e| format!("Failed to open explorer: {}", e))?;
        }
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &canonical_str])
            .spawn()
            .map_err(|e| format!("Failed to open Finder: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {}", e))?;
    }

    Ok(())
}
