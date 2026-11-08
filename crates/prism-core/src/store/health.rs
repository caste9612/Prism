//! History of the health of the physical disks (F17-AC2), and the physical disk of each volume
//! (REVIEW D64).

use std::collections::HashMap;

use rusqlite::{Connection, params};

use super::catalog::{from_db, to_db};
use super::{StoreError, WriteTx};
use crate::health::{HealthReading, HealthStatus};
use crate::model::VolumeId;

/// A stored reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthRecord {
    /// Milliseconds since 1970.
    pub at: i64,
    pub status: HealthStatus,
    pub temperature_c: Option<i32>,
    pub power_on_hours: Option<u64>,
    pub wear_percent: Option<u32>,
    pub reallocated: Option<u64>,
    pub pending: Option<u64>,
    pub media_errors: Option<u64>,
    pub available_spare: Option<u32>,
    pub reasons: Vec<String>,
}

impl HealthRecord {
    /// The values to compare with a new reading.
    #[must_use]
    pub fn reading(&self) -> HealthReading {
        HealthReading {
            status: Some(self.status),
            temperature_c: self.temperature_c,
            power_on_hours: self.power_on_hours,
            wear_percent: self.wear_percent,
            reallocated: self.reallocated,
            pending: self.pending,
            media_errors: self.media_errors,
            available_spare: self.available_spare,
            reasons: Vec::new(),
        }
    }
}

/// Stores a reading of disk `key` (readings without a status are not stored).
///
/// # Errors
/// SQLite errors.
pub fn add_health(
    wtx: &mut WriteTx<'_>,
    key: &str,
    at: i64,
    r: &HealthReading,
) -> Result<(), StoreError> {
    let Some(status) = r.status else {
        return Ok(());
    };
    wtx.conn()
        .prepare_cached(
            "INSERT OR REPLACE INTO disk_health (disk_key, at, status, temperature, \
               power_on_hours, wear, reallocated, pending, media_errors, spare, reasons) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?
        .execute(params![
            key,
            at,
            status.as_str(),
            r.temperature_c,
            r.power_on_hours.map(to_db),
            r.wear_percent,
            r.reallocated.map(to_db),
            r.pending.map(to_db),
            r.media_errors.map(to_db),
            r.available_spare,
            r.reasons.join(","),
        ])?;
    Ok(())
}

/// The latest readings of disk `key`, newest first.
///
/// # Errors
/// SQLite errors.
pub fn health_history(
    conn: &Connection,
    key: &str,
    limit: usize,
) -> Result<Vec<HealthRecord>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT at, status, temperature, power_on_hours, wear, reallocated, pending, \
                media_errors, spare, reasons \
         FROM disk_health WHERE disk_key = ?1 ORDER BY at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(
        params![key, i64::try_from(limit).unwrap_or(i64::MAX)],
        |r| {
            let status: String = r.get(1)?;
            let reasons: Option<String> = r.get(9)?;
            Ok(HealthRecord {
                at: r.get(0)?,
                status: HealthStatus::parse(&status).unwrap_or(HealthStatus::Warning),
                temperature_c: r.get(2)?,
                power_on_hours: r.get::<_, Option<i64>>(3)?.map(from_db),
                wear_percent: r.get(4)?,
                reallocated: r.get::<_, Option<i64>>(5)?.map(from_db),
                pending: r.get::<_, Option<i64>>(6)?.map(from_db),
                media_errors: r.get::<_, Option<i64>>(7)?.map(from_db),
                available_spare: r.get(8)?,
                reasons: reasons
                    .unwrap_or_default()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
            })
        },
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Removes readings older than `before` (milliseconds since 1970).
///
/// # Errors
/// SQLite errors.
pub fn prune_health(wtx: &mut WriteTx<'_>, before: i64) -> Result<usize, StoreError> {
    Ok(wtx
        .conn()
        .prepare_cached("DELETE FROM disk_health WHERE at < ?1")?
        .execute([before])?)
}

/// The physical disk ("model|serial") of the volumes for which it is known (REVIEW D64).
///
/// # Errors
/// SQLite errors.
pub fn volume_disks(conn: &Connection) -> Result<HashMap<VolumeId, String>, StoreError> {
    let mut stmt =
        conn.prepare_cached("SELECT id, disk_key FROM volumes WHERE disk_key IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((VolumeId(r.get(0)?), r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Records the physical disk of a volume, read while it is connected.
///
/// # Errors
/// SQLite errors.
pub fn set_volume_disk(
    wtx: &mut WriteTx<'_>,
    volume: VolumeId,
    key: &str,
) -> Result<(), StoreError> {
    wtx.conn()
        .prepare_cached("UPDATE volumes SET disk_key = ?2 WHERE id = ?1")?
        .execute(params![volume.0, key])?;
    Ok(())
}

/// What kind of disk a physical disk is (REVIEW D65).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskMedia {
    /// `ssd` or `hdd`.
    pub media: String,
    /// Rotation speed of a hard disk, when it says it.
    pub rpm: Option<u32>,
}

/// The kind of the physical disks for which it is known, by "model|serial".
///
/// # Errors
/// SQLite errors.
pub fn disk_media(conn: &Connection) -> Result<HashMap<String, DiskMedia>, StoreError> {
    let mut stmt = conn.prepare_cached("SELECT disk_key, media, rpm FROM disk_media")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get(0)?,
            DiskMedia {
                media: r.get(1)?,
                rpm: r.get(2)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Records the kind of a physical disk, read while it is connected.
///
/// # Errors
/// SQLite errors (also for a `media` other than `ssd` and `hdd`).
pub fn set_disk_media(
    wtx: &mut WriteTx<'_>,
    key: &str,
    media: &DiskMedia,
) -> Result<(), StoreError> {
    wtx.conn()
        .prepare_cached(
            "INSERT INTO disk_media (disk_key, media, rpm) VALUES (?1, ?2, ?3) \
             ON CONFLICT (disk_key) DO UPDATE SET media = excluded.media, rpm = excluded.rpm",
        )?
        .execute(params![key, media.media, media.rpm])?;
    Ok(())
}
