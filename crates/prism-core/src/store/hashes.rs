//! Content hashes in the catalog (F06, ADR-0011): stored with the size and modification time
//! they were computed for, and read only through queries that check both against the entry.

use rusqlite::{Connection, OptionalExtension, params};

use super::catalog::{from_db, to_db};
use super::{StoreError, WriteTx};
use crate::model::{EntryId, FileTime, VolumeId};

/// A BLAKE3 digest.
pub type Digest = [u8; 32];

/// The hashes of an entry, as computed for one state of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashRecord {
    pub entry: EntryId,
    pub size: u64,
    pub modified: Option<FileTime>,
    pub quick: Option<Digest>,
    pub full: Option<Digest>,
}

fn digest(v: Option<Vec<u8>>) -> Option<Digest> {
    v.and_then(|b| b.try_into().ok())
}

/// Stores hashes, replacing what an entry had (`id` order does not matter).
///
/// # Errors
/// SQLite errors.
pub fn set_hashes(
    wtx: &mut WriteTx<'_>,
    records: &[HashRecord],
    computed_at: i64,
) -> Result<(), StoreError> {
    let mut stmt = wtx.conn().prepare_cached(
        "INSERT INTO content_hashes (entry_id, size, modified_at, quick, full, computed_at) \
         SELECT ?1, ?2, ?3, ?4, ?5, ?6 WHERE EXISTS (SELECT 1 FROM entries WHERE id = ?1) \
         ON CONFLICT (entry_id) DO UPDATE SET size = excluded.size, \
           modified_at = excluded.modified_at, quick = excluded.quick, full = excluded.full, \
           computed_at = excluded.computed_at",
    )?;
    for r in records {
        stmt.execute(params![
            r.entry.0,
            to_db(r.size),
            r.modified.map(|t| t.0),
            r.quick.as_ref().map(<[u8; 32]>::as_slice),
            r.full.as_ref().map(<[u8; 32]>::as_slice),
            computed_at
        ])?;
    }
    Ok(())
}

/// The hashes of an entry if they are still valid for it (same size and modification time).
///
/// # Errors
/// SQLite errors.
pub fn valid_hashes(conn: &Connection, entry: EntryId) -> Result<Option<HashRecord>, StoreError> {
    Ok(conn
        .prepare_cached(
            "SELECT h.size, h.modified_at, h.quick, h.full FROM content_hashes h \
             JOIN entries e ON e.id = h.entry_id \
             WHERE h.entry_id = ?1 AND e.size = h.size AND e.modified_at IS h.modified_at",
        )?
        .query_row([entry.0], |r| {
            Ok(HashRecord {
                entry,
                size: from_db(r.get(0)?),
                modified: r.get::<_, Option<i64>>(1)?.map(FileTime),
                quick: digest(r.get(2)?),
                full: digest(r.get(3)?),
            })
        })
        .optional()?)
}

/// A file to hash and the hashes it already has (valid ones only), read from the catalog when
/// its turn comes: a run keeps only the ids of the files still to do (NF3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashState {
    pub entry: EntryId,
    pub parent: Option<EntryId>,
    /// Stored name (see `names`).
    pub name: String,
    pub size: u64,
    pub modified: Option<FileTime>,
    pub quick: Option<Digest>,
    pub full: Option<Digest>,
    /// Whether the file has a valid image print (decoded or not): an image read whole for its
    /// hashes is printed from the same bytes when it has none (REVIEW L31).
    pub printed: bool,
}

/// The quick hash of `e` stored in `h`, if still valid for its size and time.
const VALID_QUICK: &str =
    "CASE WHEN h.size = e.size AND h.modified_at IS e.modified_at THEN h.quick END";
/// Files whose content is on the disk: not cloud placeholders (OneDrive and the like) or
/// offline files. Reading those would download them, or fail: they are never analysed
/// (attributes `FILE_ATTRIBUTE_OFFLINE` 0x1000, `RECALL_ON_OPEN` 0x40000 and
/// `RECALL_ON_DATA_ACCESS` 0x400000).
pub(crate) const LOCAL_CONTENT: &str = "(coalesce(e.attrs, 0) & 4460544) = 0";
/// The full hash of `e` stored in `h`, if still valid for its size and time.
const VALID_FULL: &str =
    "CASE WHEN h.size = e.size AND h.modified_at IS e.modified_at THEN h.full END";

