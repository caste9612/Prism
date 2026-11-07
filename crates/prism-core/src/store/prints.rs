//! Perceptual prints of the images in the catalog (F11): stored with the size and modification
//! time they were computed for, and read only when both still match the entry.

use rusqlite::{Connection, params};

use super::catalog::{from_db, to_db};
use super::{StoreError, WriteTx};
use crate::model::{EntryId, FileTime, VolumeId};

/// The print of an image, as computed for one state of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintRecord {
    pub entry: EntryId,
    pub size: u64,
    pub modified: Option<FileTime>,
    /// `None` when the file could not be decoded.
    pub dhash: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Stores prints, replacing what an entry had; entries removed meanwhile are skipped.
///
/// # Errors
/// SQLite errors.
pub fn set_prints(
    wtx: &mut WriteTx<'_>,
    records: &[PrintRecord],
    computed_at: i64,
) -> Result<(), StoreError> {
    let mut stmt = wtx.conn().prepare_cached(
        "INSERT INTO image_prints (entry_id, size, modified_at, dhash, width, height, computed_at) \
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7 WHERE EXISTS (SELECT 1 FROM entries WHERE id = ?1) \
         ON CONFLICT (entry_id) DO UPDATE SET size = excluded.size, \
           modified_at = excluded.modified_at, dhash = excluded.dhash, width = excluded.width, \
           height = excluded.height, computed_at = excluded.computed_at",
    )?;
    for r in records {
        stmt.execute(params![
            r.entry.0,
            to_db(r.size),
            r.modified.map(|t| t.0),
            r.dhash.map(|h| h.cast_signed()),
            r.width,
            r.height,
            computed_at
        ])?;
    }
    Ok(())
}

/// A file of a volume and whether it has a valid print (decoded or not).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintState {
    pub entry: EntryId,
    pub parent: Option<EntryId>,
    /// Stored name.
    pub name: String,
    pub size: u64,
    pub modified: Option<FileTime>,
    pub printed: bool,
}

/// Every non-empty file of a volume with its print state; the caller keeps the images.
///
/// # Errors
/// SQLite errors.
pub fn print_states(conn: &Connection, volume: VolumeId) -> Result<Vec<PrintState>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT e.id, e.parent_id, e.name, e.size, e.modified_at, \
                p.entry_id IS NOT NULL AND p.size = e.size AND p.modified_at IS e.modified_at \
         FROM entries e LEFT JOIN image_prints p ON p.entry_id = e.id \
         WHERE e.volume_id = ?1 AND e.is_dir = 0 AND e.size > 0 ORDER BY e.id",
    )?;
    let rows = stmt.query_map([volume.0], |r| {
        Ok(PrintState {
            entry: EntryId(r.get(0)?),
            parent: r.get::<_, Option<i64>>(1)?.map(EntryId),
            name: r.get(2)?,
            size: from_db(r.get(3)?),
            modified: r.get::<_, Option<i64>>(4)?.map(FileTime),
            printed: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// A decoded image of the catalog with its valid print.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImagePrint {
    pub entry: EntryId,
    pub volume: VolumeId,
    pub dhash: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size: u64,
}

/// The valid prints of every volume, online or not.
///
/// # Errors
/// SQLite errors.
pub fn valid_prints(conn: &Connection) -> Result<Vec<ImagePrint>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.entry_id, e.volume_id, p.dhash, p.width, p.height, e.size \
         FROM image_prints p JOIN entries e ON e.id = p.entry_id \
         WHERE p.dhash IS NOT NULL AND p.size = e.size AND p.modified_at IS e.modified_at",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ImagePrint {
            entry: EntryId(r.get(0)?),
            volume: VolumeId(r.get(1)?),
            dhash: r.get::<_, i64>(2)?.cast_unsigned(),
            width: r.get(3)?,
            height: r.get(4)?,
            size: from_db(r.get(5)?),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
