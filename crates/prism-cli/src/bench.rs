//! `prism-cli bench`: full indexing and no-change update of a folder or drive into a fresh
//! catalog in `%LOCALAPPDATA%\PrismDev\bench\`, with timings, catalog size per entry and
//! peak memory (NF2, NF3). Read-only on the scanned disk. The temporary catalog is removed
//! at the end through the sandbox guard (TESTING S2).

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use prism_core::model::unix_millis_now;
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::store::{self, Store};
use prism_win::process;

use crate::format::{human_size, human_time, thousands};
use crate::{WalkArgs, dev_data_dir, dev_volume, err, index_with_progress, normalize_root};

fn secs(d: Duration) -> String {
    format!("{:.1} s", d.as_secs_f64())
}

pub fn run(path: &Path, walk: &WalkArgs, out: Option<&Path>) -> Result<(), String> {
    let mount = normalize_root(path)?;
    let bench_dir = dev_data_dir()?.join("bench");
    std::fs::create_dir_all(&bench_dir).map_err(err)?;
    let db = bench_dir.join(format!("bench-{}.db", unix_millis_now()));
    let elevated = process::is_elevated();
    let threads = walk.enumerator().threads();

    let store = Store::open(&db).map_err(err)?;
    let volume = dev_volume(&store, &mount)?;
    eprintln!("full indexing of {mount} into {} ...", db.display());
    let full = index_with_progress(&store, volume, Path::new(&mount), walk)?;
    let peak_full = process::memory_usage().map_or(0, |m| m.peak_working_set);
    let (entries, db_pages) = store
        .read(move |c| Ok((store::entry_count(c, volume)?, store::database_size(c)?)))
        .map_err(err)?;

    eprintln!("update without changes ...");
    let update = index_with_progress(&store, volume, Path::new(&mount), walk)?;

    eprintln!("consistency check ...");
    let check_started = Instant::now();
    let problems = store.read(store::check_consistency).map_err(err)?;
    let check_time = check_started.elapsed();
    drop(store); // checkpoint and close
    let file_size = std::fs::metadata(&db).map_err(err)?.len();

    let s = &full.scan;
    let total = full.scan_time + full.reconcile_time;
    let rate = entries as f64 / total.as_secs_f64().max(0.001);
    let mut md = String::new();
    let _ = writeln!(md, "### `{mount}` — {}", human_time(unix_millis_now()));
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "Walker, {threads} threads, {} logical CPUs, process {}elevated, prism-cli {} ({}).",
        std::thread::available_parallelism().map_or(0, std::num::NonZero::get),
        if elevated { "" } else { "not " },
        prism_core::VERSION,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    let _ = writeln!(md);
    let _ = writeln!(md, "| Measure | Value |");
    let _ = writeln!(md, "|---|---|");
    let _ = writeln!(
        md,
        "| Entries | {} ({} files, {} folders) |",
        thousands(entries),
        thousands(s.files),
        thousands(s.dirs)
    );
    let _ = writeln!(
        md,
        "| Excluded / unreadable folders | {} / {} |",
        s.excluded, s.unreadable_dirs
    );
    let _ = writeln!(
        md,
        "| Full indexing | {} (enumeration {} + catalog writes {}), {} entries/s |",
        secs(total),
        secs(full.scan_time),
        secs(full.reconcile_time),
        thousands(rate as u64)
    );
    let _ = writeln!(
        md,
        "| Update without changes | {} (enumeration {} + comparison {}); {} updated |",
        secs(update.scan_time + update.reconcile_time),
        secs(update.scan_time),
        secs(update.reconcile_time),
        update.reconcile.updated
    );
    let _ = writeln!(
        md,
        "| Catalog size | {} on disk, {} B per entry (pages in use: {} B per entry) |",
        human_size(file_size),
        file_size / entries.max(1),
        db_pages / entries.max(1)
    );
    let _ = writeln!(
        md,
        "| Peak memory after full indexing | {} |",
        human_size(peak_full)
    );
    let _ = writeln!(
        md,
        "| Consistency check | {} in {} |",
        if problems.is_empty() {
            "ok".to_owned()
        } else {
            format!("{} problems", problems.len())
        },
        secs(check_time)
    );
    if !s.errors.is_empty() {
        let _ = writeln!(md);
        let _ = writeln!(md, "First unreadable folders:");
        for e in s.errors.iter().take(5) {
            let _ = writeln!(md, "- `{e}`");
        }
    }
    println!("{md}");
    for p in problems.iter().take(10) {
        eprintln!("problem: {p}");
    }

    if let Some(out) = out {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out)
            .map_err(err)?;
        writeln!(f, "{md}").map_err(err)?;
    }

    // Remove the temporary catalog, only inside the bench folder.
    let fs = GuardedFs::new(SandboxGuard::new([&bench_dir]).map_err(err)?);
    for suffix in ["", "-wal", "-shm"] {
        let p = bench_dir.join(format!(
            "{}{suffix}",
            db.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ));
        if p.exists() {
            fs.remove_file(&p).map_err(err)?;
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("{} consistency problems", problems.len()))
    }
}