/// The file `entry` with its valid hashes, or `None` if it is no longer a non-empty file of the
/// catalog.
///
/// # Errors
/// SQLite errors.
pub fn hash_state(conn: &Connection, entry: EntryId) -> Result<Option<HashState>, StoreError> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT e.id, e.size, e.modified_at, e.parent_id, e.name, {VALID_QUICK}, {VALID_FULL}, \
                p.entry_id IS NOT NULL AND p.size = e.size AND p.modified_at IS e.modified_at \
         FROM entries e LEFT JOIN content_hashes h ON h.entry_id = e.id \
           LEFT JOIN image_prints p ON p.entry_id = e.id \
         WHERE e.id = ?1 AND e.is_dir = 0 AND e.size > 0"
    ))?;
    Ok(stmt
        .query_row([entry.0], |r| {
            Ok(HashState {
                entry: EntryId(r.get(0)?),
                size: from_db(r.get(1)?),
                modified: r.get::<_, Option<i64>>(2)?.map(FileTime),
                parent: r.get::<_, Option<i64>>(3)?.map(EntryId),
                name: r.get(4)?,
                quick: digest(r.get(5)?),
                full: digest(r.get(6)?),
                printed: r.get(7)?,
            })
        })
        .optional()?)
}

/// The files of a volume that need a quick hash, largest first (F06-AC4): no valid quick hash
/// yet, and a size held by another non-empty file of the catalog (only those can have
/// duplicates).
///
/// # Errors
/// SQLite errors.
pub fn quick_work(conn: &Connection, volume: VolumeId) -> Result<Vec<EntryId>, StoreError> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT e.id FROM entries e LEFT JOIN content_hashes h ON h.entry_id = e.id \
         WHERE e.volume_id = ?1 AND e.is_dir = 0 AND e.size > 0 AND {LOCAL_CONTENT} \
           AND ({VALID_QUICK}) IS NULL \
           AND e.size IN (SELECT size FROM entries WHERE is_dir = 0 AND size > 0 \
                          GROUP BY size HAVING count(*) > 1) \
         ORDER BY e.size DESC, e.id"
    ))?;
    let rows = stmt.query_map([volume.0], |r| Ok(EntryId(r.get(0)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Calls `f` with every file of a volume that has no valid full hash yet: its id, size and
/// valid quick hash, in id order.
///
/// # Errors
/// SQLite errors.
pub fn full_work(
    conn: &Connection,
    volume: VolumeId,
    mut f: impl FnMut(EntryId, u64, Option<Digest>),
) -> Result<(), StoreError> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT e.id, e.size, {VALID_QUICK} \
         FROM entries e LEFT JOIN content_hashes h ON h.entry_id = e.id \
         WHERE e.volume_id = ?1 AND e.is_dir = 0 AND e.size > 0 AND {LOCAL_CONTENT} \
           AND ({VALID_FULL}) IS NULL \
         ORDER BY e.id"
    ))?;
    let mut rows = stmt.query([volume.0])?;
    while let Some(r) = rows.next()? {
        f(EntryId(r.get(0)?), from_db(r.get(1)?), digest(r.get(2)?));
    }
    Ok(())
}

/// (size, quick hash) pairs held by more than one file with a valid quick hash: only those
/// need a full hash to find duplicates.
///
/// # Errors
/// SQLite errors.
pub fn shared_quick(
    conn: &Connection,
) -> Result<std::collections::HashSet<(u64, Digest)>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT h.size, h.quick FROM content_hashes h JOIN entries e ON e.id = h.entry_id \
         WHERE h.quick IS NOT NULL AND e.size = h.size AND e.modified_at IS h.modified_at \
         GROUP BY h.size, h.quick HAVING count(*) > 1",
    )?;
    let rows = stmt.query_map([], |r| Ok((from_db(r.get(0)?), digest(r.get(1)?))))?;
    let mut out = std::collections::HashSet::new();
    for row in rows {
        if let (size, Some(q)) = row? {
            out.insert((size, q));
        }
    }
    Ok(out)
}

