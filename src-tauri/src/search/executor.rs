//! Search query executor

use super::{ParsedQuery, QueryFilter, SearchOptions, SearchResult, SortField, SortOrder};
use super::parser::{SizeFilter, DateFilter};
use crate::database::Database;
use anyhow::Result;
use tracing::debug;

/// Executes search queries against the database
pub struct SearchExecutor;

impl SearchExecutor {
    /// Execute a parsed query and return results
    pub fn execute(
        db: &Database,
        query: &ParsedQuery,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        let conn = db.connection();

        // Build the SQL query
        let (sql, params) = Self::build_query(query, options);
        debug!("Executing search: {}", sql);

        let mut stmt = conn.prepare(&sql)?;

        // Execute and collect results
        let results = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
            Ok(SearchResult {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                extension: row.get(3)?,
                size: row.get(4)?,
                modified_at: row.get(5)?,
                rank: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                snippet: row.get(7).ok(),
            })
        })?;

        let mut search_results = Vec::new();
        for result in results {
            if let Ok(r) = result {
                search_results.push(r);
            }
        }

        Ok(search_results)
    }

    /// Build SQL query from parsed query
    fn build_query(query: &ParsedQuery, options: &SearchOptions) -> (String, Vec<String>) {
        let mut params: Vec<String> = Vec::new();
        let mut conditions: Vec<String> = Vec::new();

        // Use FTS5 if we have search text
        let use_fts = !query.text.trim().is_empty();

        let base_select = if use_fts {
            // FTS5 query with ranking
            r#"
            SELECT f.id, f.path, f.name, f.extension, f.size, f.modified_at,
                   bm25(files_fts) as rank,
                   snippet(files_fts, 0, '<mark>', '</mark>', '...', 32) as snippet
            FROM files_fts
            JOIN files f ON files_fts.rowid = f.id
            "#.to_string()
        } else {
            r#"
            SELECT f.id, f.path, f.name, f.extension, f.size, f.modified_at,
                   0.0 as rank,
                   NULL as snippet
            FROM files f
            "#.to_string()
        };

        // Add FTS match condition
        if use_fts {
            // Prepare FTS query - escape special characters and add prefix matching
            let fts_query = Self::prepare_fts_query(&query.text);
            conditions.push(format!("files_fts MATCH ?{}", params.len() + 1));
            params.push(fts_query);
        }

        // Add filter conditions
        for filter in &query.filters {
            match filter {
                QueryFilter::Size(size_filter) => {
                    match size_filter {
                        SizeFilter::GreaterThan(size) => {
                            conditions.push(format!("f.size > ?{}", params.len() + 1));
                            params.push(size.to_string());
                        }
                        SizeFilter::LessThan(size) => {
                            conditions.push(format!("f.size < ?{}", params.len() + 1));
                            params.push(size.to_string());
                        }
                        SizeFilter::Between(min, max) => {
                            conditions.push(format!("f.size BETWEEN ?{} AND ?{}", params.len() + 1, params.len() + 2));
                            params.push(min.to_string());
                            params.push(max.to_string());
                        }
                        SizeFilter::Equals(size) => {
                            conditions.push(format!("f.size = ?{}", params.len() + 1));
                            params.push(size.to_string());
                        }
                    }
                }
                QueryFilter::Extension(extensions) => {
                    if !extensions.is_empty() {
                        let placeholders: Vec<String> = extensions.iter()
                            .enumerate()
                            .map(|(i, _)| format!("?{}", params.len() + i + 1))
                            .collect();
                        conditions.push(format!("LOWER(f.extension) IN ({})", placeholders.join(", ")));
                        for ext in extensions {
                            params.push(ext.to_lowercase());
                        }
                    }
                }
                QueryFilter::Date(date_filter) => {
                    match date_filter {
                        DateFilter::After(ts) => {
                            conditions.push(format!("f.modified_at > ?{}", params.len() + 1));
                            params.push(ts.to_string());
                        }
                        DateFilter::Before(ts) => {
                            conditions.push(format!("f.modified_at < ?{}", params.len() + 1));
                            params.push(ts.to_string());
                        }
                        DateFilter::Between(start, end) => {
                            conditions.push(format!("f.modified_at BETWEEN ?{} AND ?{}", params.len() + 1, params.len() + 2));
                            params.push(start.to_string());
                            params.push(end.to_string());
                        }
                    }
                }
                QueryFilter::PathContains(path) => {
                    conditions.push(format!("f.path LIKE ?{}", params.len() + 1));
                    params.push(format!("%{}%", path));
                }
                QueryFilter::FileType(file_type) => {
                    let extensions = file_type.extensions();
                    if !extensions.is_empty() {
                        let placeholders: Vec<String> = extensions.iter()
                            .enumerate()
                            .map(|(i, _)| format!("?{}", params.len() + i + 1))
                            .collect();
                        conditions.push(format!("LOWER(f.extension) IN ({})", placeholders.join(", ")));
                        for ext in extensions {
                            params.push(ext.to_string());
                        }
                    }
                }
            }
        }

        // Build WHERE clause
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Build ORDER BY clause
        let order_by = match options.sort_by {
            SortField::Relevance if use_fts => "ORDER BY rank",
            SortField::Relevance => "ORDER BY f.modified_at DESC",
            SortField::Name => "ORDER BY f.name",
            SortField::Size => "ORDER BY f.size",
            SortField::Modified => "ORDER BY f.modified_at",
            SortField::Path => "ORDER BY f.path",
        };

        let order_direction = match options.sort_order {
            SortOrder::Asc => "ASC",
            SortOrder::Desc => "DESC",
        };

        // Build final query
        let sql = format!(
            "{} {} {} {} LIMIT {} OFFSET {}",
            base_select,
            where_clause,
            order_by,
            order_direction,
            options.limit,
            options.offset
        );

        (sql, params)
    }

    /// Prepare search text for FTS5 query
    fn prepare_fts_query(text: &str) -> String {
        // Split into words and add prefix matching with *
        let words: Vec<String> = text
            .split_whitespace()
            .map(|word| {
                // Escape FTS5 special characters
                let escaped = word
                    .replace('"', "\"\"")
                    .replace('*', "")
                    .replace(':', "");

                // Add prefix matching
                if escaped.len() >= 2 {
                    format!("{}*", escaped)
                } else {
                    escaped
                }
            })
            .filter(|w| !w.is_empty())
            .collect();

        words.join(" ")
    }

    /// Count total results for a query (without limit)
    pub fn count(db: &Database, query: &ParsedQuery) -> Result<i64> {
        let conn = db.connection();

        let use_fts = !query.text.trim().is_empty();
        let mut params: Vec<String> = Vec::new();
        let mut conditions: Vec<String> = Vec::new();

        if use_fts {
            let fts_query = Self::prepare_fts_query(&query.text);
            conditions.push("files_fts MATCH ?1".to_string());
            params.push(fts_query);
        }

        // Add same filter conditions as in build_query
        // (simplified for count)

        let sql = if use_fts {
            format!(
                "SELECT COUNT(*) FROM files_fts WHERE {}",
                if conditions.is_empty() { "1=1" } else { &conditions[0] }
            )
        } else {
            "SELECT COUNT(*) FROM files".to_string()
        };

        let count: i64 = conn.query_row(&sql, rusqlite::params_from_iter(params.iter()), |row| row.get(0))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::parse_query;

    #[test]
    fn test_prepare_fts_query() {
        assert_eq!(SearchExecutor::prepare_fts_query("hello world"), "hello* world*");
        assert_eq!(SearchExecutor::prepare_fts_query("test"), "test*");
    }
}
