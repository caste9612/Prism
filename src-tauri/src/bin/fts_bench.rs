//! FTS Indexing Benchmark Tool
//!
//! Tests different strategies for FTS5 indexing performance optimization
//! Run with: cargo run --release --bin fts_bench

use anyhow::Result;
use rusqlite::{params, Connection, OpenFlags};
use std::env;
use std::path::PathBuf;
use std::time::Instant;

/// Repair corrupt FTS index by dropping and recreating it
fn repair_fts_index(conn: &Connection) -> Result<u64> {
    println!("\n{}", "!".repeat(70));
    println!("! FTS INDEX REPAIR MODE");
    println!("{}", "!".repeat(70));

    // Drop triggers first
    println!("\n1. Dropping FTS triggers...");
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS files_ai;
         DROP TRIGGER IF EXISTS files_au;
         DROP TRIGGER IF EXISTS files_ad;"
    )?;
    println!("   Done.");

    // Drop and recreate FTS table
    println!("\n2. Dropping corrupt FTS table...");
    conn.execute("DROP TABLE IF EXISTS files_fts", [])?;
    println!("   Done.");

    println!("\n3. Creating fresh FTS5 table...");
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
            name, path, extension,
            content='files',
            content_rowid='id',
            tokenize='unicode61 remove_diacritics 1'
        )",
        [],
    )?;
    println!("   Done.");

    // Rebuild FTS from files table
    println!("\n4. Populating FTS index from files table...");
    let total_files: u64 = conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
    println!("   Files to index: {}", format_number(total_files));

    let start = Instant::now();
    let mut indexed: u64 = 0;
    let mut last_id: i64 = 0;
    const BATCH_SIZE: i64 = 50_000;

    loop {
        // Get the next batch of IDs - we need to track the actual max ID, not count
        let batch_max_id: Option<i64> = conn.query_row(
            "SELECT MAX(id) FROM (SELECT id FROM files WHERE id > ?1 ORDER BY id LIMIT ?2)",
            params![last_id, BATCH_SIZE],
            |row| row.get(0),
        ).ok();

        let Some(batch_max) = batch_max_id else {
            break;
        };

        // Insert this batch
        let rows_affected = conn.execute(
            "INSERT INTO files_fts(rowid, name, path, extension)
             SELECT id, name, path, extension FROM files
             WHERE id > ?1 AND id <= ?2",
            params![last_id, batch_max],
        )?;

        if rows_affected == 0 {
            break;
        }

        last_id = batch_max;
        indexed += rows_affected as u64;

        // Safety check: don't exceed expected file count by too much
        if indexed > total_files + 100_000 {
            println!("   WARNING: Indexed more files than expected, stopping.");
            break;
        }

        let elapsed = start.elapsed().as_secs_f64();
        let rate = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };
        println!("   Progress: {} / {} ({:.1}%) - {:.0} files/sec",
            format_number(indexed),
            format_number(total_files),
            indexed as f64 / total_files as f64 * 100.0,
            rate
        );
    }

    let elapsed = start.elapsed().as_secs_f64();
    let rate = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };

    // Recreate triggers
    println!("\n5. Recreating FTS triggers...");
    conn.execute_batch(
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
    println!("   Done.");

    println!("\n{}", "=".repeat(70));
    println!("FTS INDEX REPAIRED SUCCESSFULLY");
    println!("  Files indexed: {}", format_number(indexed));
    println!("  Time elapsed:  {:.2}s", elapsed);
    println!("  Rate:          {:.0} files/sec", rate);
    println!("{}", "=".repeat(70));

    Ok(indexed)
}

/// Check if FTS index is healthy (comprehensive check including write operations)
fn check_fts_health(conn: &Connection) -> bool {
    // Try a simple FTS query first
    if conn.query_row(
        "SELECT COUNT(*) FROM files_fts WHERE files_fts MATCH 'test' LIMIT 1",
        [],
        |_| Ok(()),
    ).is_err() {
        return false;
    }

    // Try the FTS integrity check command
    match conn.query_row(
        "INSERT INTO files_fts(files_fts) VALUES('integrity-check')",
        [],
        |_| Ok(()),
    ) {
        Ok(_) => true,
        Err(e) => {
            println!("  FTS integrity check failed: {}", e);
            false
        }
    }
}