/// Files of a volume and how many have a valid full hash (F06-AC5).
///
/// # Errors
/// SQLite errors.
pub fn hash_progress(conn: &Connection, volume: VolumeId) -> Result<(u64, u64), StoreError> {
    let (files, hashed): (i64, i64) = conn
        .prepare_cached(&format!(
            "SELECT count(*), count(CASE WHEN h.full IS NOT NULL AND h.size = e.size \
                                     AND h.modified_at IS e.modified_at THEN 1 END) \
             FROM entries e LEFT JOIN content_hashes h ON h.entry_id = e.id \
             WHERE e.volume_id = ?1 AND e.is_dir = 0 AND e.size > 0 AND {LOCAL_CONTENT}"
        ))?
        .query_row([volume.0], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok((from_db(files), from_db(hashed)))
}

/// One file of a group of exact duplicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuplicateFile {
    pub entry: EntryId,
    pub volume: VolumeId,
    pub size: u64,
    pub full: Digest,
}

/// Files with the same size and the same valid full hash as at least one other file, on any
/// volume, online or not (F10-AC1), largest first, grouped by consecutive (size, hash).
///
/// # Errors
/// SQLite errors.
pub fn duplicate_files(conn: &Connection, min_size: u64) -> Result<Vec<DuplicateFile>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "WITH v AS ( \
             SELECT h.entry_id, e.volume_id, h.size, h.full FROM content_hashes h \
             JOIN entries e ON e.id = h.entry_id \
             WHERE h.full IS NOT NULL AND h.size >= ?1 \
               AND e.size = h.size AND e.modified_at IS h.modified_at), \
         g AS (SELECT size, full FROM v GROUP BY size, full HAVING count(*) > 1) \
         SELECT v.entry_id, v.volume_id, v.size, v.full FROM v \
         JOIN g ON g.size = v.size AND g.full = v.full \
         ORDER BY v.size DESC, v.full, v.entry_id",
    )?;
    let rows = stmt.query_map([to_db(min_size)], |r| {
        Ok((
            EntryId(r.get(0)?),
            VolumeId(r.get(1)?),
            from_db(r.get(2)?),
            digest(r.get(3)?),
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (entry, volume, size, full) = row?;
        if let Some(full) = full {
            out.push(DuplicateFile {
                entry,
                volume,
                size,
                full,
            });
        }
    }
    Ok(out)
}

/// A file below a folder, for the backup checks (F13, F14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtreeFile {
    pub entry: EntryId,
    /// Path relative to the folder, stored names separated by `\`.
    pub rel: String,
    pub size: u64,
    pub modified: Option<FileTime>,
    /// The full hash, when valid for the current size and date (ADR-0011).
    pub full: Option<Digest>,
}

/// Every file below `root` (at any depth), ordered by relative path.
///
/// # Errors
/// SQLite errors.
pub fn subtree_files(conn: &Connection, root: EntryId) -> Result<Vec<SubtreeFile>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "WITH RECURSIVE sub(id, rel) AS ( \
             SELECT ?1, '' \
             UNION ALL \
             SELECT e.id, CASE WHEN sub.rel = '' THEN e.name ELSE sub.rel || '\\' || e.name END \
             FROM entries e JOIN sub ON e.parent_id = sub.id WHERE e.is_dir = 1) \
         SELECT f.id, CASE WHEN sub.rel = '' THEN f.name ELSE sub.rel || '\\' || f.name END, \
                f.size, f.modified_at, h.full \
         FROM sub JOIN entries f ON f.parent_id = sub.id AND f.is_dir = 0 \
         LEFT JOIN content_hashes h ON h.entry_id = f.id AND h.size = f.size \
                                    AND h.modified_at IS f.modified_at \
         ORDER BY 2",
    )?;
    let rows = stmt.query_map([root.0], |r| {
        Ok(SubtreeFile {
            entry: EntryId(r.get(0)?),
            rel: r.get(1)?,
            size: from_db(r.get(2)?),
            modified: r.get::<_, Option<i64>>(3)?.map(FileTime),
            full: digest(r.get(4)?),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
