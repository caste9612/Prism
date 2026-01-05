//! Utility commands

use crate::AppState;
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

/// Open file location in system file explorer
#[tauri::command]
pub async fn open_in_explorer(path: String) -> Result<(), String> {
    info!("Opening in explorer: {}", path);

    let path_obj = std::path::Path::new(&path);

    // Validate path exists
    if !path_obj.exists() {
        return Err("Path does not exist".to_string());
    }

    let folder_path = if path_obj.is_file() {
        path_obj
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path.clone())
    } else {
        path.clone()
    };

    #[cfg(target_os = "windows")]
    {
        if path_obj.is_file() {
            std::process::Command::new("explorer")
                .args(["/select,", &path])
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
            .args(["-R", &path])
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
