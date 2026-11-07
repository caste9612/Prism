//! Write operations on entries, applied by the writer thread, and the incremental update of
//! folder aggregates (`dir_stats`, F04-AC4).

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

use super::StoreError;
use super::catalog::to_db;
use crate::model::{EntryId, EntryMeta, VolumeId};

/// One change to the entries of a volume. Produced by the reconciler (ADR-0006).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteOp {
    /// Adds an entry. The id comes from [`super::Store::allocate_entry_ids`]; the parent must
    /// already exist.
    Insert {
        id: EntryId,
        volume: VolumeId,
        parent: Option<EntryId>,
        name: String,
        is_dir: bool,
        meta: EntryMeta,
    },
    /// Replaces the metadata of an entry.
    Update { id: EntryId, meta: EntryMeta },
    /// Renames and/or moves an entry (with its whole subtree), optionally with new metadata.
    Move {
        id: EntryId,
        parent: EntryId,
        name: String,
        meta: Option<EntryMeta>,
    },
    /// Removes an entry and its whole subtree. Removing a missing entry is not an error.
    DeleteSubtree { id: EntryId },
    /// Recomputes the aggregates of these folders and of all their ancestors, bottom-up.
    /// Must come after the operations that changed their children.
    RecomputeDirStats { dirs: Vec<EntryId> },
}

