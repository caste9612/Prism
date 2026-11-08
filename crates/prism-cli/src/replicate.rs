//! `prism-cli replicate`: adds offline copies of the volumes of a development catalog until it
//! holds a given number of entries, to measure size and memory on large catalogs (NF2, NF3).
//! Only the catalog file changes; the copies have no volume GUID and a made-up serial number,
//! so they never match a connected disk and stay "not connected".

use std::path::Path;

use prism_core::store::{Store, StoreError};
use rusqlite::Connection;

/// Columns of `table`, in order.
fn columns(conn: &Connection, table: &str) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

/// Copies volume `source` once (copy number `copy`) and returns the entries added.
fn copy_volume(store: &Store, source: i64, copy: i64) -> Result<i64, StoreError> {
    let (min_id, max_id, count): (i64, i64, i64) = store.read(move |c| {
        Ok(c.query_row(
            "SELECT coalesce(min(id), 0), coalesce(max(id), 0), count(*) FROM entries \
             WHERE volume_id = ?1",
            [source],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    })?;
    if count == 0 {
        return Ok(0);
    }
    let span =
        u32::try_from(max_id - min_id + 1).map_err(|_| StoreError::Corrupt("id span".into()))?;
    // New ids from the store's own allocator, so they never collide with later inserts.
    let offset = store.allocate_entry_ids(span).0 - min_id;
    store.write(move |tx| {
        let conn = tx.conn();
        let vol_cols = columns(conn, "volumes")?;
        let values: Vec<String> = vol_cols
            .iter()
            .filter(|c| c.as_str() != "id")
            .map(|c| match c.as_str() {
                // Made-up identity: negative serial, no GUID, no network path, no letter.
                "fs_serial" => format!("-({copy} * 1000000 + id)"),
                "volume_guid" | "unc_path" | "last_mount" | "usn_journal_id" | "usn_next" => {
                    "NULL".to_owned()
                }
                "user_name" => format!(
                    "coalesce(user_name, os_label, last_mount, 'Volume') || ' (copy {copy})'"
                ),
                "status" => "'indexed'".to_owned(),
                other => other.to_owned(),
            })
            .collect();
        let names: Vec<&str> = vol_cols
            .iter()
            .map(String::as_str)
            .filter(|c| *c != "id")
            .collect();
        conn.execute(
            &format!(
                "INSERT INTO volumes ({}) SELECT {} FROM volumes WHERE id = ?1",
                names.join(", "),
                values.join(", ")
            ),
            [source],
        )?;
        let volume = conn.last_insert_rowid();

        let entry_cols = columns(conn, "entries")?;
        let values: Vec<String> = entry_cols
            .iter()
            .map(|c| match c.as_str() {
                "id" => format!("id + {offset}"),
                "volume_id" => volume.to_string(),
                "parent_id" => format!("parent_id + {offset}"),
                other => other.to_owned(),
            })
            .collect();
        conn.execute(
            &format!(
                "INSERT INTO entries ({}) SELECT {} FROM entries WHERE volume_id = ?1 ORDER BY id",
                entry_cols.join(", "),
                values.join(", ")
            ),
            [source],
        )?;

        let stat_cols = columns(conn, "dir_stats")?;
        let values: Vec<String> = stat_cols
            .iter()
            .map(|c| match c.as_str() {
                "entry_id" => format!("d.entry_id + {offset}"),
                other => format!("d.{other}"),
            })
            .collect();
        conn.execute(
            &format!(
                "INSERT INTO dir_stats ({}) SELECT {} FROM dir_stats d \
                 JOIN entries e ON e.id = d.entry_id WHERE e.volume_id = ?1",
                stat_cols.join(", "),
                values.join(", ")
            ),
            [source],
        )?;
        Ok(())
    })?;
    Ok(count)
}

/// Adds copies of the volumes, largest first, until the catalog holds at least `target` entries.
pub fn run(store: &Store, dev_dir: &Path, target: u64) -> Result<(), String> {
    let target = i64::try_from(target).unwrap_or(i64::MAX);
    let path = std::path::absolute(store.path()).map_err(|e| e.to_string())?;
    let dev = std::path::absolute(dev_dir).map_err(|e| e.to_string())?;
    if !path.starts_with(&dev) {
        return Err(format!(
            "replicate only changes development catalogs, inside {}",
            dev.display()
        ));
    }
    let err = |e: StoreError| e.to_string();
    let (mut total, sources): (i64, Vec<(i64, i64)>) = store
        .read(|c| {
            let total: i64 = c.query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?;
            let mut stmt = c.prepare(
                "SELECT volume_id, count(*) FROM entries GROUP BY volume_id ORDER BY 2 DESC",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok((total, rows))
        })
        .map_err(err)?;
    if sources.is_empty() {
        return Err("the catalog has no entries to copy".into());
    }
    println!("catalog  {}: {total} entries", path.display());
    let mut copy = 1;
    while total < target {
        for &(source, _) in &sources {
            if total >= target {
                break;
            }
            let started = std::time::Instant::now();
            let added = copy_volume(store, source, copy).map_err(err)?;
            total += added;
            println!(
                "  copy {copy} of volume {source}: {added} entries in {:.1} s, {total} in all",
                started.elapsed().as_secs_f64()
            );
        }
        copy += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WalkArgs, scan};
    use prism_core::store;

    #[test]
    fn copies_are_consistent_offline_and_only_in_development_catalogs() {
        let dev = tempfile::tempdir().unwrap();
        let disk = dev.path().join("disk");
        std::fs::create_dir_all(disk.join("a").join("b")).unwrap();
        std::fs::write(disk.join("a").join("one.txt"), b"one").unwrap();
        std::fs::write(disk.join("a").join("b").join("two.txt"), b"two two").unwrap();
        let store = Store::open(&dev.path().join("catalog.db")).unwrap();
        let walk = WalkArgs {
            mft: false,
            threads: Some(1),
            no_default_exclusions: false,
            exclude_dirs: Vec::new(),
            exclude_paths: Vec::new(),
        };
        scan(&store, &disk, &walk).unwrap();
        let count = |s: &Store| {
            s.read(|c| Ok(c.query_row("SELECT count(*) FROM entries", [], |r| r.get::<_, i64>(0))?))
                .unwrap()
        };
        let before = count(&store);
        assert!(before >= 5, "root, two folders, two files");

        // Outside the development folder nothing changes.
        let other = tempfile::tempdir().unwrap();
        assert!(run(&store, other.path(), 1_000).is_err());
        assert_eq!(count(&store), before);

        run(&store, dev.path(), u64::try_from(before * 3).unwrap()).unwrap();
        assert_eq!(count(&store), before * 3);
        assert!(store.read(store::check_consistency).unwrap().is_empty());
        let volumes = store.read(store::volumes).unwrap();
        assert_eq!(volumes.len(), 3);
        let original = &volumes[0];
        // Every volume has its own serial number: no copy can be taken for another disk.
        let serials: std::collections::HashSet<_> =
            volumes.iter().map(|v| v.info.fs_serial).collect();
        assert_eq!(serials.len(), volumes.len());
        for copy in &volumes[1..] {
            assert_eq!(copy.info.volume_guid, None);
            assert_eq!(copy.info.last_mount, None);
            assert_ne!(copy.info.fs_serial, original.info.fs_serial);
            assert!(
                copy.labels
                    .name
                    .as_deref()
                    .is_some_and(|n| n.contains("(copy "))
            );
            let totals = |v: &prism_core::model::Volume| {
                store
                    .read(|c| {
                        let root = store::root_entry(c, v.id)?.unwrap();
                        Ok(store::dir_stats(c, root.id)?.unwrap())
                    })
                    .unwrap()
            };
            assert_eq!(totals(copy), totals(original));
        }
    }
}
