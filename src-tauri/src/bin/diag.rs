//! Diagnostic tool for Prism - tests drive detection and FTS indexing
//!
//! Run with: cargo run --bin diag
//! Or with real DB: cargo run --bin diag -- --real

use std::time::Instant;
use std::env;
use std::path::PathBuf;

fn main() {
    println!("=== Prism Diagnostic Tool ===\n");

    let args: Vec<String> = env::args().collect();
    let use_real_db = args.iter().any(|a| a == "--real");

    // Test 1: Drive Detection
    println!("--- TEST 1: Drive Detection ---");
    test_drive_detection();

    // Test 2: Database and FTS
    if use_real_db {
        println!("\n--- TEST 2: Real Database FTS Test ---");
        test_real_database_fts();
    } else {
        println!("\n--- TEST 2: Database & FTS Indexing (synthetic) ---");
        test_database_fts();
        println!("\nRun with --real to test on actual Prism database");
    }
}

fn test_drive_detection() {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use std::ptr;

        #[link(name = "kernel32")]
        extern "system" {
            fn GetLogicalDriveStringsW(buffer_length: u32, buffer: *mut u16) -> u32;
            fn GetDriveTypeW(root_path: *const u16) -> u32;
        }

        const DRIVE_UNKNOWN: u32 = 0;
        const DRIVE_NO_ROOT_DIR: u32 = 1;
        const DRIVE_REMOVABLE: u32 = 2;
        const DRIVE_FIXED: u32 = 3;
        const DRIVE_REMOTE: u32 = 4;
        const DRIVE_CDROM: u32 = 5;
        const DRIVE_RAMDISK: u32 = 6;

        // Test 1: GetLogicalDriveStringsW (drive letters only)
        println!("Method 1: GetLogicalDriveStringsW (mapped drives)");
        let mut buffer: [u16; 512] = [0; 512];
        let len = unsafe { GetLogicalDriveStringsW(buffer.len() as u32, buffer.as_mut_ptr()) };

        if len == 0 {
            println!("  ERROR: GetLogicalDriveStringsW failed!");
        } else {
            let mut start = 0;
            let mut drive_count = 0;

            for i in 0..len as usize {
                if buffer[i] == 0 {
                    if start < i {
                        let drive_path = OsString::from_wide(&buffer[start..i]);
                        let drive_str = drive_path.to_string_lossy().to_string();

                        let mut path_wide: Vec<u16> = drive_str.encode_utf16().collect();
                        path_wide.push(0);

                        let drive_type_raw = unsafe { GetDriveTypeW(path_wide.as_ptr()) };
                        let drive_type = match drive_type_raw {
                            DRIVE_UNKNOWN => "UNKNOWN",
                            DRIVE_NO_ROOT_DIR => "NO_ROOT_DIR",
                            DRIVE_REMOVABLE => "REMOVABLE",
                            DRIVE_FIXED => "FIXED (Local)",
                            DRIVE_REMOTE => "REMOTE (Network)",
                            DRIVE_CDROM => "CDROM",
                            DRIVE_RAMDISK => "RAMDISK",
                            _ => "OTHER",
                        };

                        drive_count += 1;
                        println!(
                            "  Drive {}: {} -> Type={} (code={})",
                            drive_count, drive_str, drive_type, drive_type_raw
                        );
                    }
                    start = i + 1;
                }
            }
            println!("  Total: {} drives", drive_count);
        }

        // Test 2: WNetEnumResource (network connections)
        println!("\nMethod 2: WNetEnumResourceW (network connections)");

        #[repr(C)]
        #[allow(non_snake_case)]
        struct NETRESOURCEW {
            dwScope: u32,
            dwType: u32,
            dwDisplayType: u32,
            dwUsage: u32,
            lpLocalName: *mut u16,
            lpRemoteName: *mut u16,
            lpComment: *mut u16,
            lpProvider: *mut u16,
        }

        #[link(name = "mpr")]
        extern "system" {
            fn WNetOpenEnumW(
                dwScope: u32, dwType: u32, dwUsage: u32,
                lpNetResource: *const NETRESOURCEW,
                lphEnum: *mut *mut std::ffi::c_void,
            ) -> u32;
            fn WNetEnumResourceW(
                hEnum: *mut std::ffi::c_void, lpcCount: *mut u32,
                lpBuffer: *mut u8, lpBufferSize: *mut u32,
            ) -> u32;
            fn WNetCloseEnum(hEnum: *mut std::ffi::c_void) -> u32;
        }

        const RESOURCE_CONNECTED: u32 = 0x00000001;
        const RESOURCE_REMEMBERED: u32 = 0x00000003;
        const RESOURCETYPE_DISK: u32 = 0x00000001;
        const NO_ERROR: u32 = 0;
        const ERROR_NO_MORE_ITEMS: u32 = 259;

        let mut network_count = 0;

        for (scope, scope_name) in [(RESOURCE_CONNECTED, "CONNECTED"), (RESOURCE_REMEMBERED, "REMEMBERED")] {
            let mut h_enum: *mut std::ffi::c_void = ptr::null_mut();

            let result = unsafe {
                WNetOpenEnumW(scope, RESOURCETYPE_DISK, 0, ptr::null(), &mut h_enum)
            };

            if result != NO_ERROR {
                println!("  {} scope: error {} opening enum", scope_name, result);
                continue;
            }

            let mut buf: Vec<u8> = vec![0u8; 16384];
            let mut found_in_scope = 0;

            loop {
                let mut count: u32 = 0xFFFFFFFF;
                let mut size: u32 = buf.len() as u32;

                let result = unsafe { WNetEnumResourceW(h_enum, &mut count, buf.as_mut_ptr(), &mut size) };

                if result == ERROR_NO_MORE_ITEMS { break; }
                if result != NO_ERROR {
                    println!("  {} scope: error {} enumerating", scope_name, result);
                    break;
                }

                let resources = unsafe {
                    std::slice::from_raw_parts(buf.as_ptr() as *const NETRESOURCEW, count as usize)
                };

                for res in resources {
                    if res.lpRemoteName.is_null() { continue; }

                    let remote = unsafe {
                        let len = (0..).find(|&i| *res.lpRemoteName.offset(i) == 0).unwrap_or(0);
                        OsString::from_wide(std::slice::from_raw_parts(res.lpRemoteName, len as usize))
                            .to_string_lossy().to_string()
                    };

                    let local = if !res.lpLocalName.is_null() {
                        unsafe {
                            let len = (0..).find(|&i| *res.lpLocalName.offset(i) == 0).unwrap_or(0);
                            OsString::from_wide(std::slice::from_raw_parts(res.lpLocalName, len as usize))
                                .to_string_lossy().to_string()
                        }
                    } else {
                        String::new()
                    };

                    found_in_scope += 1;
                    network_count += 1;
                    println!(
                        "  Network {}: {} -> {} ({})",
                        network_count,
                        if local.is_empty() { "(no letter)" } else { &local },
                        remote,
                        scope_name
                    );
                }
            }

            unsafe { WNetCloseEnum(h_enum) };
            if found_in_scope == 0 {
                println!("  {} scope: no connections found", scope_name);
            }
        }

        if network_count == 0 {
            println!("  No network connections found");
        } else {
            println!("  Total: {} network connections", network_count);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        println!("Drive detection test only available on Windows");
    }
}

