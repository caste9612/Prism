//! Search functionality commands

use crate::database::FileSearchResult;
use crate::search::{parse_query, SearchExecutor, SearchOptions, SearchResult, SortField, SortOrder};
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::debug;

/// Search request from frontend
#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<i64>,
}

/// Search files by name
#[tauri::command]
pub async fn search_files(
    state: State<'_, AppState>,
    request: SearchRequest,
) -> Result<Vec<FileSearchResult>, String> {
    let db = state.db.lock().await;
    let limit = request.limit.unwrap_or(100);

    db.search_files(&request.query, limit)
        .map_err(|e| e.to_string())
}

/// Advanced search request with filters
#[derive(Debug, Deserialize)]
pub struct AdvancedSearchRequest {
    pub query: String,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

/// Advanced search response with metadata
#[derive(Debug, Serialize)]
pub struct AdvancedSearchResponse {
    pub results: Vec<SearchResult>,
    pub total_count: i64,
    pub query_time_ms: u64,
}

/// Search files with advanced query syntax and filters
#[tauri::command]
pub async fn search_files_advanced(
    state: State<'_, AppState>,
    request: AdvancedSearchRequest,
) -> Result<AdvancedSearchResponse, String> {
    let start = std::time::Instant::now();
    debug!("Advanced search: {}", request.query);

    let parsed = parse_query(&request.query);
    debug!("Parsed query: {:?}", parsed);

    let options = SearchOptions {
        limit: request.limit.unwrap_or(100),
        offset: request.offset.unwrap_or(0),
        sort_by: match request.sort_by.as_deref() {
            Some("name") => SortField::Name,
            Some("size") => SortField::Size,
            Some("modified") => SortField::Modified,
            Some("path") => SortField::Path,
            _ => SortField::Relevance,
        },
        sort_order: match request.sort_order.as_deref() {
            Some("asc") => SortOrder::Asc,
            _ => SortOrder::Desc,
        },
    };

    let db = state.db.lock().await;
    let results = SearchExecutor::execute(&db, &parsed, &options)
        .map_err(|e| format!("Search failed: {}", e))?;

    let total_count = SearchExecutor::count(&db, &parsed).unwrap_or(results.len() as i64);

    let query_time_ms = start.elapsed().as_millis() as u64;
    debug!(
        "Search completed in {}ms, {} results",
        query_time_ms,
        results.len()
    );

    Ok(AdvancedSearchResponse {
        results,
        total_count,
        query_time_ms,
    })
}

/// Quick search result (minimal for performance)
#[derive(Debug, Serialize)]
pub struct QuickSearchResult {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub size: i64,
    pub modified_at: Option<i64>,
}

/// Quick search response
#[derive(Debug, Serialize)]
pub struct QuickSearchResponse {
    pub results: Vec<QuickSearchResult>,
    pub total: i64,
}

/// Ultra-fast search using FTS5 - optimized for instant results
/// Uses a separate read-only connection to not block during scans
#[tauri::command]
pub async fn quick_search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<i64>,
    extensions: Option<Vec<String>>,
) -> Result<QuickSearchResponse, String> {
    use rusqlite::{Connection, OpenFlags};

    if query.is_empty() {
        return Ok(QuickSearchResponse {
            results: vec![],
            total: 0,
        });
    }

    // Use a separate read-only connection for searches (doesn't block during scans)
    let db_path = state.db_path.clone();
    let limit = limit.unwrap_or(100);

    // Normalize extensions to lowercase
    let extensions: Option<Vec<String>> = extensions.map(|exts| {
        exts.into_iter()
            .map(|e| e.to_lowercase().trim_start_matches('.').to_string())
            .filter(|e| !e.is_empty())
            .collect()
    });

    // Run in blocking task to not block async runtime
    let result = tokio::task::spawn_blocking(move || {
        // Open read-only connection
        let conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ).map_err(|e| e.to_string())?;

        // Set pragmas for fast reads
        conn.execute_batch(
            "PRAGMA query_only = ON;
             PRAGMA cache_size = -16000;
             PRAGMA mmap_size = 134217728;"
        ).map_err(|e| e.to_string())?;

        // Use FTS5 for blazing fast full-text search
        // Escape special FTS5 characters to prevent query errors
        let fts_query = query
            .split_whitespace()
            .map(|word| {
                // Escape FTS5 special characters
                let escaped = word
                    .replace('"', "")
                    .replace('*', "")
                    .replace(':', "")
                    .replace('(', "")
                    .replace(')', "")
                    .replace('^', "")
                    .replace('-', " "); // Treat hyphen as space
                if escaped.trim().is_empty() {
                    String::new()
                } else {
                    format!("{}*", escaped.trim())
                }
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        // If query is empty after escaping, return empty results
        if fts_query.is_empty() {
            return Ok(QuickSearchResponse {
                results: vec![],
                total: 0,
            });
        }

        // Build extension filter clause if extensions are provided
        // Use numbered placeholders starting from ?2 for extensions
        let (ext_filter, ext_count) = if let Some(ref exts) = extensions {
            if exts.is_empty() {
                (String::new(), 0)
            } else {
                let placeholders: Vec<String> = exts.iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", i + 2)) // Start from ?2
                    .collect();
                (format!(" AND LOWER(f.extension) IN ({})", placeholders.join(", ")), exts.len())
            }
        } else {
            (String::new(), 0)
        };

        // Build count query - uses ?1 for FTS query, ?2..?N for extensions
        let count_sql = format!(
            "SELECT COUNT(*) FROM files_fts
             JOIN files f ON f.id = files_fts.rowid
             WHERE files_fts MATCH ?1{}",
            ext_filter
        );

        // Build results query - LIMIT uses the next placeholder after extensions
        let limit_placeholder = ext_count + 2; // ?1 is FTS, ?2..?N are extensions, next is limit
        let results_sql = format!(
            "SELECT f.id, f.path, f.name, f.extension, f.size, f.modified_at
             FROM files_fts
             JOIN files f ON f.id = files_fts.rowid
             WHERE files_fts MATCH ?1{}
             ORDER BY rank
             LIMIT ?{}",
            ext_filter,
            limit_placeholder
        );

        // Debug: log the query and extension filter
        if ext_count > 0 {
            debug!("Quick search with {} extension filters: {:?}", ext_count, extensions);
            debug!("SQL: {}", results_sql);
        }

        // Prepare parameters
        let mut count_params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(fts_query.clone())];
        let mut results_params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(fts_query.clone())];

        if let Some(ref exts) = extensions {
            for ext in exts {
                count_params.push(Box::new(ext.clone()));
                results_params.push(Box::new(ext.clone()));
            }
        }
        results_params.push(Box::new(limit));

        // Get total count
        let total: i64 = {
            let mut stmt = conn.prepare(&count_sql).map_err(|e| e.to_string())?;
            let params_refs: Vec<&dyn rusqlite::ToSql> = count_params.iter().map(|p| p.as_ref()).collect();
            stmt.query_row(params_refs.as_slice(), |row| row.get(0)).unwrap_or(0)
        };

        // Get results
        let mut stmt = conn.prepare(&results_sql).map_err(|e| e.to_string())?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = results_params.iter().map(|p| p.as_ref()).collect();

        let results: Vec<QuickSearchResult> = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(QuickSearchResult {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    extension: row.get(3)?,
                    size: row.get(4)?,
                    modified_at: row.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok::<_, String>(QuickSearchResponse { results, total })
    })
    .await
    .map_err(|e| e.to_string())?;

    result
}
