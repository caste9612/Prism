//! Catalog queries (volumes, entries, folder aggregates) and volume-level writes.
//!
//! Read functions take a `&Connection`, so they work both on a read-only connection
//! ([`super::Store::read`]) and inside a write transaction ([`super::WriteTx::conn`]).

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{StoreError, StoreEvent, WriteTx};
use crate::model::{
    BusType, DirStats, EntryId, EntryMeta, EntryRecord, FileTime, UsnPosition, Volume, VolumeId,
    VolumeInfo, VolumeKind, VolumeLabels, VolumeStatus, unix_millis_now,
};
use crate::names;

// SQLite integers are signed 64-bit. Sizes never reach 2^63; NTFS file references use all 64
// bits and are stored bit-for-bit.
pub(crate) fn to_db(v: u64) -> i64 {
    v as i64
}

pub(crate) fn from_db(v: i64) -> u64 {
    v as u64
}

// ---------------------------------------------------------------------------------------------
// Volumes

const VOLUME_COLS: &str = "id, kind, fs_type, fs_serial, volume_guid, unc_path, os_label, \
    capacity, free_space, user_name, location, notes, color, status, last_mount, \
    first_seen_at, last_seen_at, last_indexed_at, bus_type, usn_journal_id, usn_next";

fn volume_from_row(row: &Row<'_>) -> rusqlite::Result<Volume> {
    let kind: String = row.get(1)?;
    let status: String = row.get(13)?;
    let bad = |col: usize, v: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            col,
            rusqlite::types::Type::Text,
            format!("invalid value {v:?}").into(),
        )
    };
    Ok(Volume {
        id: VolumeId(row.get(0)?),
        info: VolumeInfo {
            kind: VolumeKind::parse(&kind).ok_or_else(|| bad(1, &kind))?,
            fs_type: row.get(2)?,
            fs_serial: row.get::<_, Option<i64>>(3)?.map(from_db),
            volume_guid: row.get(4)?,
            unc_path: row.get(5)?,
            os_label: row.get(6)?,
            capacity: row.get::<_, Option<i64>>(7)?.map(from_db),
            free_space: row.get::<_, Option<i64>>(8)?.map(from_db),
            bus: row
                .get::<_, Option<String>>(18)?
                .as_deref()
                .and_then(BusType::parse),
            last_mount: row.get(14)?,
        },
        labels: VolumeLabels {
            name: row.get(9)?,
            location: row.get(10)?,
            notes: row.get(11)?,
            color: row.get(12)?,
        },
        status: VolumeStatus::parse(&status).ok_or_else(|| bad(13, &status))?,
        first_seen_at: row.get(15)?,
        last_seen_at: row.get(16)?,
        last_indexed_at: row.get(17)?,
        usn: match (
            row.get::<_, Option<i64>>(19)?,
            row.get::<_, Option<i64>>(20)?,
        ) {
            (Some(journal_id), Some(next_usn)) => Some(UsnPosition {
                journal_id: from_db(journal_id),
                next_usn,
            }),
            _ => None,
        },
    })
}

/// A volume by id.
///
/// # Errors
/// SQLite errors.
pub fn volume(conn: &Connection, id: VolumeId) -> Result<Option<Volume>, StoreError> {
    Ok(conn
        .prepare_cached(&format!("SELECT {VOLUME_COLS} FROM volumes WHERE id = ?1"))?
        .query_row([id.0], volume_from_row)
        .optional()?)
}

