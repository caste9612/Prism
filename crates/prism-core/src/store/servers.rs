//! Names of the servers of network shares reached by IP address (REVIEW D59).

use rusqlite::{Connection, params};

use super::{StoreError, WriteTx};
use crate::model::{VolumeId, unix_millis_now};

/// The known names: (address as in the UNC paths, host name).
///
/// # Errors
/// SQLite errors.
pub fn share_servers(conn: &Connection) -> Result<Vec<(String, String)>, StoreError> {
    let mut stmt =
        conn.prepare_cached("SELECT address, name FROM share_servers ORDER BY address")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Records the host name of the server at `address`.
///
/// # Errors
/// SQLite errors.
pub fn set_share_server(
    wtx: &mut WriteTx<'_>,
    address: &str,
    name: &str,
) -> Result<(), StoreError> {
    wtx.conn()
        .prepare_cached(
            "INSERT INTO share_servers (address, name, seen_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT (address) DO UPDATE SET name = excluded.name, seen_at = excluded.seen_at",
        )?
        .execute(params![address, name, unix_millis_now()])?;
    Ok(())
}

/// Gives a network share of the catalog its new network path (the user moved it, REVIEW D59).
///
/// # Errors
/// [`StoreError::NotFound`] if no network volume has this id; SQLite errors.
pub fn set_share_path(
    wtx: &mut WriteTx<'_>,
    volume: VolumeId,
    unc: &str,
) -> Result<(), StoreError> {
    let changed = wtx
        .conn()
        .prepare_cached(
            "UPDATE volumes SET unc_path = ?2, last_mount = ?2 WHERE id = ?1 AND kind = 'network'",
        )?
        .execute(params![volume.0, unc])?;
    if changed == 0 {
        return Err(StoreError::NotFound(format!("network volume {volume}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::model::{VolumeInfo, VolumeKind, VolumeStatus};
    use crate::store::{self, Store};

    #[test]
    fn server_names_and_share_paths() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("c.db")).unwrap();
        store
            .write(|w| set_share_server(w, "192.168.1.65", "nas.local"))
            .unwrap();
        store
            .write(|w| set_share_server(w, "192.168.1.65", "zimaos.local"))
            .unwrap();
        assert_eq!(
            store.read(share_servers).unwrap(),
            vec![("192.168.1.65".to_owned(), "zimaos.local".to_owned())]
        );

        let info = VolumeInfo {
            kind: VolumeKind::Network,
            unc_path: Some(r"\\192.168.1.65\Photos".into()),
            ..VolumeInfo::dev_folder(r"\\192.168.1.65\Photos")
        };
        let local = VolumeInfo::dev_folder(r"D:\");
        let (share, disk) = store
            .write(move |w| {
                Ok((
                    store::insert_volume(w, &info, VolumeStatus::Indexed)?,
                    store::insert_volume(w, &local, VolumeStatus::Indexed)?,
                ))
            })
            .unwrap();
        store
            .write(move |w| set_share_path(w, share, r"\\192.168.1.57\Photos"))
            .unwrap();
        let v = store
            .read(move |c| store::volume(c, share))
            .unwrap()
            .unwrap();
        assert_eq!(v.info.unc_path.as_deref(), Some(r"\\192.168.1.57\Photos"));
        assert_eq!(v.info.last_mount.as_deref(), Some(r"\\192.168.1.57\Photos"));
        // Only network shares have a network path.
        assert!(
            store
                .write(move |w| set_share_path(w, disk, r"\\x\y"))
                .is_err()
        );
    }
}
