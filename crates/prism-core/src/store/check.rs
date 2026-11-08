//! Consistency check and canonical snapshot of the catalog, used by the tests (equivalence
//! between update and full indexing, F05-AC3) and by diagnostics.

use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;

use super::StoreError;
use super::catalog::{ENTRY_COLS, entry_from_row, from_db};
use crate::model::{DirStats, EntryMeta, EntryRecord, VolumeId};

/// One entry of a [`snapshot`], identified by its relative path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub is_dir: bool,
    pub meta: EntryMeta,
    pub stats: Option<DirStats>,
}

fn load_entries(
    conn: &Connection,
    volume: Option<VolumeId>,
) -> Result<Vec<EntryRecord>, StoreError> {
    let (sql, param) = match volume {
        Some(v) => (
            format!("SELECT {ENTRY_COLS} FROM entries WHERE volume_id = ?1"),
            v.0,
        ),
        None => (format!("SELECT {ENTRY_COLS} FROM entries WHERE ?1 = ?1"), 0),
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([param], entry_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn load_stats(conn: &Connection) -> Result<HashMap<i64, DirStats>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT entry_id, total_size, total_alloc, file_count, dir_count FROM dir_stats",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            DirStats {
                total_size: from_db(r.get(1)?),
                total_alloc: from_db(r.get(2)?),
                file_count: from_db(r.get(3)?),
                dir_count: from_db(r.get(4)?),
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The content of a volume keyed by relative path (components joined with `\`, root = ""),
/// independent of entry ids. Two catalogs describe the same disk state when their snapshots
/// are equal.
///
/// # Errors
/// SQLite errors; [`StoreError::Corrupt`] if an entry is unreachable from the root.
pub fn snapshot(
    conn: &Connection,
    volume: VolumeId,
) -> Result<BTreeMap<String, SnapshotEntry>, StoreError> {
    let entries = load_entries(conn, Some(volume))?;
    let stats = load_stats(conn)?;
    let by_id: HashMap<i64, &EntryRecord> = entries.iter().map(|e| (e.id.0, e)).collect();
    let mut paths: HashMap<i64, String> = HashMap::with_capacity(entries.len());
    let mut out = BTreeMap::new();
    for e in &entries {
        let path = path_of(e.id.0, &by_id, &mut paths)?;
        out.insert(
            path,
            SnapshotEntry {
                is_dir: e.is_dir,
                meta: e.meta,
                stats: stats.get(&e.id.0).copied(),
            },
        );
    }
    Ok(out)
}

fn path_of(
    id: i64,
    by_id: &HashMap<i64, &EntryRecord>,
    memo: &mut HashMap<i64, String>,
) -> Result<String, StoreError> {
    if let Some(p) = memo.get(&id) {
        return Ok(p.clone());
    }
    // Walk up to the first memoized ancestor (or the root), then build down.
    let mut chain = Vec::new();
    let mut current = id;
    let base = loop {
        if let Some(p) = memo.get(&current) {
            break p.clone();
        }
        let e = by_id
            .get(&current)
            .ok_or_else(|| StoreError::Corrupt(format!("entry {current} is unreachable")))?;
        match e.parent {
            None => {
                memo.insert(current, String::new());
                break String::new();
            }
            Some(p) => {
                chain.push(current);
                current = p.0;
            }
        }
        if chain.len() > by_id.len() {
            return Err(StoreError::Corrupt(format!("cycle above entry {id}")));
        }
    };
    let mut path = base;
    for cid in chain.iter().rev() {
        let e = by_id
            .get(cid)
            .ok_or_else(|| StoreError::Corrupt(format!("entry {cid} is unreachable")))?;
        if !path.is_empty() {
            path.push('\\');
        }
        path.push_str(&e.name);
        memo.insert(*cid, path.clone());
    }
    Ok(path)
}

/// Checks the structural invariants of the catalog and returns a description of every
/// problem found (empty when consistent):
///
/// - every entry has a parent folder on the same volume, except one root per volume, named "";
/// - every folder has aggregates and no file has them;
/// - aggregates equal a full recomputation from the entries;
/// - every entry is reachable from its volume root (no cycles);
/// - no temporary name left by an interrupted reconciliation.
///
/// # Errors
/// SQLite errors.
pub fn check_consistency(conn: &Connection) -> Result<Vec<String>, StoreError> {
    let entries = load_entries(conn, None)?;
    let stats = load_stats(conn)?;
    let mut problems = Vec::new();
    let by_id: HashMap<i64, &EntryRecord> = entries.iter().map(|e| (e.id.0, e)).collect();
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut roots: HashMap<i64, Vec<i64>> = HashMap::new();

    for e in &entries {
        match e.parent {
            None => {
                roots.entry(e.volume.0).or_default().push(e.id.0);
                if !e.name.is_empty() {
                    problems.push(format!("root {} has a non-empty name", e.id));
                }
            }
            Some(p) => match by_id.get(&p.0) {
                None => problems.push(format!("entry {} has a missing parent {p}", e.id)),
                Some(parent) if !parent.is_dir => {
                    problems.push(format!("entry {} has a file as parent", e.id));
                }
                Some(parent) if parent.volume != e.volume => {
                    problems.push(format!("entry {} has a parent on another volume", e.id));
                }
                Some(_) => children.entry(p.0).or_default().push(e.id.0),
            },
        }
        if e.name.starts_with(crate::reconcile::PARKED_PREFIX) {
            problems.push(format!("entry {} has a temporary name", e.id));
        }
        match (e.is_dir, stats.contains_key(&e.id.0)) {
            (true, false) => problems.push(format!("folder {} has no aggregates", e.id)),
            (false, true) => problems.push(format!("file {} has aggregates", e.id)),
            _ => {}
        }
    }
    for (volume, r) in &roots {
        if r.len() > 1 {
            problems.push(format!("volume {volume} has {} roots", r.len()));
        }
    }

    // Recompute every aggregate from scratch (iterative post-order from each root).
    let mut computed: HashMap<i64, DirStats> = HashMap::new();
    let mut reached = 0usize;
    for root in roots.values().flatten() {
        let mut stack: Vec<(i64, bool)> = vec![(*root, false)];
        while let Some((id, expanded)) = stack.pop() {
            let Some(e) = by_id.get(&id) else { continue };
            if !e.is_dir {
                reached += 1;
                continue;
            }
            if !expanded {
                reached += 1;
                stack.push((id, true));
                for c in children.get(&id).into_iter().flatten() {
                    stack.push((*c, false));
                }
                continue;
            }
            let mut s = DirStats::default();
            for c in children.get(&id).into_iter().flatten() {
                let Some(child) = by_id.get(c) else { continue };
                if child.is_dir {
                    let cs = computed.get(c).copied().unwrap_or_default();
                    s.total_size += cs.total_size;
                    s.total_alloc += cs.total_alloc;
                    s.file_count += cs.file_count;
                    s.dir_count += cs.dir_count + 1;
                } else {
                    s.total_size += child.meta.size;
                    s.total_alloc += child.meta.alloc_size.unwrap_or(child.meta.size);
                    s.file_count += 1;
                }
            }
            computed.insert(id, s);
        }
    }
    if reached != entries.len() {
        problems.push(format!(
            "{} entries are unreachable from their volume root",
            entries.len() - reached
        ));
    }
    let mut mismatches: Vec<_> = computed
        .iter()
        .filter(|(id, s)| stats.get(id).is_some_and(|stored| stored != *s))
        .map(|(id, s)| (*id, *s))
        .collect();
    mismatches.sort_unstable_by_key(|(id, _)| *id);
    for (id, s) in mismatches.into_iter().take(20) {
        problems.push(format!(
            "folder {id}: stored aggregates {:?}, expected {s:?}",
            stats.get(&id)
        ));
    }
    Ok(problems)
}
