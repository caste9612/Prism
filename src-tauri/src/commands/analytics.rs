//! Analytics and distribution commands

use crate::AppState;
use serde::Serialize;
use tauri::State;
use tracing::{debug, info};

/// Size category for distribution
#[derive(Debug, Serialize)]
pub struct SizeCategory {
    pub name: String,
    pub count: i64,
    pub total_size: i64,
}

/// Extension category for distribution
#[derive(Debug, Serialize)]
pub struct ExtensionCategory {
    pub extension: String,
    pub count: i64,
    pub total_size: i64,
}

/// Get file size distribution for analytics
#[tauri::command]
pub async fn get_size_distribution(state: State<'_, AppState>) -> Result<Vec<SizeCategory>, String> {
    let db = state.db.lock().await;
    let conn = db.connection();

    let categories = vec![
        ("< 1 KB", 0i64, 1024i64),
        ("1 KB - 10 KB", 1024, 10 * 1024),
        ("10 KB - 100 KB", 10 * 1024, 100 * 1024),
        ("100 KB - 1 MB", 100 * 1024, 1024 * 1024),
        ("1 MB - 10 MB", 1024 * 1024, 10 * 1024 * 1024),
        ("10 MB - 100 MB", 10 * 1024 * 1024, 100 * 1024 * 1024),
        ("100 MB - 1 GB", 100 * 1024 * 1024, 1024 * 1024 * 1024),
        ("> 1 GB", 1024 * 1024 * 1024, i64::MAX),
    ];

    let mut result = Vec::new();

    for (name, min_size, max_size) in categories {
        let (count, total_size): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM files WHERE size >= ?1 AND size < ?2",
                rusqlite::params![min_size, max_size],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap_or((0, 0));

        result.push(SizeCategory {
            name: name.to_string(),
            count,
            total_size,
        });
    }

    Ok(result)
}