/// All volumes, in creation order.
///
/// # Errors
/// SQLite errors.
pub fn volumes(conn: &Connection) -> Result<Vec<Volume>, StoreError> {
    let mut stmt =
        conn.prepare_cached(&format!("SELECT {VOLUME_COLS} FROM volumes ORDER BY id"))?;
    let rows = stmt.query_map([], volume_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The volume last seen at `mount` (compared without case), if any. Used by the development
/// CLI; real volume matching uses identity keys (ADR-0004, T2).
///
/// # Errors
/// SQLite errors.
pub fn volume_by_mount(conn: &Connection, mount: &str) -> Result<Option<Volume>, StoreError> {
    Ok(volumes(conn)?.into_iter().find(|v| {
        v.info
            .last_mount
            .as_deref()
            .is_some_and(|m| names::eq_ignore_case(m, mount))
    }))
}

/// Adds a volume to the catalog.
///
/// # Errors
/// SQLite errors.
pub fn insert_volume(
    wtx: &mut WriteTx<'_>,
    info: &VolumeInfo,
    status: VolumeStatus,
) -> Result<VolumeId, StoreError> {
    let now = unix_millis_now();
    wtx.conn()
        .prepare_cached(
            "INSERT INTO volumes (kind, fs_type, fs_serial, volume_guid, unc_path, os_label, \
             capacity, free_space, status, last_mount, first_seen_at, last_seen_at, bus_type) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12)",
        )?
        .execute(params![
            info.kind.as_str(),
            info.fs_type,
            info.fs_serial.map(to_db),
            info.volume_guid,
            info.unc_path,
            info.os_label,
            info.capacity.map(to_db),
            info.free_space.map(to_db),
            status.as_str(),
            info.last_mount,
            now,
            info.bus.map(BusType::as_str),
        ])?;
    let id = VolumeId(wtx.conn().last_insert_rowid());
    wtx.notify(StoreEvent::VolumeChanged { volume: id });
    Ok(id)
}

/// Records what was detected for a connected volume: system information, current mount and
/// "last seen" time. Identity keys are refreshed too (they matched, or the user confirmed the
/// match). User labels and status are not touched.
///
/// # Errors
/// [`StoreError::NotFound`] if the volume does not exist; SQLite errors.
pub fn update_volume_info(
    wtx: &mut WriteTx<'_>,
    id: VolumeId,
    info: &VolumeInfo,
) -> Result<(), StoreError> {
    let changed = wtx
        .conn()
        .prepare_cached(
            "UPDATE volumes SET kind = ?2, fs_type = ?3, fs_serial = ?4, volume_guid = ?5, \
             unc_path = ?6, os_label = ?7, capacity = ?8, free_space = ?9, last_mount = ?10, \
             bus_type = ?11, last_seen_at = ?12 WHERE id = ?1",
        )?
        .execute(params![
            id.0,
            info.kind.as_str(),
            info.fs_type,
            info.fs_serial.map(to_db),
            info.volume_guid,
            info.unc_path,
            info.os_label,
            info.capacity.map(to_db),
            info.free_space.map(to_db),
            info.last_mount,
            info.bus.map(BusType::as_str),
            unix_millis_now(),
        ])?;
    if changed == 0 {
        return Err(StoreError::NotFound(format!("volume {id}")));
    }
    wtx.notify(StoreEvent::VolumeChanged { volume: id });
    Ok(())
}

/// Changes the status of a volume; `Indexed` also records the indexing time.
///
/// # Errors
/// [`StoreError::NotFound`] if the volume does not exist; SQLite errors.
pub fn set_volume_status(
    wtx: &mut WriteTx<'_>,
    id: VolumeId,
    status: VolumeStatus,
) -> Result<(), StoreError> {
    let changed = if status == VolumeStatus::Indexed {
        wtx.conn()
            .prepare_cached(
                "UPDATE volumes SET status = ?2, last_indexed_at = ?3, last_seen_at = ?3 \
                 WHERE id = ?1",
            )?
            .execute(params![id.0, status.as_str(), unix_millis_now()])?
    } else {
        wtx.conn()
            .prepare_cached("UPDATE volumes SET status = ?2 WHERE id = ?1")?
            .execute(params![id.0, status.as_str()])?
    };
    if changed == 0 {
        return Err(StoreError::NotFound(format!("volume {id}")));
    }
    wtx.notify(StoreEvent::VolumeChanged { volume: id });
    Ok(())
}

/// Records (or clears) the change-journal position up to which the volume's catalog is up to
/// date (ADR-0007). Cleared whenever the catalog can no longer be updated from the journal.
///
/// # Errors
/// [`StoreError::NotFound`] if the volume does not exist; SQLite errors.
pub fn set_usn_position(
    wtx: &mut WriteTx<'_>,
    id: VolumeId,
    position: Option<UsnPosition>,
) -> Result<(), StoreError> {
    let changed = wtx
        .conn()
        .prepare_cached("UPDATE volumes SET usn_journal_id = ?2, usn_next = ?3 WHERE id = ?1")?
        .execute(params![
            id.0,
            position.map(|p| to_db(p.journal_id)),
            position.map(|p| p.next_usn)
        ])?;
    if changed == 0 {
        return Err(StoreError::NotFound(format!("volume {id}")));
    }
    Ok(())
}

/// Sets the user labels of a volume (F03).
///
/// # Errors
/// [`StoreError::NotFound`] if the volume does not exist; SQLite errors.
pub fn set_volume_labels(
    wtx: &mut WriteTx<'_>,
    id: VolumeId,
    labels: &VolumeLabels,
) -> Result<(), StoreError> {
    let changed = wtx
        .conn()
        .prepare_cached(
            "UPDATE volumes SET user_name = ?2, location = ?3, notes = ?4, color = ?5 \
             WHERE id = ?1",
        )?
        .execute(params![
            id.0,
            labels.name,
            labels.location,
            labels.notes,
            labels.color
        ])?;
    if changed == 0 {
        return Err(StoreError::NotFound(format!("volume {id}")));
    }
    wtx.notify(StoreEvent::VolumeChanged { volume: id });
    Ok(())
}

/// Removes a volume and all its catalog data. Files on the disk are never touched (F01-AC5).
///
/// # Errors
/// [`StoreError::NotFound`] if the volume does not exist; SQLite errors.
pub fn remove_volume(wtx: &mut WriteTx<'_>, id: VolumeId) -> Result<(), StoreError> {
    let conn = wtx.conn();
    conn.prepare_cached("DELETE FROM entries WHERE volume_id = ?1")?
        .execute([id.0])?;
    let removed = conn
        .prepare_cached("DELETE FROM volumes WHERE id = ?1")?
        .execute([id.0])?;
    if removed == 0 {
        return Err(StoreError::NotFound(format!("volume {id}")));
    }
    wtx.notify(StoreEvent::VolumeRemoved { volume: id });
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Entries

pub(crate) const ENTRY_COLS: &str = "id, volume_id, parent_id, name, is_dir, size, alloc_size, \
    created_at, modified_at, attrs, file_ref";

pub(crate) fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<EntryRecord> {
    Ok(EntryRecord {
        id: EntryId(row.get(0)?),
        volume: VolumeId(row.get(1)?),
        parent: row.get::<_, Option<i64>>(2)?.map(EntryId),
        name: row.get(3)?,
        is_dir: row.get(4)?,
        meta: EntryMeta {
            size: from_db(row.get(5)?),
            alloc_size: row.get::<_, Option<i64>>(6)?.map(from_db),
            created: row.get::<_, Option<i64>>(7)?.map(FileTime),
            modified: row.get::<_, Option<i64>>(8)?.map(FileTime),
            attrs: u32::try_from(row.get::<_, i64>(9)?).unwrap_or(0),
            file_ref: row.get::<_, Option<i64>>(10)?.map(from_db),
        },
    })
}

/// An entry by id.
///
/// # Errors
/// SQLite errors.
pub fn entry(conn: &Connection, id: EntryId) -> Result<Option<EntryRecord>, StoreError> {
    Ok(conn
        .prepare_cached(&format!("SELECT {ENTRY_COLS} FROM entries WHERE id = ?1"))?
        .query_row([id.0], entry_from_row)
        .optional()?)
}

/// The root entry of a volume, if the volume has been indexed.
///
/// # Errors
/// SQLite errors.
pub fn root_entry(conn: &Connection, volume: VolumeId) -> Result<Option<EntryRecord>, StoreError> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {ENTRY_COLS} FROM entries WHERE parent_id IS NULL AND volume_id = ?1"
        ))?
        .query_row([volume.0], entry_from_row)
        .optional()?)
}

