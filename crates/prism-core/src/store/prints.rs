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
    states(conn, volume, "image_prints")
}

/// Every non-empty file of a volume with its video print state; the caller keeps the videos.
///
/// # Errors
/// SQLite errors.
pub fn video_print_states(
    conn: &Connection,
    volume: VolumeId,
) -> Result<Vec<PrintState>, StoreError> {
    states(conn, volume, "video_prints")
}

/// The files of a volume `keep` accepts (by stored name and size) and how many of them have a
/// valid print in `table` (decoded or not), counted row by row: the progress of every volume is
/// counted every few minutes, and loading 3 million files to count them took memory and time
/// (REVIEW L31). `table` is one of the print tables of this module, never user input.
///
/// # Errors
/// SQLite errors.
pub fn count_prints(
    conn: &Connection,
    volume: VolumeId,
    table: &str,
    keep: &dyn Fn(&str, u64) -> bool,
) -> Result<(u64, u64), StoreError> {
    debug_assert!(table == "image_prints" || table == "video_prints");
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT e.name, e.size, \
                p.entry_id IS NOT NULL AND p.size = e.size AND p.modified_at IS e.modified_at \
         FROM entries e LEFT JOIN {table} p ON p.entry_id = e.id \
         WHERE e.volume_id = ?1 AND e.is_dir = 0 AND e.size > 0 AND {LOCAL_CONTENT}",
        LOCAL_CONTENT = super::hashes::LOCAL_CONTENT
    ))?;
    let mut rows = stmt.query([volume.0])?;
    let (mut total, mut done) = (0u64, 0u64);
    while let Some(r) = rows.next()? {
        let name = r.get_ref(0)?.as_str().unwrap_or_default();
        if keep(name, from_db(r.get(1)?)) {
            total += 1;
            done += u64::from(r.get::<_, bool>(2)?);
        }
    }
    Ok((total, done))
}

/// `table` is one of the print tables of this module, never user input.
fn states(conn: &Connection, volume: VolumeId, table: &str) -> Result<Vec<PrintState>, StoreError> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT e.id, e.parent_id, e.name, e.size, e.modified_at, \
                p.entry_id IS NOT NULL AND p.size = e.size AND p.modified_at IS e.modified_at \
         FROM entries e LEFT JOIN {table} p ON p.entry_id = e.id \
         WHERE e.volume_id = ?1 AND e.is_dir = 0 AND e.size > 0 AND {LOCAL_CONTENT} ORDER BY e.id",
        LOCAL_CONTENT = super::hashes::LOCAL_CONTENT
    ))?;
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

/// The print of a video, as computed for one state of the file (F12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoPrintRecord {
    pub entry: EntryId,
    pub size: u64,
    pub modified: Option<FileTime>,
    pub duration_ms: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// One print per sampled frame; `None` when the video could not be decoded.
    pub frames: Option<Vec<u64>>,
}

fn frames_blob(frames: &[u64]) -> Vec<u8> {
    frames.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn frames_of(blob: &[u8]) -> Vec<u64> {
    blob.chunks_exact(8)
        .filter_map(|c| <[u8; 8]>::try_from(c).ok())
        .map(u64::from_le_bytes)
        .collect()
}

/// Stores video prints, replacing what an entry had; entries removed meanwhile are skipped.
///
/// # Errors
/// SQLite errors.
pub fn set_video_prints(
    wtx: &mut WriteTx<'_>,
    records: &[VideoPrintRecord],
    computed_at: i64,
) -> Result<(), StoreError> {
    let mut stmt = wtx.conn().prepare_cached(
        "INSERT INTO video_prints \
           (entry_id, size, modified_at, duration_ms, width, height, frames, computed_at) \
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8 WHERE EXISTS (SELECT 1 FROM entries WHERE id = ?1) \
         ON CONFLICT (entry_id) DO UPDATE SET size = excluded.size, \
           modified_at = excluded.modified_at, duration_ms = excluded.duration_ms, \
           width = excluded.width, height = excluded.height, frames = excluded.frames, \
           computed_at = excluded.computed_at",
    )?;
    for r in records {
        stmt.execute(params![
            r.entry.0,
            to_db(r.size),
            r.modified.map(|t| t.0),
            r.duration_ms.map(to_db),
            r.width,
            r.height,
            r.frames.as_deref().map(frames_blob),
            computed_at
        ])?;
    }
    Ok(())
}

/// A decoded video of the catalog with its valid print.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoPrint {
    pub entry: EntryId,
    pub volume: VolumeId,
    pub duration_ms: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size: u64,
    pub frames: Vec<u64>,
}

/// The valid video prints of every volume, online or not.
///
/// # Errors
/// SQLite errors.
pub fn valid_video_prints(conn: &Connection) -> Result<Vec<VideoPrint>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.entry_id, e.volume_id, p.duration_ms, p.width, p.height, e.size, p.frames \
         FROM video_prints p JOIN entries e ON e.id = p.entry_id \
         WHERE p.frames IS NOT NULL AND p.duration_ms IS NOT NULL \
           AND p.size = e.size AND p.modified_at IS e.modified_at",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(VideoPrint {
            entry: EntryId(r.get(0)?),
            volume: VolumeId(r.get(1)?),
            duration_ms: from_db(r.get(2)?),
            width: r.get(3)?,
            height: r.get(4)?,
            size: from_db(r.get(5)?),
            frames: frames_of(&r.get::<_, Vec<u8>>(6)?),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
