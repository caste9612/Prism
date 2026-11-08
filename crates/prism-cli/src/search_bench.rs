//! `prism-cli search-bench`: measures the two options of ADR-0010 on a real catalog (read-only
//! on the disks): the in-memory name index and an SQLite FTS5 trigram index built in a separate
//! file of the bench folder.

use std::path::Path;
use std::time::Instant;

use prism_core::names;
use prism_core::search::index::VolumeIndex;
use prism_core::store::{self, Store};

const QUERIES: &[&str] = &[
    "a",
    "e",
    "port",
    "report",
    ".dll",
    "setup",
    "windows",
    "prism",
    "readme.md",
    "zzqx",
    "microsoft",
    "png",
    "2024",
    "cache",
];

fn percentiles(mut ms: Vec<f64>) -> (f64, f64) {
    ms.sort_by(f64::total_cmp);
    let at = |q: f64| ms[((ms.len() as f64 - 1.0) * q).round() as usize];
    (at(0.5), at(0.95))
}

fn time<T>(runs: usize, mut f: impl FnMut() -> T) -> (f64, f64, T) {
    let started = Instant::now();
    let mut last = f();
    let mut ms = vec![started.elapsed().as_secs_f64() * 1000.0];
    for _ in 1..runs {
        let started = Instant::now();
        last = f();
        ms.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let (p50, p95) = percentiles(ms);
    (p50, p95, last)
}

pub fn run(store: &Store, bench_dir: &Path, copies: usize, fts: bool) -> Result<(), String> {
    let volumes = store.read(store::volumes).map_err(|e| e.to_string())?;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));

    // Option A: in-memory index.
    let started = Instant::now();
    let mut indexes = Vec::new();
    for _ in 0..copies.max(1) {
        for v in &volumes {
            let id = v.id;
            indexes.push(
                store
                    .read(move |c| VolumeIndex::build(c, id))
                    .map_err(|e| e.to_string())?,
            );
        }
    }
    let build_s = started.elapsed().as_secs_f64();
    let entries: usize = indexes.iter().map(VolumeIndex::len).sum();
    let memory: usize = indexes.iter().map(VolumeIndex::memory).sum();
    println!(
        "## In-memory index ({} copies, {entries} entries)\n\nbuild {build_s:.2} s, memory {:.1} MiB ({:.0} B/entry), {threads} threads\n",
        copies.max(1),
        memory as f64 / 1_048_576.0,
        memory as f64 / entries.max(1) as f64
    );
    println!(
        "| query | matches | 1 thread p50/p95 ms | {threads} threads p50/p95 ms | first 1000 p50/p95 ms |"
    );
    println!("|---|---|---|---|---|");
    for q in QUERIES {
        let needle = names::fold(q);
        let n = needle.as_bytes();
        let (a50, a95, count) = time(15, || {
            indexes
                .iter()
                .map(|ix| ix.find(n, usize::MAX).len())
                .sum::<usize>()
        });
        let (b50, b95, _) = time(15, || {
            indexes
                .iter()
                .map(|ix| ix.find_parallel(n, usize::MAX, threads).len())
                .sum::<usize>()
        });
        let (c50, c95, _) = time(15, || {
            let mut left = 1000usize;
            for ix in &indexes {
                if left == 0 {
                    break;
                }
                left -= ix.find_parallel(n, left, threads).len();
            }
        });
        println!(
            "| `{q}` | {count} | {a50:.1} / {a95:.1} | {b50:.1} / {b95:.1} | {c50:.1} / {c95:.1} |"
        );
    }

    if fts {
        // Option B: FTS5 trigram, in a separate database of the bench folder.
        let path = bench_dir.join("fts-trigram.db");
        let _ = std::fs::remove_file(&path);
        let src = store.path().to_path_buf();
        let conn = rusqlite::Connection::open(&path).map_err(|e| e.to_string())?;
        let started = Instant::now();
        conn.execute_batch(&format!(
            "ATTACH DATABASE '{}' AS src;
             CREATE VIRTUAL TABLE names USING fts5(name, tokenize = 'trigram');
             INSERT INTO names (rowid, name) SELECT id, name FROM src.entries;
             DETACH DATABASE src;",
            src.display().to_string().replace('\'', "''")
        ))
        .map_err(|e| e.to_string())?;
        let build = started.elapsed().as_secs_f64();
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM names", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        println!(
            "\n## FTS5 trigram ({rows} names)\n\nbuild {build:.2} s, file {:.1} MiB ({:.0} B/entry)\n",
            size as f64 / 1_048_576.0,
            size as f64 / rows.max(1) as f64
        );
        println!("| query | matches | count p50/p95 ms | first 1000 p50/p95 ms |");
        println!("|---|---|---|---|");
        for q in QUERIES.iter().filter(|q| q.chars().count() >= 3) {
            let phrase = format!("\"{}\"", q.replace('"', "\"\""));
            let (a50, a95, count) = time(15, || {
                conn.query_row(
                    "SELECT count(*) FROM names WHERE names MATCH ?1",
                    [&phrase],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(-1)
            });
            let (b50, b95, _) = time(15, || {
                let mut stmt = conn
                    .prepare_cached("SELECT rowid FROM names WHERE names MATCH ?1 LIMIT 1000")
                    .ok();
                stmt.as_mut()
                    .and_then(|s| {
                        s.query_map([&phrase], |r| r.get::<_, i64>(0))
                            .ok()
                            .map(Iterator::count)
                    })
                    .unwrap_or(0)
            });
            println!("| `{q}` | {count} | {a50:.1} / {a95:.1} | {b50:.1} / {b95:.1} |");
        }
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}