/// Get extension distribution for analytics
#[tauri::command]
pub async fn get_extension_distribution(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<ExtensionCategory>, String> {
    let db = state.db.lock().await;
    let conn = db.connection();
    let limit = limit.unwrap_or(20);

    let mut stmt = conn
        .prepare(
            "SELECT LOWER(COALESCE(extension, 'no extension')) as ext,
                COUNT(*) as cnt,
                SUM(size) as total
         FROM files
         GROUP BY ext
         ORDER BY total DESC
         LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([limit], |row| {
            Ok(ExtensionCategory {
                extension: row.get(0)?,
                count: row.get(1)?,
                total_size: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let result: Result<Vec<_>, _> = rows.collect();
    Ok(result.map_err(|e| e.to_string())?)
}

/// Extension detail within a category
#[derive(Debug, Serialize)]
pub struct ExtensionDetail {
    pub extension: String,
    pub count: i64,
    pub size: i64,
}

/// File type category with aggregated stats
#[derive(Debug, Serialize)]
pub struct FileTypeCategory {
    pub category: String,
    pub count: i64,
    pub total_size: i64,
    pub color: String,
    pub extensions: Vec<ExtensionDetail>,
}

/// Category definitions with their extensions
const FILE_CATEGORIES: &[(&str, &str, &[&str])] = &[
    ("Documents", "#3b82f6", &["pdf", "doc", "docx", "txt", "rtf", "odt", "xlsx", "xls", "pptx", "ppt", "csv", "md", "epub"]),
    ("Images", "#ec4899", &["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff", "tif", "raw", "psd", "ai", "heic", "heif"]),
    ("Videos", "#8b5cf6", &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpeg", "mpg", "3gp", "ts"]),
    ("Audio", "#22c55e", &["mp3", "wav", "flac", "aac", "ogg", "wma", "m4a", "opus", "aiff", "ape"]),
    ("Archives", "#f59e0b", &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "dmg", "cab"]),
    ("Code", "#06b6d4", &["js", "ts", "py", "rs", "go", "java", "c", "cpp", "h", "cs", "php", "rb", "swift", "kt", "scala", "vue", "jsx", "tsx", "html", "css", "scss", "sass", "less"]),
    ("Data", "#6366f1", &["json", "xml", "yaml", "yml", "sql", "db", "sqlite", "csv", "ini", "toml", "conf", "cfg"]),
];

/// Get file distribution grouped by type category
#[tauri::command]
pub async fn get_file_type_distribution(
    state: State<'_, AppState>,
) -> Result<Vec<FileTypeCategory>, String> {
    let db = state.db.lock().await;
    let conn = db.connection();

    // Get all extensions with counts and sizes
    let mut stmt = conn
        .prepare(
            "SELECT LOWER(COALESCE(extension, '')) as ext, COUNT(*) as cnt, SUM(size) as total
             FROM files
             GROUP BY ext
             ORDER BY total DESC",
        )
        .map_err(|e| e.to_string())?;

    let ext_data: Vec<(String, i64, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2).unwrap_or(0),
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Create category map
    let mut categories: Vec<FileTypeCategory> = FILE_CATEGORIES
        .iter()
        .map(|(name, color, _)| FileTypeCategory {
            category: name.to_string(),
            count: 0,
            total_size: 0,
            color: color.to_string(),
            extensions: Vec::new(),
        })
        .collect();

    // Add "Other" category
    categories.push(FileTypeCategory {
        category: "Other".to_string(),
        count: 0,
        total_size: 0,
        color: "#6b7280".to_string(),
        extensions: Vec::new(),
    });

    // Classify each extension
    for (ext, count, size) in ext_data {
        let mut found = false;

        for (i, (_, _, exts)) in FILE_CATEGORIES.iter().enumerate() {
            if exts.contains(&ext.as_str()) {
                categories[i].count += count;
                categories[i].total_size += size;
                categories[i].extensions.push(ExtensionDetail {
                    extension: ext.clone(),
                    count,
                    size,
                });
                found = true;
                break;
            }
        }

        if !found && !ext.is_empty() {
            let other_idx = categories.len() - 1;
            categories[other_idx].count += count;
            categories[other_idx].total_size += size;
            if categories[other_idx].extensions.len() < 10 {
                categories[other_idx].extensions.push(ExtensionDetail {
                    extension: ext,
                    count,
                    size,
                });
            }
        }
    }

    // Sort by total size descending
    categories.sort_by(|a, b| b.total_size.cmp(&a.total_size));

    // Sort extensions within each category by size
    for cat in &mut categories {
        cat.extensions.sort_by(|a, b| b.size.cmp(&a.size));
    }

    Ok(categories)
}

/// Folder item for treemap navigation
#[derive(Debug, Serialize)]
pub struct FolderItem {
    pub path: String,
    pub name: String,
    pub size: i64,
    pub file_count: i64,
    pub is_folder: bool,
}

/// Get folder contents for interactive treemap drill-down
/// Optimized to use SQL aggregation instead of in-memory processing
#[tauri::command]
pub async fn get_folder_contents(
    state: State<'_, AppState>,
    path: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<FolderItem>, String> {
    info!("get_folder_contents called with path: {:?}, limit: {:?}", path, limit);

    let db = state.db.lock().await;
    let conn = db.connection();
    let limit = limit.unwrap_or(20);

    if let Some(ref base_path) = path {
        // Normalize the base path
        let normalized_base = if base_path.ends_with('\\') || base_path.ends_with('/') {
            base_path.to_uppercase()
        } else {
            format!("{}\\", base_path.to_uppercase())
        };

        let pattern = format!("{}%", normalized_base);
        let base_len = normalized_base.len() as i64;

        // Use SQL to aggregate folder sizes directly
        // This extracts the immediate subfolder name and aggregates in one query
        let query = r#"
            WITH child_folders AS (
                SELECT
                    path,
                    size,
                    CASE
                        WHEN INSTR(SUBSTR(UPPER(path), ?2 + 1), '\') > 0
                        THEN SUBSTR(path, 1, ?2 + INSTR(SUBSTR(UPPER(path), ?2 + 1), '\') - 1)
                        ELSE NULL
                    END as folder_path
                FROM files
                WHERE UPPER(path) LIKE ?1 AND LENGTH(path) > ?2
            )
            SELECT
                folder_path,
                SUM(size) as total_size,
                COUNT(*) as file_count
            FROM child_folders
            WHERE folder_path IS NOT NULL
            GROUP BY UPPER(folder_path)
            ORDER BY total_size DESC
            LIMIT ?3
        "#;

        let mut stmt = conn.prepare_cached(query).map_err(|e| e.to_string())?;

        let items: Vec<FolderItem> = stmt
            .query_map(rusqlite::params![pattern, base_len, limit], |row| {
                let folder_path: String = row.get(0)?;
                let size: i64 = row.get(1)?;
                let count: i64 = row.get(2)?;

                let name = folder_path
                    .rsplit(|c| c == '\\' || c == '/')
                    .next()
                    .unwrap_or(&folder_path)
                    .to_string();

                Ok(FolderItem {
                    path: folder_path,
                    name,
                    size,
                    file_count: count,
                    is_folder: true,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        debug!(
            "Pattern: {}, base_len: {}, found {} folders using SQL aggregation",
            pattern, base_len, items.len()
        );

        info!("Returning {} folder items for path {:?}", items.len(), path);
        Ok(items)
    } else {
        // Root level: show drives
        let mut stmt = conn
            .prepare_cached(
                "SELECT UPPER(SUBSTR(path, 1, 2)) as drive, SUM(size) as total, COUNT(*) as cnt
                 FROM files
                 GROUP BY drive
                 ORDER BY total DESC",
            )
            .map_err(|e| e.to_string())?;

        let items: Vec<FolderItem> = stmt
            .query_map([], |row| {
                let drive: String = row.get(0)?;
                let size: i64 = row.get(1)?;
                let count: i64 = row.get(2)?;
                Ok(FolderItem {
                    path: format!("{}\\", drive),
                    name: drive,
                    size,
                    file_count: count,
                    is_folder: true,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        info!("Returning {} drives at root level", items.len());
        Ok(items)
    }
}

/// Folder size for treemap
#[derive(Debug, Serialize)]
pub struct FolderSize {
    pub path: String,
    pub name: String,
    pub size: i64,
    pub depth: i32,
}

/// Get folder sizes for treemap - optimized using SQL aggregation
#[tauri::command]
pub async fn get_folder_sizes(
    state: State<'_, AppState>,
    root_path: Option<String>,
    depth: Option<i32>,
) -> Result<Vec<FolderSize>, String> {
    let start = std::time::Instant::now();
    let db = state.db.lock().await;
    let conn = db.connection();
    let max_depth = depth.unwrap_or(2);

    // The max_depth parameter is reserved for future multi-level queries
    let _ = max_depth; // Mark as used

    // Fallback to simpler approach: get top-level folders using get_folder_contents logic
    // For treemap, we typically only need 1-2 levels at a time anyway
    let query = if let Some(ref root) = root_path {
        let normalized = if root.ends_with('\\') || root.ends_with('/') {
            root.to_uppercase()
        } else {
            format!("{}\\", root.to_uppercase())
        };
        let pattern = format!("{}%", normalized);
        let base_len = normalized.len() as i64;

        format!(
            r#"
            WITH immediate_folders AS (
                SELECT
                    path,
                    size,
                    CASE
                        WHEN INSTR(SUBSTR(UPPER(path), {} + 1), '\') > 0
                        THEN SUBSTR(path, 1, {} + INSTR(SUBSTR(UPPER(path), {} + 1), '\') - 1)
                        ELSE NULL
                    END as folder_path
                FROM files
                WHERE UPPER(path) LIKE '{}'
            )
            SELECT
                folder_path,
                SUM(size) as total_size,
                COUNT(*) as file_count
            FROM immediate_folders
            WHERE folder_path IS NOT NULL
            GROUP BY UPPER(folder_path)
            ORDER BY total_size DESC
            LIMIT 100
            "#,
            base_len, base_len, base_len, pattern
        )
    } else {
        // Root level: aggregate by drive
        r#"
            SELECT
                SUBSTR(path, 1, 3) as folder_path,
                SUM(size) as total_size,
                COUNT(*) as file_count
            FROM files
            GROUP BY UPPER(SUBSTR(path, 1, 3))
            ORDER BY total_size DESC
            LIMIT 20
        "#.to_string()
    };

    let mut stmt = conn.prepare_cached(&query).map_err(|e| e.to_string())?;

    let base_depth = if let Some(ref root) = root_path {
        root.matches(|c| c == '\\' || c == '/').count() as i32
    } else {
        0
    };

    let result: Vec<FolderSize> = stmt
        .query_map([], |row| {
            let path: String = row.get(0)?;
            let size: i64 = row.get(1)?;
            let name = path
                .rsplit(|c| c == '\\' || c == '/')
                .next()
                .unwrap_or(&path)
                .to_string();
            let depth = path.matches(|c| c == '\\' || c == '/').count() as i32 - base_depth;
            Ok(FolderSize {
                path,
                name,
                size,
                depth,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let elapsed = start.elapsed();
    info!(
        "get_folder_sizes completed in {:.2}ms, {} results",
        elapsed.as_secs_f64() * 1000.0,
        result.len()
    );

    Ok(result)
}
