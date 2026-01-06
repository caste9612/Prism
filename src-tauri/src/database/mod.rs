//! Database module for Prism
//!
//! Handles all SQLite operations including schema management,
//! file indexing, and query execution.

mod schema;

pub use schema::SCHEMA_SQL;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use tracing::{debug, info};

use crate::scanner::FileMetadata;

/// Database wrapper for SQLite operations
pub struct Database {
    conn: Connection,
}

/// Ensure WAL checkpoint on drop to prevent corruption
impl Drop for Database {
    fn drop(&mut self) {
        // Checkpoint WAL to ensure all changes are written to main database file
        // This prevents corruption if the app crashes after this point
        if let Err(e) = self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
            // Can't use tracing here as it may be shut down
            eprintln!("[Database] WAL checkpoint on drop failed: {}", e);
        }
    }
}

impl Database {
    /// Create a new database connection
    pub fn new(path: &Path) -> Result<Self> {
        info!("Opening database at {:?}", path);
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrency (allows reads during writes)
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -64000;
             PRAGMA temp_store = MEMORY;
             PRAGMA mmap_size = 268435456;
             PRAGMA busy_timeout = 5000;",
        )?;

        // Set busy handler to retry on lock
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        // Skip integrity check on startup - it's too slow on large databases (18+ seconds)
        // The WAL mode and proper checkpointing provide sufficient protection
        // If corruption is suspected, user can clear the database manually
        debug!("Database opened successfully (integrity check skipped for performance)");