fn test_database_fts() {
    use rusqlite::{Connection, params};

    // Create in-memory database
    let conn = Connection::open_in_memory().expect("Failed to open in-memory DB");

    // Create schema
    conn.execute_batch(
        "CREATE TABLE files (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL,
            name TEXT NOT NULL,
            extension TEXT,
            size INTEGER NOT NULL
        );
        CREATE VIRTUAL TABLE files_fts USING fts5(
            name, path, extension,
            content='files',
            content_rowid='id'
        );"
    ).expect("Failed to create schema");

    // Insert test data
    let test_sizes = [1000, 5000, 10000, 50000, 100000];

    for &size in &test_sizes {
        // Clear tables
        conn.execute("DELETE FROM files", []).unwrap();
        conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", []).unwrap();

        // Insert files
        println!("\nInserting {} test files...", size);
        let insert_start = Instant::now();

        {
            let tx = conn.unchecked_transaction().unwrap();
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO files (path, name, extension, size) VALUES (?1, ?2, ?3, ?4)"
                ).unwrap();

                for i in 0..size {
                    let path = format!("C:\\Test\\Folder{}\\file{}.txt", i / 100, i);
                    let name = format!("file{}.txt", i);
                    stmt.execute(params![path, name, "txt", 1000i64]).unwrap();
                }
            }
            tx.commit().unwrap();
        }

        let insert_elapsed = insert_start.elapsed();
        println!("  Insert: {:.2}s ({:.0} files/sec)",
            insert_elapsed.as_secs_f64(),
            size as f64 / insert_elapsed.as_secs_f64());

        // Test FTS indexing with different batch sizes
        for batch_size in [500, 1000, 2000, 5000] {
            // Clear FTS
            conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", []).unwrap();

            let fts_start = Instant::now();
            let mut indexed = 0u64;
            let mut last_id = 0i64;
            let mut iterations = 0;

            // Apply optimizations
            conn.execute_batch("PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;").ok();

            loop {
                // Simple approach: just use LIMIT and OFFSET equivalent
                let batch_max: Option<i64> = conn.query_row(
                    "SELECT MAX(id) FROM (SELECT id FROM files WHERE id > ?1 ORDER BY id LIMIT ?2)",
                    params![last_id, batch_size],
                    |row| row.get(0),
                ).ok().flatten();

                let Some(max_id) = batch_max else { break; };

                let rows = conn.execute(
                    "INSERT INTO files_fts(rowid, name, path, extension)
                     SELECT id, name, path, extension FROM files
                     WHERE id > ?1 AND id <= ?2",
                    params![last_id, max_id],
                ).unwrap();

                if rows == 0 { break; }

                last_id = max_id;
                indexed += rows as u64;
                iterations += 1;
            }

            // Restore
            conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA journal_mode = WAL;").ok();

            let fts_elapsed = fts_start.elapsed();
            let rate = indexed as f64 / fts_elapsed.as_secs_f64();

            println!("  FTS batch={}: {:.3}s, {} iterations, {:.0} files/sec",
                batch_size, fts_elapsed.as_secs_f64(), iterations, rate);
        }
    }

    println!("\n--- FTS Indexing Benchmark Complete ---");
    println!("If rates are good here but slow in app, the issue is event emission or UI blocking.");
}

