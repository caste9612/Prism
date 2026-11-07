//! Schema migrations, embedded in the binary and tracked with `PRAGMA user_version` (ADR-0008).
//!
//! Rules: a migration is never edited once committed; a schema change is a new file appended
//! to [`MIGRATIONS`]. The tests apply every migration to an empty database and to a database
//! at each previous version, and check that the result is identical.

use rusqlite::{Connection, TransactionBehavior};

use super::StoreError;

/// All migrations, in order. Index `i` brings the schema from version `i` to `i + 1`.
pub(crate) const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_initial.sql"),
    include_str!("migrations/0002_volume_bus.sql"),
    include_str!("migrations/0003_entries_volume_index.sql"),
    include_str!("migrations/0004_actions_log.sql"),
    include_str!("migrations/0005_content_hashes.sql"),
    include_str!("migrations/0006_image_prints.sql"),
    include_str!("migrations/0007_video_prints.sql"),
];

/// Schema version produced by this build.
#[must_use]
pub fn latest_version() -> i64 {
    i64::try_from(MIGRATIONS.len()).unwrap_or(i64::MAX)
}

pub(crate) fn current_version(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Applies the pending migrations, each in its own transaction.
pub(crate) fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    migrate_to(conn, latest_version())
}

pub(crate) fn migrate_to(conn: &mut Connection, target: i64) -> Result<(), StoreError> {
    let found = current_version(conn)?;
    if found > latest_version() {
        return Err(StoreError::TooNew {
            found,
            supported: latest_version(),
        });
    }
    for version in found..target {
        let Some(sql) = usize::try_from(version)
            .ok()
            .and_then(|i| MIGRATIONS.get(i))
        else {
            break;
        };
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version + 1)?;
        tx.commit()?;
        tracing::info!(from = version, to = version + 1, "catalog schema migrated");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_of(conn: &Connection) -> Vec<(String, String, String)> {
        let mut stmt = conn
            .prepare(
                "SELECT type, name, coalesce(sql, '') FROM sqlite_schema \
                 WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn migrates_empty_database_to_latest() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), latest_version());
        let names: Vec<String> = schema_of(&conn).into_iter().map(|s| s.1).collect();
        for table in ["volumes", "entries", "dir_stats", "settings"] {
            assert!(names.iter().any(|n| n == table), "missing table {table}");
        }
        // Applying again is a no-op.
        migrate(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), latest_version());
    }

    #[test]
    fn every_previous_version_migrates_to_the_same_schema() {
        let mut fresh = Connection::open_in_memory().unwrap();
        migrate(&mut fresh).unwrap();
        let expected = schema_of(&fresh);
        for start in 0..latest_version() {
            let mut conn = Connection::open_in_memory().unwrap();
            migrate_to(&mut conn, start).unwrap();
            assert_eq!(current_version(&conn).unwrap(), start);
            migrate(&mut conn).unwrap();
            assert_eq!(schema_of(&conn), expected, "migration from version {start}");
        }
    }

    #[test]
    fn refuses_a_newer_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", latest_version() + 1)
            .unwrap();
        assert!(matches!(migrate(&mut conn), Err(StoreError::TooNew { .. })));
    }
}