/// The children of a folder, ordered by name.
///
/// # Errors
/// SQLite errors.
pub fn children(conn: &Connection, parent: EntryId) -> Result<Vec<EntryRecord>, StoreError> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {ENTRY_COLS} FROM entries WHERE parent_id = ?1 ORDER BY name"
    ))?;
    let rows = stmt.query_map([parent.0], entry_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The child of `parent` named `name`: exact match first, then without case (Windows
/// semantics, for paths typed by a user).
///
/// # Errors
/// SQLite errors.
pub fn child_by_name(
    conn: &Connection,
    parent: EntryId,
    name: &str,
) -> Result<Option<EntryRecord>, StoreError> {
    let exact = conn
        .prepare_cached(&format!(
            "SELECT {ENTRY_COLS} FROM entries WHERE parent_id = ?1 AND name = ?2"
        ))?
        .query_row(params![parent.0, name], entry_from_row)
        .optional()?;
    if exact.is_some() {
        return Ok(exact);
    }
    Ok(children(conn, parent)?
        .into_iter()
        .find(|c| names::eq_ignore_case(&c.name, name)))
}

/// Entries of a volume with the given file reference (several for NTFS hard links).
///
/// There is no index on file references (ADR-0017): this scans the volume's entries. Use it
/// for single lookups in tests and diagnostics; for many lookups build a [`file_ref_map`].
///
/// # Errors
/// SQLite errors.
pub fn entries_by_ref(
    conn: &Connection,
    volume: VolumeId,
    file_ref: u64,
) -> Result<Vec<EntryRecord>, StoreError> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {ENTRY_COLS} FROM entries WHERE volume_id = ?1 AND file_ref = ?2"
    ))?;
    let rows = stmt.query_map(params![volume.0, to_db(file_ref)], entry_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Every (file reference, entry) pair of a volume, sorted by reference, for fast lookups by
/// reference (moves during reconciliation, USN journal). About 16 bytes per entry; loading
/// one million entries takes about 0.2 s (ADR-0017).
///
/// # Errors
/// SQLite errors.
pub fn file_ref_map(conn: &Connection, volume: VolumeId) -> Result<FileRefMap, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT file_ref, id FROM entries WHERE volume_id = ?1 AND file_ref IS NOT NULL",
    )?;
    let rows = stmt.query_map([volume.0], |r| Ok((from_db(r.get(0)?), EntryId(r.get(1)?))))?;
    let mut pairs: Vec<(u64, EntryId)> = rows.collect::<Result<_, _>>()?;
    pairs.sort_unstable();
    Ok(FileRefMap { pairs })
}

