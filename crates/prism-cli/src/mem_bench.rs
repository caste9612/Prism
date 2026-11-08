//! `prism-cli mem-bench`: the memory of Prism at rest (NF3), reproduced without the interface:
//! the catalog is opened and every search index is built as the app does at startup
//! (`LiveIndexes::maintain`), then the memory of the process is reported, before and after the
//! SQLite caches are given back (`Store::release_memory`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use prism_core::search::live::LiveIndexes;
use prism_core::store::{self, Store};

fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn report(what: &str) {
    match prism_win::process::memory_usage() {
        Some(m) => println!(
            "{what:<28} private {:>7.1} MiB  working set {:>7.1} MiB  peak {:>7.1} MiB",
            mib(m.private_bytes),
            mib(m.working_set),
            mib(m.peak_working_set)
        ),
        None => println!("{what:<28} memory not available"),
    }
}

pub fn run(store: &Store) -> Result<(), String> {
    let err = |e: store::StoreError| e.to_string();
    let (volumes, entries) = store
        .read(|c| {
            let mut volumes = 0;
            for v in store::volumes(c)? {
                if store::volume_has_entries(c, v.id)? {
                    volumes += 1;
                }
            }
            let entries: i64 = c.query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?;
            Ok((volumes, entries))
        })
        .map_err(err)?;
    println!(
        "catalog  {}: {entries} entries in {volumes} volumes",
        store.path().display()
    );
    report("catalog open");

    let live = Arc::new(LiveIndexes::default());
    let stop = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let worker = {
        let (live, stop, store) = (Arc::clone(&live), Arc::clone(&stop), store.clone());
        std::thread::spawn(move || live.maintain(&store, Duration::from_secs(3600), &stop))
    };
    while live.snapshot().len() < volumes {
        std::thread::sleep(Duration::from_millis(100));
    }
    let index: usize = live.snapshot().iter().map(|ix| ix.memory()).sum();
    println!(
        "indexes built in {:.1} s: {:.1} MiB ({:.0} B/entry)",
        started.elapsed().as_secs_f64(),
        mib(index as u64),
        index as f64 / entries.max(1) as f64
    );
    report("indexes built");
    std::thread::sleep(Duration::from_secs(2));
    report("at rest (2 s later)");
    store.release_memory();
    std::thread::sleep(Duration::from_secs(1));
    report("SQLite caches given back");

    stop.store(true, Ordering::Relaxed);
    worker
        .join()
        .map_err(|_| "index thread panicked".to_owned())?;
    Ok(())
}