#[derive(Debug, Clone)]
struct BenchConfig {
    db_path: PathBuf,
    test_batch_sizes: Vec<u64>,
    test_pragmas: bool,
    test_transactions: bool,
}

#[derive(Debug, Clone)]
struct BenchResult {
    strategy: String,
    files_indexed: u64,
    elapsed_secs: f64,
    files_per_sec: f64,
    batch_size: Option<u64>,
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn get_db_path() -> PathBuf {
    // Try to get standard Prism database path
    // Tauri 2.x uses the identifier from tauri.conf.json
    let app_data = env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(app_data)
        .join("com.prism.diskanalyzer")
        .join("prism.db")
}

fn count_files(conn: &Connection) -> Result<u64> {
    let count: u64 = conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
    Ok(count)
}

fn count_fts_entries(conn: &Connection) -> Result<u64> {
    let count: u64 = conn.query_row("SELECT COUNT(*) FROM files_fts", [], |row| row.get(0))?;
    Ok(count)
}

/// Strategy 0: FTS5 native 'rebuild' command
fn bench_fts_rebuild(conn: &Connection) -> Result<BenchResult> {
    println!("\n[FTS-REBUILD] Using FTS5 native rebuild command...");

    let total_files = count_files(conn)?;
    if total_files == 0 {
        return Ok(BenchResult {
            strategy: "fts-rebuild".to_string(),
            files_indexed: 0,
            elapsed_secs: 0.0,
            files_per_sec: 0.0,
            batch_size: None,
        });
    }

    // Clear FTS using proper FTS5 command
    conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", [])?;

    let start = Instant::now();

    // Use FTS5 native rebuild - this reads directly from content table
    conn.execute("INSERT INTO files_fts(files_fts) VALUES('rebuild')", [])?;

    let elapsed = start.elapsed().as_secs_f64();
    let rate = if elapsed > 0.0 { total_files as f64 / elapsed } else { 0.0 };

    Ok(BenchResult {
        strategy: "fts-rebuild".to_string(),
        files_indexed: total_files,
        elapsed_secs: elapsed,
        files_per_sec: rate,
        batch_size: None,
    })
}

/// Strategy 1: Current approach - batch INSERT INTO ... SELECT
fn bench_batch_insert_select(conn: &Connection, batch_size: u64) -> Result<BenchResult> {
    println!("\n[BATCH-INSERT-SELECT] Batch size: {}...", format_number(batch_size));

    let total_files = count_files(conn)?;
    if total_files == 0 {
        return Ok(BenchResult {
            strategy: format!("batch-insert-select-{}", batch_size),
            files_indexed: 0,
            elapsed_secs: 0.0,
            files_per_sec: 0.0,
            batch_size: Some(batch_size),
        });
    }

    // Clear FTS using proper FTS5 command (not DELETE which corrupts content-table FTS)
    conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", [])?;

    let start = Instant::now();
    let mut indexed: u64 = 0;
    let mut last_id: i64 = 0;

    loop {
        // First get the max ID for this batch
        let batch_max_id: Option<i64> = conn.query_row(
            "SELECT MAX(id) FROM (SELECT id FROM files WHERE id > ?1 ORDER BY id LIMIT ?2)",
            params![last_id, batch_size as i64],
            |row| row.get(0),
        ).ok();

        let Some(batch_max) = batch_max_id else {
            break;
        };

        // Insert this batch
        let rows_affected = conn.execute(
            "INSERT INTO files_fts(rowid, name, path, extension)
             SELECT id, name, path, extension FROM files
             WHERE id > ?1 AND id <= ?2",
            params![last_id, batch_max],
        )?;

        if rows_affected == 0 {
            break;
        }

        last_id = batch_max;
        indexed += rows_affected as u64;

        if indexed % 100_000 == 0 {
            println!("  Progress: {} / {} ({:.1}%)",
                format_number(indexed),
                format_number(total_files),
                indexed as f64 / total_files as f64 * 100.0
            );
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let rate = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };

    Ok(BenchResult {
        strategy: format!("batch-insert-select-{}", batch_size),
        files_indexed: indexed,
        elapsed_secs: elapsed,
        files_per_sec: rate,
        batch_size: Some(batch_size),
    })
}

/// Strategy 2: Transaction-wrapped batch inserts
fn bench_transaction_batches(conn: &Connection, batch_size: u64) -> Result<BenchResult> {
    println!("\n[TRANSACTION-BATCHES] Batch size: {} with explicit transactions...", format_number(batch_size));

    let total_files = count_files(conn)?;
    if total_files == 0 {
        return Ok(BenchResult {
            strategy: format!("tx-batches-{}", batch_size),
            files_indexed: 0,
            elapsed_secs: 0.0,
            files_per_sec: 0.0,
            batch_size: Some(batch_size),
        });
    }

    // Clear FTS
    conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", [])?;

    let start = Instant::now();
    let mut indexed: u64 = 0;
    let mut last_id: i64 = 0;

    loop {
        // First get the max ID for this batch
        let batch_max_id: Option<i64> = conn.query_row(
            "SELECT MAX(id) FROM (SELECT id FROM files WHERE id > ?1 ORDER BY id LIMIT ?2)",
            params![last_id, batch_size as i64],
            |row| row.get(0),
        ).ok();

        let Some(batch_max) = batch_max_id else {
            break;
        };

        // Wrap each batch in explicit transaction
        conn.execute("BEGIN IMMEDIATE", [])?;

        let rows_affected = conn.execute(
            "INSERT INTO files_fts(rowid, name, path, extension)
             SELECT id, name, path, extension FROM files
             WHERE id > ?1 AND id <= ?2",
            params![last_id, batch_max],
        )?;

        conn.execute("COMMIT", [])?;

        if rows_affected == 0 {
            break;
        }

        last_id = batch_max;
        indexed += rows_affected as u64;

        if indexed % 100_000 == 0 {
            println!("  Progress: {} / {} ({:.1}%)",
                format_number(indexed),
                format_number(total_files),
                indexed as f64 / total_files as f64 * 100.0
            );
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let rate = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };

    Ok(BenchResult {
        strategy: format!("tx-batches-{}", batch_size),
        files_indexed: indexed,
        elapsed_secs: elapsed,
        files_per_sec: rate,
        batch_size: Some(batch_size),
    })
}

/// Strategy 3: Single large transaction for all inserts
fn bench_single_transaction(conn: &Connection, batch_size: u64) -> Result<BenchResult> {
    println!("\n[SINGLE-TRANSACTION] All inserts in one transaction (batch: {})...", format_number(batch_size));

    let total_files = count_files(conn)?;
    if total_files == 0 {
        return Ok(BenchResult {
            strategy: format!("single-tx-{}", batch_size),
            files_indexed: 0,
            elapsed_secs: 0.0,
            files_per_sec: 0.0,
            batch_size: Some(batch_size),
        });
    }

    // Clear FTS
    conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", [])?;

    let start = Instant::now();

    // Start single transaction
    conn.execute("BEGIN IMMEDIATE", [])?;

    let mut indexed: u64 = 0;
    let mut last_id: i64 = 0;

    loop {
        // First get the max ID for this batch
        let batch_max_id: Option<i64> = conn.query_row(
            "SELECT MAX(id) FROM (SELECT id FROM files WHERE id > ?1 ORDER BY id LIMIT ?2)",
            params![last_id, batch_size as i64],
            |row| row.get(0),
        ).ok();

        let Some(batch_max) = batch_max_id else {
            break;
        };

        let rows_affected = conn.execute(
            "INSERT INTO files_fts(rowid, name, path, extension)
             SELECT id, name, path, extension FROM files
             WHERE id > ?1 AND id <= ?2",
            params![last_id, batch_max],
        )?;

        if rows_affected == 0 {
            break;
        }

        last_id = batch_max;
        indexed += rows_affected as u64;

        if indexed % 100_000 == 0 {
            println!("  Progress: {} / {} ({:.1}%)",
                format_number(indexed),
                format_number(total_files),
                indexed as f64 / total_files as f64 * 100.0
            );
        }
    }

    // Commit all at once
    conn.execute("COMMIT", [])?;

    let elapsed = start.elapsed().as_secs_f64();
    let rate = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };

    Ok(BenchResult {
        strategy: format!("single-tx-{}", batch_size),
        files_indexed: indexed,
        elapsed_secs: elapsed,
        files_per_sec: rate,
        batch_size: Some(batch_size),
    })
}

/// Strategy 4: Row-by-row with prepared statement (worst case baseline)
fn bench_row_by_row(conn: &Connection, limit: u64) -> Result<BenchResult> {
    println!("\n[ROW-BY-ROW] Individual inserts (limited to {} rows)...", format_number(limit));

    let total_files = count_files(conn)?;
    let actual_limit = limit.min(total_files);

    if actual_limit == 0 {
        return Ok(BenchResult {
            strategy: "row-by-row".to_string(),
            files_indexed: 0,
            elapsed_secs: 0.0,
            files_per_sec: 0.0,
            batch_size: None,
        });
    }

    // Clear FTS
    conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", [])?;

    let start = Instant::now();

    conn.execute("BEGIN IMMEDIATE", [])?;

    // Prepare statements
    let mut select_stmt = conn.prepare(
        "SELECT id, name, path, extension FROM files ORDER BY id LIMIT ?1"
    )?;
    let mut insert_stmt = conn.prepare(
        "INSERT INTO files_fts(rowid, name, path, extension) VALUES (?1, ?2, ?3, ?4)"
    )?;

    let mut indexed: u64 = 0;

    let rows = select_stmt.query_map(params![actual_limit], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;

    for row in rows {
        let (id, name, path, ext) = row?;
        insert_stmt.execute(params![id, name, path, ext])?;
        indexed += 1;

        if indexed % 10_000 == 0 {
            println!("  Progress: {} / {} ({:.1}%)",
                format_number(indexed),
                format_number(actual_limit),
                indexed as f64 / actual_limit as f64 * 100.0
            );
        }
    }

    conn.execute("COMMIT", [])?;

    let elapsed = start.elapsed().as_secs_f64();
    let rate = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };

    Ok(BenchResult {
        strategy: "row-by-row".to_string(),
        files_indexed: indexed,
        elapsed_secs: elapsed,
        files_per_sec: rate,
        batch_size: None,
    })
}

/// Test the effect of different PRAGMA settings
fn bench_with_pragmas(db_path: &PathBuf, batch_size: u64, pragmas: &[(&str, &str)]) -> Result<BenchResult> {
    let pragma_desc: Vec<String> = pragmas.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
    println!("\n[PRAGMA-TEST] Testing with: {}", pragma_desc.join(", "));

    // Open fresh connection with specific pragmas
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;

    for (pragma, value) in pragmas {
        // Some pragmas return results, use pragma_update to ignore them
        let _ = conn.pragma_update(None, pragma, value);
    }

    let total_files = count_files(&conn)?;
    if total_files == 0 {
        return Ok(BenchResult {
            strategy: format!("pragma-{}", pragma_desc.join("-")),
            files_indexed: 0,
            elapsed_secs: 0.0,
            files_per_sec: 0.0,
            batch_size: Some(batch_size),
        });
    }

    // Clear FTS
    conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", [])?;

    let start = Instant::now();
    let mut indexed: u64 = 0;
    let mut last_id: i64 = 0;

    loop {
        // First get the max ID for this batch
        let batch_max_id: Option<i64> = conn.query_row(
            "SELECT MAX(id) FROM (SELECT id FROM files WHERE id > ?1 ORDER BY id LIMIT ?2)",
            params![last_id, batch_size as i64],
            |row| row.get(0),
        ).ok();

        let Some(batch_max) = batch_max_id else {
            break;
        };

        // Insert this batch
        let rows_affected = conn.execute(
            "INSERT INTO files_fts(rowid, name, path, extension)
             SELECT id, name, path, extension FROM files
             WHERE id > ?1 AND id <= ?2",
            params![last_id, batch_max],
        )?;

        if rows_affected == 0 {
            break;
        }

        last_id = batch_max;
        indexed += rows_affected as u64;
    }

    let elapsed = start.elapsed().as_secs_f64();
    let rate = if elapsed > 0.0 { indexed as f64 / elapsed } else { 0.0 };

    Ok(BenchResult {
        strategy: format!("pragma-{}", pragma_desc.join("-")),
        files_indexed: indexed,
        elapsed_secs: elapsed,
        files_per_sec: rate,
        batch_size: Some(batch_size),
    })
}

fn print_result(result: &BenchResult) {
    println!("\n{}", "=".repeat(65));
    println!("Strategy: {}", result.strategy);
    println!("{}", "-".repeat(65));
    println!("Files indexed:    {:>20}", format_number(result.files_indexed));
    println!("Time elapsed:     {:>20.2}s", result.elapsed_secs);
    println!("Files/sec:        {:>20.0}", result.files_per_sec);
    if let Some(batch) = result.batch_size {
        println!("Batch size:       {:>20}", format_number(batch));
    }
    println!("{}", "=".repeat(65));
}

fn print_comparison(results: &[BenchResult]) {
    println!("\n\n{}", "=".repeat(85));
    println!("COMPARISON SUMMARY - FTS INDEXING STRATEGIES");
    println!("{}", "=".repeat(85));
    println!("{:<40} {:>15} {:>15} {:>12}", "Strategy", "Files/sec", "Time (s)", "Batch");
    println!("{}", "-".repeat(85));

    let mut sorted = results.to_vec();
    sorted.sort_by(|a, b| b.files_per_sec.partial_cmp(&a.files_per_sec).unwrap());

    for (i, r) in sorted.iter().enumerate() {
        let marker = if i == 0 { " <-- BEST" } else { "" };
        let batch_str = r.batch_size.map(|b| format_number(b)).unwrap_or("-".to_string());
        println!(
            "{:<40} {:>15.0} {:>15.2} {:>12}{}",
            r.strategy,
            r.files_per_sec,
            r.elapsed_secs,
            batch_str,
            marker
        );
    }

    if sorted.len() >= 2 {
        let best = &sorted[0];
        let worst = sorted.last().unwrap();
        if best.files_per_sec > worst.files_per_sec && worst.files_per_sec > 0.0 {
            let improvement = best.files_per_sec / worst.files_per_sec;
            println!("\n>>> Best strategy is {:.1}x faster than worst", improvement);
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    println!("FTS Indexing Benchmark Tool for Prism");
    println!("{}", "=".repeat(50));

    // Parse arguments
    let mut db_path = get_db_path();
    let mut quick_mode = false;
    let mut repair_only = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                db_path = PathBuf::from(args.get(i + 1).expect("--db requires a path"));
                i += 2;
            }
            "--quick" => {
                quick_mode = true;
                i += 1;
            }
            "--repair" => {
                repair_only = true;
                i += 1;
            }
            "--help" | "-h" => {
                println!("\nUsage: fts_bench [options]");
                println!("\nOptions:");
                println!("  --db <path>     Path to Prism database (default: auto-detect)");
                println!("  --quick         Run quick benchmark (fewer tests)");
                println!("  --repair        Repair corrupt FTS index and exit");
                println!("  --help          Show this help");
                println!("\nDefault database location:");
                println!("  {}", get_db_path().display());
                return Ok(());
            }
            _ => {
                i += 1;
            }
        }
    }

    // Check if database exists
    if !db_path.exists() {
        eprintln!("ERROR: Database not found at: {}", db_path.display());
        eprintln!("\nRun a scan in Prism first to create the database.");
        eprintln!("Or specify a different path with --db <path>");
        return Ok(());
    }

    println!("\nDatabase: {}", db_path.display());

    // Open connection
    let conn = Connection::open(&db_path)?;

    // Get current state
    let total_files = count_files(&conn)?;
    let fts_entries = count_fts_entries(&conn)?;

    println!("Total files in DB: {}", format_number(total_files));
    println!("Current FTS entries: {}", format_number(fts_entries));

    if total_files == 0 {
        eprintln!("\nNo files in database. Run a scan first.");
        return Ok(());
    }

    // Check FTS health
    println!("\nChecking FTS index health...");
    let fts_healthy = check_fts_health(&conn);

    if !fts_healthy {
        println!("WARNING: FTS index is CORRUPT!");
        if repair_only {
            repair_fts_index(&conn)?;
            return Ok(());
        } else {
            println!("Attempting automatic repair before benchmark...");
            repair_fts_index(&conn)?;
        }
    } else {
        println!("FTS index is healthy.");
        if repair_only {
            println!("\nNo repair needed - FTS index is already healthy.");
            return Ok(());
        }
    }

    println!("\n{}", "#".repeat(70));
    println!("# Starting FTS Benchmark Tests");
    println!("# This will rebuild the FTS index multiple times");
    println!("{}", "#".repeat(70));

    let mut results: Vec<BenchResult> = Vec::new();

    // Test 0: FTS5 native rebuild command (should be fastest)
    let result = bench_fts_rebuild(&conn)?;
    print_result(&result);
    results.push(result);

    // Define batch sizes to test
    let batch_sizes: Vec<u64> = if quick_mode {
        vec![50_000]
    } else {
        vec![10_000, 25_000, 50_000, 100_000, 200_000]
    };

    // Test 1: Current batch INSERT...SELECT with different batch sizes
    for &batch_size in &batch_sizes {
        let result = bench_batch_insert_select(&conn, batch_size)?;
        print_result(&result);
        results.push(result);
    }

    if !quick_mode {
        // Test 2: Transaction-wrapped batches
        for &batch_size in &[50_000u64, 100_000] {
            let result = bench_transaction_batches(&conn, batch_size)?;
            print_result(&result);
            results.push(result);
        }

        // Test 3: Single transaction
        let result = bench_single_transaction(&conn, 100_000)?;
        print_result(&result);
        results.push(result);

        // Test 4: Row-by-row (baseline - limited to 50k rows for speed)
        let result = bench_row_by_row(&conn, 50_000)?;
        print_result(&result);
        results.push(result);
    }

    // Test 5: PRAGMA optimizations
    drop(conn); // Close connection before pragma tests

    let pragma_tests: Vec<Vec<(&str, &str)>> = if quick_mode {
        vec![
            vec![("synchronous", "OFF"), ("journal_mode", "MEMORY")],
        ]
    } else {
        vec![
            vec![("synchronous", "OFF")],
            vec![("journal_mode", "MEMORY")],
            vec![("synchronous", "OFF"), ("journal_mode", "MEMORY")],
            vec![("cache_size", "-100000")], // 100MB cache
            vec![("synchronous", "OFF"), ("journal_mode", "MEMORY"), ("cache_size", "-100000")],
        ]
    };

    for pragmas in pragma_tests {
        let result = bench_with_pragmas(&db_path, 50_000, &pragmas)?;
        print_result(&result);
        results.push(result);
    }

    // Print comparison
    print_comparison(&results);

    // Recommendations
    println!("\n{}", "=".repeat(85));
    println!("RECOMMENDATIONS");
    println!("{}", "=".repeat(85));

    let mut sorted = results.clone();
    sorted.sort_by(|a, b| b.files_per_sec.partial_cmp(&a.files_per_sec).unwrap());

    if let Some(best) = sorted.first() {
        println!("\n1. Best strategy: {}", best.strategy);
        println!("   - {:.0} files/sec", best.files_per_sec);
        println!("   - Would index {} files in {:.1}s",
            format_number(total_files),
            total_files as f64 / best.files_per_sec
        );

        if best.strategy.contains("pragma") {
            println!("\n2. The best strategy uses PRAGMA optimizations.");
            println!("   Consider applying these PRAGMAs before FTS indexing:");
            if best.strategy.contains("synchronous=OFF") {
                println!("   - PRAGMA synchronous = OFF (faster but risks data loss on crash)");
            }
            if best.strategy.contains("journal_mode=MEMORY") {
                println!("   - PRAGMA journal_mode = MEMORY (faster but risks data loss on crash)");
            }
            if best.strategy.contains("cache_size") {
                println!("   - PRAGMA cache_size = -100000 (100MB cache)");
            }
        }

        if let Some(batch) = best.batch_size {
            println!("\n3. Optimal batch size: {}", format_number(batch));
        }
    }

    // Restore FTS index
    println!("\n{}", "-".repeat(65));
    println!("Restoring FTS index with default settings...");
    let conn = Connection::open(&db_path)?;
    let _ = bench_batch_insert_select(&conn, 50_000)?;
    println!("FTS index restored.");

    Ok(())
}
