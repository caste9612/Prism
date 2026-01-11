//! Export functionality commands

use crate::AppState;
use serde::Serialize;
use std::path::Path;
use tauri::State;
use tracing::info;

/// Export file for results
#[derive(Debug, Serialize)]
pub struct ExportFile {
    pub path: String,
    pub name: String,
    pub size: i64,
    pub extension: Option<String>,
    pub modified_at: Option<i64>,
}

/// Validate export output path for security
/// Returns the canonicalized path if valid, error otherwise
fn validate_export_path(output_path: &str) -> Result<std::path::PathBuf, String> {
    let path = Path::new(output_path);

    // Check parent directory exists and is writable
    let parent = path.parent().ok_or("Invalid output path: no parent directory")?;

    if !parent.exists() {
        return Err(format!("Output directory does not exist: {}", parent.display()));
    }

    // Canonicalize parent to resolve symlinks and prevent traversal
    let canonical_parent = std::fs::canonicalize(parent)
        .map_err(|e| format!("Failed to resolve output directory: {}", e))?;

    // Get filename and construct safe path
    let filename = path.file_name()
        .ok_or("Invalid output path: no filename")?;

    // Validate filename doesn't contain path separators (additional traversal protection)
    let filename_str = filename.to_string_lossy();
    if filename_str.contains('/') || filename_str.contains('\\') || filename_str.contains("..") {
        return Err("Invalid filename: contains path separators".to_string());
    }

    let safe_path = canonical_parent.join(filename);

    info!("Validated export path: {} -> {}", output_path, safe_path.display());
    Ok(safe_path)
}

/// Query files from database with optional filter
fn query_files(conn: &rusqlite::Connection, query: &Option<String>) -> Result<Vec<ExportFile>, String> {
    let sql = if query.is_some() {
        "SELECT path, name, size, extension, modified_at FROM files WHERE name LIKE ?1 OR path LIKE ?1 ORDER BY size DESC"
    } else {
        "SELECT path, name, size, extension, modified_at FROM files ORDER BY size DESC"
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let files: Vec<ExportFile> = if let Some(ref q) = query {
        let pattern = format!("%{}%", q);
        stmt.query_map([&pattern], |row| {
            Ok(ExportFile {
                path: row.get(0)?,
                name: row.get(1)?,
                size: row.get(2)?,
                extension: row.get(3)?,
                modified_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect()
    } else {
        stmt.query_map([], |row| {
            Ok(ExportFile {
                path: row.get(0)?,
                name: row.get(1)?,
                size: row.get(2)?,
                extension: row.get(3)?,
                modified_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect()
    };

    Ok(files)
}

/// Export search results to JSON
#[tauri::command]
pub async fn export_to_json(
    state: State<'_, AppState>,
    output_path: String,
    query: Option<String>,
) -> Result<u64, String> {
    info!("Exporting to JSON: {}", output_path);

    // Validate output path before any database operations
    let safe_path = validate_export_path(&output_path)?;

    let db = state.db.lock().await;
    let conn = db.get_connection().map_err(|e| e.to_string())?;

    let files = query_files(conn, &query)?;
    let count = files.len() as u64;

    let json = serde_json::to_string_pretty(&files).map_err(|e| e.to_string())?;
    std::fs::write(&safe_path, json).map_err(|e| e.to_string())?;

    info!("Exported {} files to JSON: {}", count, safe_path.display());
    Ok(count)
}

/// Export search results to CSV
#[tauri::command]
pub async fn export_to_csv(
    state: State<'_, AppState>,
    output_path: String,
    query: Option<String>,
) -> Result<u64, String> {
    info!("Exporting to CSV: {}", output_path);

    // Validate output path before any database operations
    let safe_path = validate_export_path(&output_path)?;

    let db = state.db.lock().await;
    let conn = db.get_connection().map_err(|e| e.to_string())?;

    let files = query_files(conn, &query)?;
    let count = files.len() as u64;

    // Pre-allocate string with estimated capacity for better performance
    let mut csv = String::with_capacity(count as usize * 100);
    csv.push_str("Path,Name,Size,Extension,Modified\n");

    for file in &files {
        let ext = file.extension.as_deref().unwrap_or("");
        let modified = file.modified_at.map(|t| t.to_string()).unwrap_or_default();
        let path_escaped = format!("\"{}\"", file.path.replace('"', "\"\""));
        let name_escaped = format!("\"{}\"", file.name.replace('"', "\"\""));
        csv.push_str(&format!(
            "{},{},{},{},{}\n",
            path_escaped, name_escaped, file.size, ext, modified
        ));
    }

    std::fs::write(&safe_path, csv).map_err(|e| e.to_string())?;

    info!("Exported {} files to CSV: {}", count, safe_path.display());
    Ok(count)
}