/// See [`file_ref_map`].
#[derive(Clone, Debug, Default)]
pub struct FileRefMap {
    pairs: Vec<(u64, EntryId)>,
}

impl FileRefMap {
    /// Entries with this file reference (several for hard links), in id order.
    #[must_use]
    pub fn get(&self, file_ref: u64) -> &[(u64, EntryId)] {
        let start = self.pairs.partition_point(|(r, _)| *r < file_ref);
        let end = self.pairs.partition_point(|(r, _)| *r <= file_ref);
        &self.pairs[start..end]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

/// Whether the volume has any entry.
///
/// # Errors
/// SQLite errors.
pub fn volume_has_entries(conn: &Connection, volume: VolumeId) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached("SELECT EXISTS (SELECT 1 FROM entries WHERE volume_id = ?1)")?
        .query_row([volume.0], |r| r.get(0))?)
}

/// The ids from `id` up to the root of its volume, starting with `id` itself.
///
/// # Errors
/// [`StoreError::NotFound`] if `id` does not exist; [`StoreError::Corrupt`] on a cycle.
pub fn ancestors_and_self(conn: &Connection, id: EntryId) -> Result<Vec<EntryId>, StoreError> {
    let mut stmt = conn.prepare_cached("SELECT parent_id FROM entries WHERE id = ?1")?;
    let mut chain = vec![id];
    let mut current = id;
    loop {
        let parent: Option<Option<i64>> = stmt.query_row([current.0], |r| r.get(0)).optional()?;
        match parent {
            None => return Err(StoreError::NotFound(format!("entry {current}"))),
            Some(None) => return Ok(chain),
            Some(Some(p)) => {
                current = EntryId(p);
                if chain.len() > 100_000 || chain.contains(&current) {
                    return Err(StoreError::Corrupt(format!("cycle above entry {id}")));
                }
                chain.push(current);
            }
        }
    }
}