        Ok(Self { conn })
    }

    /// Get a reference to the underlying connection (for advanced queries)
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Initialize the database schema
    pub fn initialize_schema(&self) -> Result<()> {
        info!("Initializing database schema");
        self.conn
            .execute_batch(SCHEMA_SQL)
            .context("Failed to initialize schema")?;
        info!("Schema initialized successfully");
        Ok(())
    }

    /// Create a new scan session
    pub fn create_scan(&self, root_paths: &[String]) -> Result<i64> {
        let root_paths_json = serde_json::to_string(root_paths)?;
        let now = chrono::Utc::now().timestamp();

        self.conn.execute(
            "INSERT INTO scans (started_at, root_paths, status) VALUES (?1, ?2, 'running')",
            params![now, root_paths_json],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Update scan status
    pub fn update_scan_status(
        &self,
        scan_id: i64,
        status: &str,
        total_files: Option<i64>,
        total_size: Option<i64>,
    ) -> Result<()> {
        let now = if status == "completed" || status == "failed" {
            Some(chrono::Utc::now().timestamp())
        } else {
            None
        };

        self.conn.execute(
            "UPDATE scans SET status = ?1, completed_at = ?2, total_files = ?3, total_size = ?4 WHERE id = ?5",
            params![status, now, total_files, total_size, scan_id],
        )?;

        Ok(())
    }

    /// Insert a batch of files efficiently
    pub fn insert_files_batch(&self, files: &[FileMetadata], scan_id: i64) -> Result<usize> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO files (path, name, extension, size, created_at, modified_at, accessed_at, attributes, partial_hash, scan_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;

        let mut count = 0;
        for file in files {
            stmt.execute(params![
                file.path,
                file.name,
                file.extension,
                file.size,
                file.created_at,
                file.modified_at,
                file.accessed_at,
                file.attributes,
                file.partial_hash,
                scan_id,
            ])?;
            count += 1;
        }

        debug!("Inserted {} files", count);
        Ok(count)
    }

    /// Get total file count
    pub fn get_file_count(&self) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Get total size of all indexed files
    pub fn get_total_size(&self) -> Result<i64> {
        let size: i64 = self
            .conn
            .query_row("SELECT COALESCE(SUM(size), 0) FROM files", [], |row| {
                row.get(0)
            })?;
        Ok(size)
    }

    /// Get count of potential duplicates (files with same size and partial hash)
    pub fn get_duplicate_count(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM (
                SELECT partial_hash FROM files
                WHERE partial_hash IS NOT NULL
                GROUP BY size, partial_hash
                HAVING COUNT(*) > 1
            )",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Search files by name (simple substring search)
    pub fn search_files(&self, query: &str, limit: i64) -> Result<Vec<FileSearchResult>> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, path, name, extension, size, modified_at
             FROM files
             WHERE name LIKE ?1
             ORDER BY modified_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![pattern, limit], |row| {
            Ok(FileSearchResult {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                extension: row.get(3)?,
                size: row.get(4)?,
                modified_at: row.get(5)?,
            })
        })?;

        let results: Result<Vec<_>, _> = rows.collect();
        Ok(results?)
    }

    /// Get recent scans
    pub fn get_recent_scans(&self, limit: i64) -> Result<Vec<ScanInfo>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, started_at, completed_at, root_paths, total_files, total_size, status
             FROM scans
             ORDER BY started_at DESC
             LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            Ok(ScanInfo {
                id: row.get(0)?,
                started_at: row.get(1)?,
                completed_at: row.get(2)?,
                root_paths: row.get(3)?,
                total_files: row.get(4)?,
                total_size: row.get(5)?,
                status: row.get(6)?,
            })
        })?;

        let results: Result<Vec<_>, _> = rows.collect();
        Ok(results?)
    }

    /// Get the last scan that is still running
    pub fn get_running_scan(&self) -> Result<Option<ScanInfo>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, started_at, completed_at, root_paths, total_files, total_size, status
             FROM scans
             WHERE status = 'running'
             ORDER BY started_at DESC
             LIMIT 1",
                [],
                |row| {
                    Ok(ScanInfo {
                        id: row.get(0)?,
                        started_at: row.get(1)?,
                        completed_at: row.get(2)?,
                        root_paths: row.get(3)?,
                        total_files: row.get(4)?,
                        total_size: row.get(5)?,
                        status: row.get(6)?,
                    })
                },
            )
            .optional()?;

        Ok(result)
    }

    /// Get a reference to the connection for advanced queries
    pub fn get_connection(&self) -> Result<&Connection> {
        Ok(&self.conn)
    }

    /// Clear all data from the database
    pub fn clear_all_data(&self) -> Result<()> {
        info!("Clearing all data from database");
        self.conn.execute_batch(
            "DELETE FROM files;
             DELETE FROM files_fts;
             DELETE FROM folder_sizes;
             DELETE FROM scans;
             VACUUM;"
        )?;
        info!("Database cleared successfully");
        Ok(())
    }

    /// Delete files from specific drives before rescanning them
    /// Takes a list of drive paths (e.g., ["C:\\", "Z:\\"])
    pub fn clear_drives(&self, drive_paths: &[String]) -> Result<usize> {
        let mut total_deleted = 0;

        for drive_path in drive_paths {
            // Normalize the drive path to match file paths
            let pattern = if drive_path.ends_with('\\') || drive_path.ends_with('/') {
                format!("{}%", drive_path)
            } else {
                format!("{}\\%", drive_path)
            };

            // Also try uppercase pattern for case-insensitive matching
            let pattern_upper = pattern.to_uppercase();
            let pattern_lower = pattern.to_lowercase();

            let deleted = self.conn.execute(
                "DELETE FROM files WHERE path LIKE ?1 OR path LIKE ?2 OR path LIKE ?3",
                params![pattern, pattern_upper, pattern_lower],
            )?;

            info!("Cleared {} files from drive {}", deleted, drive_path);
            total_deleted += deleted;
        }

        // Also clear from FTS index
        if total_deleted > 0 {
            // Rebuild FTS to remove orphaned entries
            self.conn.execute("INSERT INTO files_fts(files_fts) VALUES('rebuild')", [])?;
        }

        info!("Total files cleared: {}", total_deleted);
        Ok(total_deleted)
    }

    /// Run database optimization (should be called periodically)
    pub fn optimize(&self) -> Result<()> {
        info!("Running database optimization");
        self.conn.execute_batch(
            "PRAGMA optimize;
             PRAGMA wal_checkpoint(TRUNCATE);
             ANALYZE;"
        )?;
        // Track when optimization was run
        self.set_setting("last_optimize_time", &chrono::Utc::now().timestamp().to_string())?;
        self.set_setting("scans_since_optimize", "0")?;
        info!("Database optimization complete");
        Ok(())
    }

    /// Check if optimization should run based on frequency settings
    /// Optimizes: every 10 scans OR every 24 hours OR if >50k total changes
    pub fn should_optimize(&self, total_changes: u64) -> bool {
        const MAX_SCANS_BETWEEN_OPTIMIZE: i64 = 10;
        const MAX_HOURS_BETWEEN_OPTIMIZE: i64 = 24;
        const LARGE_CHANGE_THRESHOLD: u64 = 50_000;

        // Large changes always trigger optimization
        if total_changes >= LARGE_CHANGE_THRESHOLD {
            info!("Optimization triggered: {} changes exceeds {} threshold",
                total_changes, LARGE_CHANGE_THRESHOLD);
            return true;
        }

        // Check scans since last optimize
        let scans_since: i64 = self.get_setting("scans_since_optimize")
            .unwrap_or_default()
            .parse()
            .unwrap_or(0);
        if scans_since >= MAX_SCANS_BETWEEN_OPTIMIZE {
            info!("Optimization triggered: {} scans since last optimize", scans_since);
            return true;
        }

        // Check time since last optimize
        if let Ok(last_optimize_str) = self.get_setting("last_optimize_time") {
            if let Ok(last_optimize) = last_optimize_str.parse::<i64>() {
                let now = chrono::Utc::now().timestamp();
                let hours_since = (now - last_optimize) / 3600;
                if hours_since >= MAX_HOURS_BETWEEN_OPTIMIZE {
                    info!("Optimization triggered: {} hours since last optimize", hours_since);
                    return true;
                }
            }
        } else {
            // Never optimized before
            info!("Optimization triggered: never optimized before");
            return true;
        }

        false
    }

    /// Conditionally run optimization based on frequency settings
    /// Increments scan counter even when skipping optimization
    pub fn optimize_if_needed(&self, total_changes: u64) -> Result<bool> {
        // Increment scans counter
        let scans_since: i64 = self.get_setting("scans_since_optimize")
            .unwrap_or_default()
            .parse()
            .unwrap_or(0);
        self.set_setting("scans_since_optimize", &(scans_since + 1).to_string())?;

        if self.should_optimize(total_changes) {
            self.optimize()?;
            Ok(true)
        } else {
            info!("Skipping database optimization (scan {}/{}, changes: {})",
                scans_since + 1, 10, total_changes);
            Ok(false)
        }
    }

    /// Get a setting value from the settings table
    fn get_setting(&self, key: &str) -> Result<String> {
        self.conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        ).map_err(Into::into)
    }

    /// Set a setting value in the settings table
    fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Delete a file by ID with path validation
    pub fn delete_file(&self, file_id: i64, expected_path: &str) -> Result<bool> {
        // First verify the path matches what's in the database (security check)
        let actual_path: Option<String> = self.conn.query_row(
            "SELECT path FROM files WHERE id = ?1",
            params![file_id],
            |row| row.get(0),
        ).optional()?;

        match actual_path {
            Some(path) if path == expected_path => {
                self.conn.execute(
                    "DELETE FROM files WHERE id = ?1",
                    params![file_id],
                )?;
                Ok(true)
            }
            Some(_) => {
                // Path mismatch - potential attack
                Err(anyhow::anyhow!("Path mismatch for file ID {}", file_id))
            }
            None => Ok(false), // File not found
        }
    }

    /// Delete multiple files in a transaction
    pub fn delete_files_batch(&self, files: &[(i64, String)]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut deleted = 0;

        for (file_id, expected_path) in files {
            let actual_path: Option<String> = tx.query_row(
                "SELECT path FROM files WHERE id = ?1",
                params![file_id],
                |row| row.get(0),
            ).optional()?;

            if let Some(path) = actual_path {
                if path == *expected_path {
                    tx.execute("DELETE FROM files WHERE id = ?1", params![file_id])?;
                    deleted += 1;
                }
            }
        }

        tx.commit()?;
        Ok(deleted)
    }

    /// Insert files in a transaction for better performance and atomicity
    /// FTS triggers are disabled - call rebuild_fts_index() after all inserts
    pub fn insert_files_transaction(&self, files: &[FileMetadata], scan_id: i64) -> Result<usize> {
        if files.is_empty() {
            return Ok(0);
        }

        let tx = self.conn.unchecked_transaction()?;

        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO files (path, name, extension, size, created_at, modified_at, accessed_at, attributes, partial_hash, scan_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;

            for file in files {
                stmt.execute(params![
                    file.path,
                    file.name,
                    file.extension,
                    file.size,
                    file.created_at,
                    file.modified_at,
                    file.accessed_at,
                    file.attributes,
                    file.partial_hash,
                    scan_id,
                ])?;
            }
        }

        tx.commit()?;
        Ok(files.len())
    }

    /// Disable FTS triggers before bulk scanning (call once at start)
    pub fn disable_fts_triggers(&self) -> Result<()> {
        info!("Disabling FTS triggers for bulk insert");
        self.conn.execute_batch(
            "DROP TRIGGER IF EXISTS files_ai;
             DROP TRIGGER IF EXISTS files_au;
             DROP TRIGGER IF EXISTS files_ad;"
        )?;
        Ok(())
    }

    /// Rebuild FTS index and re-enable triggers (call once after all inserts)
    /// This is the legacy method without progress reporting
    pub fn rebuild_fts_index(&self) -> Result<()> {
        self.rebuild_fts_index_with_progress(|_, _| {})
    }

    /// Rebuild FTS index with progress callback
    /// The callback receives (files_indexed, total_files)
    pub fn rebuild_fts_index_with_progress<F>(&self, mut progress_callback: F) -> Result<()>
    where
        F: FnMut(u64, u64),
    {
        eprintln!("[FTS] === REBUILD START ===");
        info!("=== FTS REBUILD START ===");
        let start = std::time::Instant::now();

        // Get total file count for progress calculation
        let total_files: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files",
            [],
            |row| row.get(0),
        )?;
        eprintln!("[FTS] Total files in database: {}", total_files);

        if total_files == 0 {
            eprintln!("[FTS] No files to index - skipping");
            info!("No files to index - skipping FTS rebuild");
            self.recreate_fts_triggers()?;
            return Ok(());
        }

        eprintln!("[FTS] Starting to index {} files...", total_files);
        info!("FTS: Starting to index {} files", total_files);

        // Store current journal mode to restore later
        let original_journal_mode: String = self.conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap_or_else(|_| "wal".to_string());
        debug!("Original journal_mode: {}", original_journal_mode);

        // FTS indexing optimization: Use NORMAL synchronous (not OFF!)
        // WAL mode provides good performance without corruption risk
        // Note: We do NOT change journal_mode to MEMORY - this causes corruption on crash!

        // Clear existing FTS data using proper FTS5 command
        // (DELETE FROM corrupts content-table FTS5)
        info!("Clearing existing FTS data...");
        self.conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", [])?;

        // Large batch size = maximum indexing speed (benchmarked: 500k+ files/sec)
        // Callback is now non-blocking (just atomic counter update), so call it every batch
        const FTS_BATCH_SIZE: i64 = 10_000;

        let mut indexed: u64 = 0;
        let mut last_id: i64 = 0;
        let log_interval = std::time::Instant::now();
        let mut last_log = std::time::Instant::now();
        let mut batch_count: u64 = 0;

        // Report initial progress
        info!("FTS: Calling initial progress callback (0/{})", total_files);
        progress_callback(0, total_files);

        loop {
            batch_count += 1;
            let batch_start = std::time::Instant::now();

            // Simple batch query - get next batch_size IDs
            let rows_affected = self.conn.execute(
                "INSERT INTO files_fts(rowid, name, path, extension)
                 SELECT id, name, path, extension FROM files
                 WHERE id > ?1
                 ORDER BY id
                 LIMIT ?2",
                params![last_id, FTS_BATCH_SIZE],
            )?;

            let batch_elapsed = batch_start.elapsed();

            if rows_affected == 0 {
                info!("FTS: Indexing complete after {} batches, last_id={}", batch_count, last_id);
                break;
            }

            // Get the actual last ID we inserted (max of the batch, not all files!)
            let new_last_id: i64 = self.conn.query_row(
                "SELECT MAX(id) FROM (SELECT id FROM files WHERE id > ?1 ORDER BY id LIMIT ?2)",
                params![last_id, FTS_BATCH_SIZE],
                |row| row.get(0),
            ).unwrap_or(last_id);

            last_id = new_last_id;
            indexed += rows_affected as u64;

            // Update progress counter (non-blocking, read by separate thread)
            progress_callback(indexed, total_files);

            // Log every batch for first 5, then every 2 seconds
            let should_log = batch_count <= 5 || last_log.elapsed().as_secs() >= 2;
            if should_log {
                let elapsed = log_interval.elapsed().as_secs_f64();
                let rate = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };
                let batch_rate = if batch_elapsed.as_secs_f64() > 0.0 {
                    rows_affected as f64 / batch_elapsed.as_secs_f64()
                } else {
                    0.0
                };
                let percent = (indexed as f64 / total_files as f64) * 100.0;
                eprintln!(
                    "[FTS] Batch #{}: {}/{} ({:.1}%) - {:.0} files/sec",
                    batch_count, indexed, total_files, percent, rate
                );
                info!(
                    "FTS batch #{}: {} rows in {:.3}s ({:.0}/s), total: {}/{} ({:.1}%) overall {:.0}/s",
                    batch_count, rows_affected, batch_elapsed.as_secs_f64(), batch_rate,
                    indexed, total_files, percent, rate
                );
                last_log = std::time::Instant::now();
            }
        }

        // Final progress update
        info!("FTS: Calling final progress callback ({}/{})", indexed, total_files);
        progress_callback(indexed, total_files);

        // No PRAGMA restore needed - we keep WAL mode throughout
        // This ensures database integrity even if app crashes during indexing
        info!("FTS: Indexing complete, running WAL checkpoint...");
        if let Err(e) = self.conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)") {
            debug!("WAL checkpoint: {}", e);
        }

        let elapsed = start.elapsed().as_secs_f64();
        let rate = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };
        eprintln!(
            "[FTS] === COMPLETE: {} files in {:.2}s ({:.0} files/sec) ===",
            indexed, elapsed, rate
        );
        info!(
            "=== FTS REBUILD COMPLETE: {} files in {:.2}s ({:.0} files/sec) ===",
            indexed, elapsed, rate
        );

        // Recreate triggers for future single-row operations
        self.recreate_fts_triggers()?;

        Ok(())
    }

    /// Recreate FTS triggers after bulk operations
    fn recreate_fts_triggers(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
                INSERT INTO files_fts(rowid, name, path, extension)
                VALUES (new.id, new.name, new.path, new.extension);
             END;
             CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
                INSERT INTO files_fts(files_fts, rowid, name, path, extension)
                VALUES ('delete', old.id, old.name, old.path, old.extension);
             END;
             CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE ON files BEGIN
                INSERT INTO files_fts(files_fts, rowid, name, path, extension)
                VALUES ('delete', old.id, old.name, old.path, old.extension);
                INSERT INTO files_fts(rowid, name, path, extension)
                VALUES (new.id, new.name, new.path, new.extension);
             END;"
        )?;
        info!("FTS triggers re-enabled");
        Ok(())
    }

    /// Ensure FTS triggers are in place (idempotent, safe to call anytime)
    /// Used for incremental scans where triggers should already exist
    pub fn ensure_fts_triggers(&self) -> Result<()> {
        // CREATE TRIGGER IF NOT EXISTS is idempotent - safe to call even if triggers exist
        self.recreate_fts_triggers()
    }

    /// Get statistics grouped by drive (first path component)
    pub fn get_drive_stats(&self) -> Result<Vec<DriveStatsResult>> {
        // Get file counts and sizes per drive (Windows paths start with drive letter like C:\)
        let mut stmt = self.conn.prepare(
            "SELECT
                SUBSTR(path, 1, 3) as drive,
                COUNT(*) as file_count,
                SUM(size) as total_size
             FROM files
             GROUP BY SUBSTR(path, 1, 3)
             ORDER BY total_size DESC"
        )?;

        let drives: Vec<(String, i64, i64)> = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
        })?.filter_map(|r| r.ok()).collect();

        let mut results = Vec::new();
        for (drive_path, file_count, total_size) in drives {
            // Get top extensions for this drive
            let mut ext_stmt = self.conn.prepare(
                "SELECT
                    COALESCE(LOWER(extension), 'no extension') as ext,
                    COUNT(*) as count,
                    SUM(size) as size
                 FROM files
                 WHERE SUBSTR(path, 1, 3) = ?1
                 GROUP BY ext
                 ORDER BY size DESC
                 LIMIT 10"
            )?;

            let extensions: Vec<ExtensionStats> = ext_stmt.query_map(params![&drive_path], |row| {
                Ok(ExtensionStats {
                    extension: row.get(0)?,
                    count: row.get(1)?,
                    size: row.get(2)?,
                })
            })?.filter_map(|r| r.ok()).collect();

            results.push(DriveStatsResult {
                path: drive_path.clone(),
                name: drive_path,
                file_count,
                total_size,
                extensions,
            });
        }

        Ok(results)
    }

    // ========== Known Drives (Offline Detection) ==========

    /// Save or update a known drive after scanning
    pub fn upsert_known_drive(
        &self,
        path: &str,
        volume_name: Option<&str>,
        drive_type: &str,
        total_files: i64,
        total_size: i64,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO known_drives (path, volume_name, drive_type, last_scan_at, total_files, total_size)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![path, volume_name, drive_type, now, total_files, total_size],
        )?;
        debug!("Upserted known drive: {} ({} files, {} bytes)", path, total_files, total_size);
        Ok(())
    }

    /// Get all known drives from database
    pub fn get_known_drives(&self) -> Result<Vec<KnownDrive>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, volume_name, drive_type, last_scan_at, total_files, total_size
             FROM known_drives"
        )?;

        let drives = stmt.query_map([], |row| {
            Ok(KnownDrive {
                path: row.get(0)?,
                volume_name: row.get(1)?,
                drive_type: row.get(2)?,
                last_scan_at: row.get(3)?,
                total_files: row.get(4)?,
                total_size: row.get(5)?,
            })
        })?.filter_map(|r| r.ok()).collect();

        Ok(drives)
    }

    /// Remove a known drive and all its files from database
    pub fn remove_known_drive(&self, path: &str) -> Result<(i64, i64)> {
        // Start transaction
        let tx = self.conn.unchecked_transaction()?;

        // Delete from known_drives
        tx.execute("DELETE FROM known_drives WHERE path = ?1", params![path])?;

        // Delete all files from this drive (case-insensitive for Windows)
        let pattern = format!("{}%", path.to_uppercase());
        let deleted_files: i64 = tx.query_row(
            "SELECT COUNT(*) FROM files WHERE UPPER(path) LIKE ?1",
            params![pattern],
            |row| row.get(0),
        )?;

        let deleted_size: i64 = tx.query_row(
            "SELECT COALESCE(SUM(size), 0) FROM files WHERE UPPER(path) LIKE ?1",
            params![pattern],
            |row| row.get(0),
        )?;

        tx.execute("DELETE FROM files WHERE UPPER(path) LIKE ?1", params![pattern])?;

        tx.commit()?;

        info!("Removed known drive {}: {} files, {} bytes", path, deleted_files, deleted_size);
        Ok((deleted_files, deleted_size))
    }

    /// Check if a drive has been scanned before
    pub fn is_drive_known(&self, path: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM known_drives WHERE UPPER(path) = UPPER(?1)",
            params![path],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get known drive info by path
    pub fn get_known_drive(&self, path: &str) -> Result<Option<KnownDrive>> {
        self.conn.query_row(
            "SELECT path, volume_name, drive_type, last_scan_at, total_files, total_size
             FROM known_drives WHERE UPPER(path) = UPPER(?1)",
            params![path],
            |row| {
                Ok(KnownDrive {
                    path: row.get(0)?,
                    volume_name: row.get(1)?,
                    drive_type: row.get(2)?,
                    last_scan_at: row.get(3)?,
                    total_files: row.get(4)?,
                    total_size: row.get(5)?,
                })
            },
        ).optional().map_err(Into::into)
    }

    // ========== Incremental Scan Support ==========

    /// Get all file paths and mtimes for a drive (for incremental scan)
    pub fn get_drive_files_map(&self, drive_path: &str) -> Result<std::collections::HashMap<String, Option<i64>>> {
        let pattern = format!("{}%", drive_path.to_uppercase());
        let mut stmt = self.conn.prepare(
            "SELECT path, modified_at FROM files WHERE UPPER(path) LIKE ?1"
        )?;

        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })?;

        for row in rows {
            if let Ok((path, mtime)) = row {
                map.insert(path.to_uppercase(), mtime);
            }
        }

        Ok(map)
    }

    /// Get file count for a specific drive
    pub fn get_drive_file_count(&self, drive_path: &str) -> Result<i64> {
        let pattern = format!("{}%", drive_path.to_uppercase());
        self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE UPPER(path) LIKE ?1",
            params![pattern],
            |row| row.get(0),
        ).map_err(Into::into)
    }

    /// Delete files by paths (batch operation for incremental cleanup)
    /// Optimized to use a temporary table for efficient bulk deletion
    /// Paths should be normalized (uppercase with backslash separators)
    pub fn delete_files_by_paths(&self, paths: &[String]) -> Result<usize> {
        use tracing::info;

        if paths.is_empty() {
            return Ok(0);
        }

        info!("Starting deletion of {} files using temp table approach...", paths.len());
        let start = std::time::Instant::now();

        let tx = self.conn.unchecked_transaction()?;

        // Create temporary table for paths to delete
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS paths_to_delete (normalized_path TEXT PRIMARY KEY);"
        )?;
        tx.execute("DELETE FROM paths_to_delete;", [])?;

        // Insert all normalized paths into temp table (batched)
        let mut inserted = 0;
        for chunk in paths.chunks(500) {
            let placeholders: String = chunk.iter().map(|_| "(?)").collect::<Vec<_>>().join(",");
            let query = format!("INSERT OR IGNORE INTO paths_to_delete (normalized_path) VALUES {}", placeholders);

            let params: Vec<&dyn rusqlite::ToSql> = chunk
                .iter()
                .map(|s| s as &dyn rusqlite::ToSql)
                .collect();

            inserted += tx.execute(&query, params.as_slice())?;
        }

        info!("Inserted {} paths to temp table in {:?}", inserted, start.elapsed());

        // Delete using JOIN - much more efficient than IN with function calls
        // This uses a single scan of the files table instead of one per batch
        let deleted = tx.execute(
            "DELETE FROM files WHERE id IN (
                SELECT f.id FROM files f
                INNER JOIN paths_to_delete p ON UPPER(REPLACE(f.path, '/', '\\')) = p.normalized_path
            )",
            [],
        )?;

        // Clean up temp table
        tx.execute("DELETE FROM paths_to_delete;", [])?;

        tx.commit()?;

        info!("Deleted {} files in {:?} total", deleted, start.elapsed());
        debug!("Deleted {} files by path using temp table", deleted);
        Ok(deleted)
    }

    /// Update a single file's metadata (for incremental updates)
    pub fn update_file(&self, file: &FileMetadata, scan_id: i64) -> Result<bool> {
        let rows = self.conn.execute(
            "UPDATE files SET name = ?2, extension = ?3, size = ?4, modified_at = ?5,
             partial_hash = ?6, scan_id = ?7 WHERE UPPER(path) = UPPER(?1)",
            params![
                file.path,
                file.name,
                file.extension,
                file.size,
                file.modified_at,
                file.partial_hash,
                scan_id
            ],
        )?;
        Ok(rows > 0)
    }

    /// Batch update files efficiently using a temp table approach
    /// Much faster than individual updates when processing many files
    pub fn update_files_batch(&self, files: &[FileMetadata], scan_id: i64) -> Result<usize> {
        if files.is_empty() {
            return Ok(0);
        }

        info!("Starting batch update of {} files using temp table approach...", files.len());
        let start = std::time::Instant::now();

        let tx = self.conn.unchecked_transaction()?;

        // Create temporary table for updates
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS files_to_update (
                normalized_path TEXT PRIMARY KEY,
                name TEXT,
                extension TEXT,
                size INTEGER,
                modified_at INTEGER,
                partial_hash TEXT
            );"
        )?;
        tx.execute("DELETE FROM files_to_update;", [])?;

        // Insert all files to update into temp table
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO files_to_update (normalized_path, name, extension, size, modified_at, partial_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
            )?;

            for file in files {
                // Normalize path for matching
                let normalized = file.path.replace('/', "\\").to_uppercase();
                stmt.execute(params![
                    normalized,
                    file.name,
                    file.extension,
                    file.size,
                    file.modified_at,
                    file.partial_hash
                ])?;
            }
        }

        info!("Inserted {} files to temp table in {:?}", files.len(), start.elapsed());

        // Update using JOIN - single table scan instead of one per file
        let updated = tx.execute(
            &format!(
                "UPDATE files SET
                    name = (SELECT u.name FROM files_to_update u WHERE UPPER(REPLACE(files.path, '/', '\\')) = u.normalized_path),
                    extension = (SELECT u.extension FROM files_to_update u WHERE UPPER(REPLACE(files.path, '/', '\\')) = u.normalized_path),
                    size = (SELECT u.size FROM files_to_update u WHERE UPPER(REPLACE(files.path, '/', '\\')) = u.normalized_path),
                    modified_at = (SELECT u.modified_at FROM files_to_update u WHERE UPPER(REPLACE(files.path, '/', '\\')) = u.normalized_path),
                    partial_hash = (SELECT u.partial_hash FROM files_to_update u WHERE UPPER(REPLACE(files.path, '/', '\\')) = u.normalized_path),
                    scan_id = {}
                WHERE EXISTS (
                    SELECT 1 FROM files_to_update u WHERE UPPER(REPLACE(files.path, '/', '\\')) = u.normalized_path
                )",
                scan_id
            ),
            [],
        )?;

        // Clean up temp table
        tx.execute("DELETE FROM files_to_update;", [])?;

        tx.commit()?;

        info!("Updated {} files in {:?} total", updated, start.elapsed());
        Ok(updated)
    }

    /// Insert a single file (for incremental new files)
    pub fn insert_file(&self, file: &FileMetadata, scan_id: i64) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO files (path, name, extension, size, created_at, modified_at, accessed_at, attributes, partial_hash, scan_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                file.path,
                file.name,
                file.extension,
                file.size,
                file.created_at,
                file.modified_at,
                file.accessed_at,
                file.attributes,
                file.partial_hash,
                scan_id
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    // ========== Overlapping Drive Detection ==========

    /// Sample random file relative paths from a drive (for overlap detection)
    /// Returns paths relative to the drive root (e.g., "Users\foo\bar.txt")
    pub fn sample_drive_relative_paths(&self, drive_path: &str, sample_size: usize) -> Result<Vec<String>> {
        let pattern = format!("{}%", drive_path.to_uppercase());
        let drive_len = drive_path.len();

        // Sample random files using RANDOM() - efficient for large tables
        let mut stmt = self.conn.prepare(
            "SELECT SUBSTR(path, ?1) FROM files
             WHERE UPPER(path) LIKE ?2
             ORDER BY RANDOM()
             LIMIT ?3"
        )?;

        let paths: Vec<String> = stmt.query_map(
            params![drive_len + 1, pattern, sample_size as i64],
            |row| row.get(0)
        )?.filter_map(|r| r.ok()).collect();

        Ok(paths)
    }

    /// Check how many of the given relative paths exist on a different drive
    /// Returns the count of matching paths
    pub fn count_matching_paths(&self, drive_path: &str, relative_paths: &[String]) -> Result<usize> {
        if relative_paths.is_empty() {
            return Ok(0);
        }

        let mut matches = 0;
        for rel_path in relative_paths {
            let full_path = format!("{}{}", drive_path, rel_path).to_uppercase();
            let exists: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM files WHERE UPPER(path) = ?1",
                params![full_path],
                |row| row.get(0),
            )?;
            if exists > 0 {
                matches += 1;
            }
        }

        Ok(matches)
    }

    /// Detect overlapping drives by sampling and comparing file paths
    /// Returns pairs of (drive_a, drive_b, overlap_percentage)
    /// where drive_a's files are also found on drive_b
    pub fn detect_overlapping_drives(&self, sample_size: usize, threshold_percent: f64) -> Result<Vec<DriveOverlap>> {
        // Get all known drives from database (handles both local and network drives correctly)
        let known_drives = self.get_known_drives()?;

        if known_drives.len() < 2 {
            info!("Need at least 2 scanned drives to detect overlaps, found {}", known_drives.len());
            return Ok(Vec::new());
        }

        let drives: Vec<String> = known_drives.iter().map(|k| k.path.clone()).collect();
        info!("Checking {} drives for overlaps: {:?}", drives.len(), drives);

        let mut overlaps = Vec::new();

        for i in 0..drives.len() {
            let drive_a = &drives[i];

            // Sample files from drive A
            let samples = self.sample_drive_relative_paths(drive_a, sample_size)?;
            if samples.is_empty() {
                debug!("No samples found for drive {}", drive_a);
                continue;
            }

            debug!("Sampled {} paths from {}", samples.len(), drive_a);

            // Check against all other drives
            for j in 0..drives.len() {
                if i == j {
                    continue;
                }

                let drive_b = &drives[j];
                let matches = self.count_matching_paths(drive_b, &samples)?;
                let overlap_percent = (matches as f64 / samples.len() as f64) * 100.0;

                debug!(
                    "Overlap check: {} -> {} = {}/{} ({:.1}%)",
                    drive_a, drive_b, matches, samples.len(), overlap_percent
                );

                if overlap_percent >= threshold_percent {
                    overlaps.push(DriveOverlap {
                        source_drive: drive_a.clone(),
                        target_drive: drive_b.clone(),
                        overlap_percent,
                        sample_size: samples.len(),
                        matches_found: matches,
                    });
                }
            }
        }

        Ok(overlaps)
    }

    /// Extract drive root from a path (handles both local and UNC paths)
    /// C:\Users\foo -> C:\
    /// \\server\share\foo -> \\server\share\
    pub fn extract_drive_root(path: &str) -> String {
        if path.len() >= 2 && path.chars().nth(1) == Some(':') {
            // Local path: C:\...
            let drive_letter = path.chars().next().unwrap().to_uppercase().next().unwrap();
            format!("{}:\\", drive_letter)
        } else if path.starts_with("\\\\") {
            // UNC path: \\server\share\...
            let without_prefix = path.trim_start_matches("\\\\");
            let parts: Vec<&str> = without_prefix.splitn(3, '\\').collect();
            if parts.len() >= 2 {
                format!("\\\\{}\\{}\\", parts[0], parts[1])
            } else {
                path.to_string()
            }
        } else {
            path.to_string()
        }
    }
}