fn test_real_database_fts() {
    use rusqlite::{Connection, params};

    // Find the Prism database
    let db_path = if cfg!(windows) {
        let appdata = env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata).join("com.prism.app").join("prism.db")
    } else {
        PathBuf::from("prism.db")
    };

    println!("Looking for database at: {:?}", db_path);

    if !db_path.exists() {
        println!("ERROR: Database not found! Run the app first to create it.");
        return;
    }

    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            println!("ERROR: Failed to open database: {}", e);
            return;
        }
    };

    // Get file count
    let file_count: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .unwrap_or(0);

    println!("Database contains {} files", file_count);

    if file_count == 0 {
        println!("No files in database. Run a scan first.");
        return;
    }

    // Check current FTS state
    let fts_count: i64 = conn.query_row("SELECT COUNT(*) FROM files_fts", [], |row| row.get(0))
        .unwrap_or(0);
    println!("FTS index contains {} entries", fts_count);

    println!("\nTesting FTS rebuild performance...");

    // Clear FTS
    println!("Clearing FTS index...");
    conn.execute("INSERT INTO files_fts(files_fts) VALUES('delete-all')", []).ok();

    // Apply optimizations
    conn.execute_batch("PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;").ok();

    let start = Instant::now();
    let mut indexed = 0u64;
    let mut last_id = 0i64;
    let mut last_report = Instant::now();
    const BATCH_SIZE: i64 = 10_000;

    loop {
        let rows = conn.execute(
            "INSERT INTO files_fts(rowid, name, path, extension)
             SELECT id, name, path, extension FROM files
             WHERE id > ?1
             ORDER BY id
             LIMIT ?2",
            params![last_id, BATCH_SIZE],
        ).unwrap_or(0);

        if rows == 0 { break; }

        // Get actual last ID
        let new_last_id: i64 = conn.query_row(
            "SELECT MAX(id) FROM files WHERE id > ?1 ORDER BY id LIMIT ?2",
            params![last_id, BATCH_SIZE],
            |row| row.get(0),
        ).unwrap_or(last_id);

        last_id = new_last_id;
        indexed += rows as u64;

        // Report every 500ms
        if last_report.elapsed().as_millis() >= 500 {
            let elapsed = start.elapsed().as_secs_f64();
            let rate = indexed as f64 / elapsed;
            let percent = (indexed as f64 / file_count as f64) * 100.0;
            println!(
                "  Progress: {}/{} ({:.1}%) - {:.0} files/sec",
                indexed, file_count, percent, rate
            );
            last_report = Instant::now();
        }
    }

    // Restore settings
    conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA journal_mode = WAL;").ok();

    let elapsed = start.elapsed();
    let rate = indexed as f64 / elapsed.as_secs_f64();

    println!("\n--- Results ---");
    println!("Indexed {} files in {:.2}s", indexed, elapsed.as_secs_f64());
    println!("Rate: {:.0} files/sec", rate);

    if rate > 100_000.0 {
        println!("\nFTS indexing is FAST. Issue is likely in event emission.");
    } else if rate > 10_000.0 {
        println!("\nFTS indexing is moderate. Check for lock contention.");
    } else {
        println!("\nFTS indexing is SLOW. Check disk I/O or database corruption.");
    }
}