/// Path of an entry relative to the root of its volume, components separated by `\`
/// (empty for the root). Names are in stored form (see [`crate::names`]).
///
/// # Errors
/// [`StoreError::NotFound`] if the entry does not exist.
pub fn relative_path(conn: &Connection, id: EntryId) -> Result<String, StoreError> {
    let mut stmt = conn.prepare_cached("SELECT parent_id, name FROM entries WHERE id = ?1")?;
    let mut parts: Vec<String> = Vec::new();
    let mut current = Some(id);
    while let Some(cur) = current {
        let (parent, name): (Option<i64>, String) = stmt
            .query_row([cur.0], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("entry {cur}")))?;
        if parent.is_some() {
            parts.push(name);
        }
        current = parent.map(EntryId);
        if parts.len() > 100_000 {
            return Err(StoreError::Corrupt(format!("cycle above entry {id}")));
        }
    }
    parts.reverse();
    Ok(parts.join("\\"))
}

/// Resolves a path relative to the volume root (components separated by `\` or `/`, compared
/// exactly first and then without case) to an entry.
///
/// # Errors
/// SQLite errors.
pub fn resolve_relative_path(
    conn: &Connection,
    volume: VolumeId,
    rel: &str,
) -> Result<Option<EntryRecord>, StoreError> {
    let Some(mut current) = root_entry(conn, volume)? else {
        return Ok(None);
    };
    for part in rel.split(['\\', '/']).filter(|p| !p.is_empty()) {
        match child_by_name(conn, current.id, part)? {
            Some(child) => current = child,
            None => return Ok(None),
        }
    }
    Ok(Some(current))
}

/// Aggregates of a folder (F04-AC4).
///
/// # Errors
/// SQLite errors.
pub fn dir_stats(conn: &Connection, id: EntryId) -> Result<Option<DirStats>, StoreError> {
    Ok(conn
        .prepare_cached(
            "SELECT total_size, total_alloc, file_count, dir_count FROM dir_stats \
             WHERE entry_id = ?1",
        )?
        .query_row([id.0], |r| {
            Ok(DirStats {
                total_size: from_db(r.get(0)?),
                total_alloc: from_db(r.get(1)?),
                file_count: from_db(r.get(2)?),
                dir_count: from_db(r.get(3)?),
            })
        })
        .optional()?)
}

/// Number of entries of a volume.
///
/// # Errors
/// SQLite errors.
pub fn entry_count(conn: &Connection, volume: VolumeId) -> Result<u64, StoreError> {
    let n: i64 = conn
        .prepare_cached("SELECT count(*) FROM entries WHERE volume_id = ?1")?
        .query_row([volume.0], |r| r.get(0))?;
    Ok(from_db(n))
}

/// Size of the database (pages in use times page size), excluding the WAL.
///
/// # Errors
/// SQLite errors.
pub fn database_size(conn: &Connection) -> Result<u64, StoreError> {
    let pages: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let free: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    Ok(from_db((pages - free) * size))
}