// ========== Folder Sizes (Treemap) ==========

/// Folder size information for treemap visualization
#[derive(Debug, Clone, serde::Serialize)]
pub struct FolderSize {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub drive: String,
    pub depth: i32,
    pub total_size: i64,
    pub file_count: i64,
    pub folder_count: i64,
    pub parent_path: Option<String>,
}

/// Treemap node for hierarchical visualization
#[derive(Debug, Clone, serde::Serialize)]
pub struct TreemapNode {
    pub path: String,
    pub name: String,
    pub size: i64,
    pub file_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<TreemapNode>>,
}

impl Database {
    /// Rebuild folder_sizes table from files table
    /// Called after scan completes to pre-compute folder aggregations
    /// Uses a Rust-based approach for reliable path handling
    pub fn rebuild_folder_sizes(&self, scan_id: i64) -> Result<usize> {
        use std::collections::HashMap;

        info!("Rebuilding folder_sizes table...");
        let start = std::time::Instant::now();

        // Clear all existing folder_sizes (we rebuild completely)
        self.conn.execute("DELETE FROM folder_sizes", [])?;

        // Step 1: Read all files and aggregate by folder path
        // HashMap: folder_path -> (total_size, file_count)
        let mut folder_stats: HashMap<String, (i64, i64)> = HashMap::new();

        {
            let mut stmt = self.conn.prepare("SELECT path, size FROM files")?;
            let mut rows = stmt.query([])?;

            while let Some(row) = rows.next()? {
                let file_path: String = row.get(0)?;
                let size: i64 = row.get(1)?;

                // Extract folder path from file path
                if let Some(last_sep) = file_path.rfind('\\') {
                    let mut folder_path = &file_path[..last_sep];

                    // Aggregate to this folder and all parent folders
                    loop {
                        let entry = folder_stats.entry(folder_path.to_string()).or_insert((0, 0));
                        entry.0 += size;
                        entry.1 += 1;

                        // Find parent folder
                        if folder_path.len() <= 3 {
                            // We're at drive root (e.g., "C:\"), stop
                            break;
                        }

                        if let Some(parent_sep) = folder_path[..folder_path.len()].rfind('\\') {
                            if parent_sep < 3 {
                                // Parent is drive root
                                folder_path = &folder_path[..3];
                            } else {
                                folder_path = &folder_path[..parent_sep];
                            }
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        info!("Aggregated {} unique folders", folder_stats.len());

        // Step 2: Count direct subfolders for each folder
        let mut subfolder_counts: HashMap<String, i64> = HashMap::new();
        for folder_path in folder_stats.keys() {
            if let Some(parent) = get_parent_path(folder_path) {
                *subfolder_counts.entry(parent).or_insert(0) += 1;
            }
        }

        // Step 3: Insert all folder stats into database
        let tx = self.conn.unchecked_transaction()?;
        let mut inserted = 0;

        {
            let mut insert_stmt = tx.prepare(
                "INSERT OR REPLACE INTO folder_sizes (path, name, drive, depth, total_size, file_count, folder_count, parent_path, scan_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
            )?;

            for (folder_path, (total_size, file_count)) in &folder_stats {
                let name = get_folder_name(folder_path);
                let drive = get_drive_root(folder_path);
                let depth = get_folder_depth(folder_path);
                let parent_path = get_parent_path(folder_path);
                let folder_count = subfolder_counts.get(folder_path).copied().unwrap_or(0);

                insert_stmt.execute(params![
                    folder_path,
                    name,
                    drive,
                    depth,
                    total_size,
                    file_count,
                    folder_count,
                    parent_path,
                    scan_id
                ])?;
                inserted += 1;
            }
        }

        tx.commit()?;

        let elapsed = start.elapsed();
        info!("Rebuilt folder_sizes: {} folders in {:.2}s", inserted, elapsed.as_secs_f64());

        Ok(inserted)
    }

    /// Get treemap data for a specific path with children up to max_depth
    /// Optimized: fetches all needed data in ONE query and builds tree in memory
    pub fn get_treemap_data(
        &self,
        drive: Option<&str>,
        path: Option<&str>,
        max_depth: i32,
        min_size: i64,
    ) -> Result<Vec<TreemapNode>> {
        use std::collections::HashMap;

        info!("get_treemap_data called: drive={:?}, path={:?}, max_depth={}, min_size={}",
              drive, path, max_depth, min_size);

        // Normalize drive path (ensure it ends with backslash for consistency)
        let drive_normalized = drive.map(|d| {
            if d.ends_with('\\') { d.to_string() } else { format!("{}\\", d) }
        });

        // Determine the filtering strategy
        let folders: Vec<FolderSize> = match (&drive_normalized, path) {
            // Case 1: Specific path - get children of that path
            (_, Some(p)) => {
                let pattern = format!("{}\\%", p.trim_end_matches('\\'));
                let base_depth = if p.len() <= 3 { 0 } else { (p.matches('\\').count() as i32) - 1 };
                let target_depth = base_depth + max_depth;

                info!("Querying path '{}' with pattern '{}', depth {} to {}", p, pattern, base_depth, target_depth);

                let mut stmt = self.conn.prepare(
                    "SELECT id, path, name, drive, depth, total_size, file_count, folder_count, parent_path
                     FROM folder_sizes
                     WHERE path LIKE ?1 AND depth <= ?2 AND total_size >= ?3
                     ORDER BY depth, total_size DESC"
                )?;
                let result: Vec<FolderSize> = stmt.query_map(params![pattern, target_depth, min_size], |row| {
                    Ok(FolderSize {
                        id: row.get(0)?, path: row.get(1)?, name: row.get(2)?,
                        drive: row.get(3)?, depth: row.get(4)?, total_size: row.get(5)?,
                        file_count: row.get(6)?, folder_count: row.get(7)?, parent_path: row.get(8)?,
                    })
                })?.filter_map(|r| r.ok()).collect();
                result
            },

            // Case 2: Drive selected, no specific path - get drive root children
            (Some(d), None) => {
                info!("Querying drive '{}', depth 0 to {}", d, max_depth);

                let mut stmt = self.conn.prepare(
                    "SELECT id, path, name, drive, depth, total_size, file_count, folder_count, parent_path
                     FROM folder_sizes
                     WHERE drive = ?1 AND depth <= ?2 AND total_size >= ?3
                     ORDER BY depth, total_size DESC"
                )?;
                let result: Vec<FolderSize> = stmt.query_map(params![d, max_depth, min_size], |row| {
                    Ok(FolderSize {
                        id: row.get(0)?, path: row.get(1)?, name: row.get(2)?,
                        drive: row.get(3)?, depth: row.get(4)?, total_size: row.get(5)?,
                        file_count: row.get(6)?, folder_count: row.get(7)?, parent_path: row.get(8)?,
                    })
                })?.filter_map(|r| r.ok()).collect();
                result
            },

            // Case 3: All drives - get all roots with children
            (None, None) => {
                info!("Querying all drives, depth 0 to {}", max_depth);

                let mut stmt = self.conn.prepare(
                    "SELECT id, path, name, drive, depth, total_size, file_count, folder_count, parent_path
                     FROM folder_sizes
                     WHERE depth <= ?1 AND total_size >= ?2
                     ORDER BY depth, total_size DESC"
                )?;
                let result: Vec<FolderSize> = stmt.query_map(params![max_depth, min_size], |row| {
                    Ok(FolderSize {
                        id: row.get(0)?, path: row.get(1)?, name: row.get(2)?,
                        drive: row.get(3)?, depth: row.get(4)?, total_size: row.get(5)?,
                        file_count: row.get(6)?, folder_count: row.get(7)?, parent_path: row.get(8)?,
                    })
                })?.filter_map(|r| r.ok()).collect();
                result
            },
        };

        info!("Fetched {} folders from database", folders.len());

        // Build tree in memory
        let mut nodes_map: HashMap<String, TreemapNode> = HashMap::new();
        for folder in &folders {
            nodes_map.insert(folder.path.clone(), TreemapNode {
                path: folder.path.clone(),
                name: folder.name.clone(),
                size: folder.total_size,
                file_count: folder.file_count,
                children: None,
            });
        }

        // Attach children to parents (process from deepest to shallowest)
        let mut sorted_folders = folders.clone();
        sorted_folders.sort_by(|a, b| b.depth.cmp(&a.depth));

        for folder in &sorted_folders {
            if let Some(ref parent_path) = folder.parent_path {
                if let Some(child_node) = nodes_map.remove(&folder.path) {
                    if let Some(parent_node) = nodes_map.get_mut(parent_path) {
                        parent_node.children.get_or_insert_with(Vec::new).push(child_node);
                    } else {
                        nodes_map.insert(folder.path.clone(), child_node);
                    }
                }
            }
        }

        // Sort children by size descending
        fn sort_children(node: &mut TreemapNode) {
            if let Some(ref mut children) = node.children {
                children.sort_by(|a, b| b.size.cmp(&a.size));
                children.truncate(30); // Limit children per node
                for child in children.iter_mut() {
                    sort_children(child);
                }
            }
        }
        for node in nodes_map.values_mut() {
            sort_children(node);
        }

        // Determine which nodes to return as top-level
        let mut result: Vec<TreemapNode> = match (&drive_normalized, path) {
            (_, Some(p)) => {
                // Return direct children of the specified path
                let parent = p.trim_end_matches('\\');
                nodes_map.into_values()
                    .filter(|n| {
                        sorted_folders.iter()
                            .find(|f| f.path == n.path)
                            .map(|f| f.parent_path.as_deref() == Some(parent))
                            .unwrap_or(false)
                    })
                    .collect()
            },
            (Some(d), None) => {
                // Return depth 0 folders for the selected drive
                nodes_map.into_values()
                    .filter(|n| {
                        sorted_folders.iter()
                            .find(|f| f.path == n.path)
                            .map(|f| f.depth == 0 && f.drive == *d)
                            .unwrap_or(false)
                    })
                    .collect()
            },
            (None, None) => {
                // Return all depth 0 folders (drive roots)
                nodes_map.into_values()
                    .filter(|n| {
                        sorted_folders.iter()
                            .find(|f| f.path == n.path)
                            .map(|f| f.depth == 0)
                            .unwrap_or(false)
                    })
                    .collect()
            },
        };

        result.sort_by(|a, b| b.size.cmp(&a.size));
        info!("Returning {} top-level nodes", result.len());

        Ok(result)
    }

    /// Get folder children for a specific path (single level, for lazy loading)
    pub fn get_folder_children(&self, path: &str, min_size: i64, limit: i64) -> Result<Vec<FolderSize>> {
        let mut stmt = self.conn.prepare_cached(
            r#"
            SELECT id, path, name, drive, depth, total_size, file_count, folder_count, parent_path
            FROM folder_sizes
            WHERE parent_path = ?1 AND total_size >= ?2
            ORDER BY total_size DESC
            LIMIT ?3
            "#,
        )?;

        let folders = stmt
            .query_map(params![path, min_size, limit], |row| {
                Ok(FolderSize {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    drive: row.get(3)?,
                    depth: row.get(4)?,
                    total_size: row.get(5)?,
                    file_count: row.get(6)?,
                    folder_count: row.get(7)?,
                    parent_path: row.get(8)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(folders)
    }

    /// Clear folder_sizes for specific drives (before rescan)
    pub fn clear_folder_sizes_for_drives(&self, drive_paths: &[String]) -> Result<usize> {
        let mut total = 0;
        for drive in drive_paths {
            let pattern = format!("{}%", drive);
            let deleted = self.conn.execute(
                "DELETE FROM folder_sizes WHERE drive LIKE ?1",
                params![pattern],
            )?;
            total += deleted;
        }
        Ok(total)
    }
}

/// Known drive information (for offline detection)
#[derive(Debug, Clone, serde::Serialize)]
pub struct KnownDrive {
    pub path: String,
    pub volume_name: Option<String>,
    pub drive_type: String,
    pub last_scan_at: i64,
    pub total_files: i64,
    pub total_size: i64,
}

/// Drive overlap detection result
#[derive(Debug, Clone, serde::Serialize)]
pub struct DriveOverlap {
    /// The drive whose files were sampled
    pub source_drive: String,
    /// The drive where matching files were found
    pub target_drive: String,
    /// Percentage of sampled files that exist on target
    pub overlap_percent: f64,
    /// Number of files sampled from source
    pub sample_size: usize,
    /// Number of matches found on target
    pub matches_found: usize,
}

/// Drive statistics result
#[derive(Debug, serde::Serialize)]
pub struct DriveStatsResult {
    pub path: String,
    pub name: String,
    pub file_count: i64,
    pub total_size: i64,
    pub extensions: Vec<ExtensionStats>,
}

/// Extension statistics
#[derive(Debug, serde::Serialize)]
pub struct ExtensionStats {
    pub extension: String,
    pub count: i64,
    pub size: i64,
}

/// File search result
#[derive(Debug, serde::Serialize)]
pub struct FileSearchResult {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub size: i64,
    pub modified_at: Option<i64>,
}

/// Scan information
#[derive(Debug, serde::Serialize)]
pub struct ScanInfo {
    pub id: i64,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub root_paths: String,
    pub total_files: Option<i64>,
    pub total_size: Option<i64>,
    pub status: String,
}

// ========== Path Helper Functions ==========

/// Get the folder name from a full path
fn get_folder_name(path: &str) -> String {
    if path.len() <= 3 {
        // Drive root (e.g., "C:\")
        return path.to_string();
    }

    if let Some(last_sep) = path.rfind('\\') {
        path[last_sep + 1..].to_string()
    } else {
        path.to_string()
    }
}

/// Get the drive root from a path (e.g., "C:\" from "C:\Users\foo")
fn get_drive_root(path: &str) -> String {
    if path.len() >= 3 && path.chars().nth(1) == Some(':') {
        path[..3].to_uppercase()
    } else if path.starts_with("\\\\") {
        // UNC path: \\server\share -> \\server\share\
        let without_prefix = path.trim_start_matches("\\\\");
        let parts: Vec<&str> = without_prefix.splitn(3, '\\').collect();
        if parts.len() >= 2 {
            format!("\\\\{}\\{}\\", parts[0], parts[1])
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    }
}

/// Get folder depth (0 = drive root, 1 = first level folder, etc.)
fn get_folder_depth(path: &str) -> i32 {
    if path.len() <= 3 {
        return 0;
    }

    // Count backslashes after the drive root
    let after_root = &path[3..];
    after_root.matches('\\').count() as i32
}

/// Get parent folder path, or None if at drive root
fn get_parent_path(path: &str) -> Option<String> {
    if path.len() <= 3 {
        // Already at drive root
        return None;
    }

    if let Some(last_sep) = path.rfind('\\') {
        if last_sep < 3 {
            // Parent is drive root
            Some(path[..3].to_string())
        } else {
            Some(path[..last_sep].to_string())
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_database_creation() {
        let temp_file = NamedTempFile::new().unwrap();
        let db = Database::new(temp_file.path()).unwrap();
        db.initialize_schema().unwrap();

        assert_eq!(db.get_file_count().unwrap(), 0);
    }

    #[test]
    fn test_scan_creation() {
        let temp_file = NamedTempFile::new().unwrap();
        let db = Database::new(temp_file.path()).unwrap();
        db.initialize_schema().unwrap();

        let scan_id = db.create_scan(&["C:\\".to_string()]).unwrap();
        assert!(scan_id > 0);

        let scans = db.get_recent_scans(10).unwrap();
        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].status, "running");
    }
}