pub(crate) fn apply(conn: &Connection, op: &WriteOp) -> Result<(), StoreError> {
    match op {
        WriteOp::Insert {
            id,
            volume,
            parent,
            name,
            is_dir,
            meta,
        } => {
            conn.prepare_cached(
                "INSERT INTO entries (id, volume_id, parent_id, name, is_dir, size, alloc_size, \
                 created_at, modified_at, attrs, file_ref) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?
            .execute(params![
                id.0,
                volume.0,
                parent.map(|p| p.0),
                name,
                is_dir,
                to_db(meta.size),
                meta.alloc_size.map(to_db),
                meta.created.map(|t| t.0),
                meta.modified.map(|t| t.0),
                i64::from(meta.attrs),
                meta.file_ref.map(to_db),
            ])?;
            if *is_dir {
                conn.prepare_cached(
                    "INSERT INTO dir_stats (entry_id, total_size, total_alloc, file_count, \
                     dir_count) VALUES (?1, 0, 0, 0, 0)",
                )?
                .execute([id.0])?;
            }
        }
        WriteOp::Update { id, meta } => update_meta(conn, *id, meta)?,
        WriteOp::Move {
            id,
            parent,
            name,
            meta,
        } => {
            conn.prepare_cached("UPDATE entries SET parent_id = ?2, name = ?3 WHERE id = ?1")?
                .execute(params![id.0, parent.0, name])?;
            if let Some(meta) = meta {
                update_meta(conn, *id, meta)?;
            }
        }
        WriteOp::DeleteSubtree { id } => {
            // A recursive CTE instead of ON DELETE CASCADE: cascades are limited in depth,
            // folder trees are not. The self-referencing foreign key is checked at the end of
            // the statement, when the whole subtree is gone.
            conn.prepare_cached(
                "WITH RECURSIVE sub(id) AS ( \
                     SELECT ?1 \
                     UNION ALL \
                     SELECT e.id FROM entries e JOIN sub ON e.parent_id = sub.id \
                 ) \
                 DELETE FROM entries WHERE id IN (SELECT id FROM sub)",
            )?
            .execute([id.0])?;
        }
        WriteOp::RecomputeDirStats { dirs } => recompute_dir_stats(conn, dirs)?,
    }
    Ok(())
}

fn update_meta(conn: &Connection, id: EntryId, meta: &EntryMeta) -> Result<(), StoreError> {
    conn.prepare_cached(
        "UPDATE entries SET size = ?2, alloc_size = ?3, created_at = ?4, modified_at = ?5, \
         attrs = ?6, file_ref = ?7 WHERE id = ?1",
    )?
    .execute(params![
        id.0,
        to_db(meta.size),
        meta.alloc_size.map(to_db),
        meta.created.map(|t| t.0),
        meta.modified.map(|t| t.0),
        i64::from(meta.attrs),
        meta.file_ref.map(to_db),
    ])?;
    Ok(())
}

/// Recomputes `dir_stats` for `seeds` and all their ancestors, deepest first, each folder from
/// its direct children (files: their sizes; folders: their already updated aggregates).
/// Seeds that no longer exist or are files are ignored.
pub(crate) fn recompute_dir_stats(conn: &Connection, seeds: &[EntryId]) -> Result<(), StoreError> {
    let mut parent_stmt =
        conn.prepare_cached("SELECT parent_id, is_dir FROM entries WHERE id = ?1")?;
    // Depth of every folder to recompute (root = 0).
    let mut depth: HashMap<i64, u32> = HashMap::with_capacity(seeds.len() * 2);
    let mut chain: Vec<i64> = Vec::new();

    for seed in seeds {
        if depth.contains_key(&seed.0) {
            continue;
        }
        chain.clear();
        let mut current = seed.0;
        // Depth of the first already-known folder above the chain; None if the chain reaches
        // the root.
        let known_above: Option<u32> = loop {
            if let Some(&d) = depth.get(&current) {
                break Some(d);
            }
            let row: Option<(Option<i64>, bool)> = parent_stmt
                .query_row([current], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            let Some((parent, is_dir)) = row else {
                if chain.is_empty() {
                    break None; // the seed itself was deleted
                }
                return Err(StoreError::Corrupt(format!(
                    "entry {current} is referenced as a parent but does not exist"
                )));
            };
            if !is_dir {
                if chain.is_empty() {
                    break None; // a file seed: nothing to recompute for it
                }
                return Err(StoreError::Corrupt(format!(
                    "file {current} is the parent of another entry"
                )));
            }
            chain.push(current);
            if chain.len() > 100_000 {
                return Err(StoreError::Corrupt(format!("cycle above entry {}", seed.0)));
            }
            match parent {
                Some(p) => current = p,
                None => break None,
            }
        };
        let mut d = known_above.map_or(0, |k| k + 1);
        for id in chain.iter().rev() {
            depth.insert(*id, d);
            d += 1;
        }
    }

    let mut order: Vec<(u32, i64)> = depth.into_iter().map(|(id, d)| (d, id)).collect();
    order.sort_unstable_by(|a, b| b.cmp(a));

    let mut aggregate = conn.prepare_cached(
        "SELECT \
           coalesce(sum(CASE WHEN e.is_dir THEN coalesce(s.total_size, 0) ELSE e.size END), 0), \
           coalesce(sum(CASE WHEN e.is_dir THEN coalesce(s.total_alloc, 0) \
                             ELSE coalesce(e.alloc_size, e.size) END), 0), \
           coalesce(sum(CASE WHEN e.is_dir THEN coalesce(s.file_count, 0) ELSE 1 END), 0), \
           coalesce(sum(CASE WHEN e.is_dir THEN coalesce(s.dir_count, 0) + 1 ELSE 0 END), 0) \
         FROM entries e LEFT JOIN dir_stats s ON s.entry_id = e.id \
         WHERE e.parent_id = ?1",
    )?;
    let mut upsert = conn.prepare_cached(
        "INSERT INTO dir_stats (entry_id, total_size, total_alloc, file_count, dir_count) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT (entry_id) DO UPDATE SET total_size = excluded.total_size, \
           total_alloc = excluded.total_alloc, file_count = excluded.file_count, \
           dir_count = excluded.dir_count",
    )?;
    for (_, id) in order {
        let (size, alloc, files, dirs): (i64, i64, i64, i64) =
            aggregate.query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        upsert.execute(params![id, size, alloc, files, dirs])?;
    }
    Ok(())
}
