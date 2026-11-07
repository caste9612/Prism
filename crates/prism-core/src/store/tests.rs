use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::*;
use crate::model::{EntryMeta, VolumeInfo, VolumeLabels, VolumeStatus};

fn temp_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("catalog.db")).unwrap();
    (dir, store)
}

fn add_volume(store: &Store, mount: &str) -> VolumeId {
    let info = VolumeInfo::dev_folder(mount);
    store
        .write(move |w| insert_volume(w, &info, VolumeStatus::New))
        .unwrap()
}

fn file(size: u64) -> EntryMeta {
    EntryMeta {
        size,
        ..EntryMeta::default()
    }
}

/// Inserts root/d1/d2/... (a chain of `depth` folders) with one file of `size` in the deepest.
fn insert_chain(store: &Store, volume: VolumeId, depth: u32, size: u64) -> Vec<EntryId> {
    let first = store.allocate_entry_ids(depth + 2);
    let mut ops = Vec::new();
    let mut ids = Vec::new();
    for i in 0..=depth {
        let id = EntryId(first.0 + i64::from(i));
        ops.push(WriteOp::Insert {
            id,
            volume,
            parent: ids.last().copied(),
            name: if i == 0 {
                String::new()
            } else {
                format!("d{i}")
            },
            is_dir: true,
            meta: EntryMeta::default(),
        });
        ids.push(id);
    }
    let file_id = EntryId(first.0 + i64::from(depth) + 1);
    ops.push(WriteOp::Insert {
        id: file_id,
        volume,
        parent: ids.last().copied(),
        name: "f.bin".into(),
        is_dir: false,
        meta: file(size),
    });
    ops.push(WriteOp::RecomputeDirStats {
        dirs: vec![*ids.last().unwrap()],
    });
    let session = store.session();
    session.submit(volume, ops).unwrap();
    session.flush().unwrap();
    ids.push(file_id);
    ids
}

#[test]
fn opens_in_wal_mode_with_latest_schema() {
    let (_dir, store) = temp_store();
    store
        .read(|c| {
            let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            assert_eq!(mode.to_lowercase(), "wal");
            assert_eq!(schema::current_version(c)?, schema::latest_version());
            Ok(())
        })
        .unwrap();
}

#[test]
fn volume_roundtrip_and_labels() {
    let (_dir, store) = temp_store();
    let id = add_volume(&store, r"D:\Photos");
    let labels = VolumeLabels {
        name: Some("Backup Foto".into()),
        location: Some("cassetto 2".into()),
        notes: None,
        color: Some("#ff8800".into()),
    };
    let l2 = labels.clone();
    store.write(move |w| set_volume_labels(w, id, &l2)).unwrap();
    let v = store.read(|c| volume(c, id)).unwrap().unwrap();
    assert_eq!(v.labels, labels);
    assert_eq!(v.status, VolumeStatus::New);
    assert_eq!(v.info.last_mount.as_deref(), Some(r"D:\Photos"));
    let found = store.read(|c| volume_by_mount(c, r"d:\photos")).unwrap();
    assert_eq!(found.map(|v| v.id), Some(id));
    assert!(matches!(
        store.write(|w| set_volume_status(w, VolumeId(999), VolumeStatus::Indexed)),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn usn_position_roundtrip() {
    let (_dir, store) = temp_store();
    let id = add_volume(&store, "C:");
    assert_eq!(store.read(|c| volume(c, id)).unwrap().unwrap().usn, None);
    let pos = crate::model::UsnPosition {
        journal_id: 0x01DB_42D5_FC01_83FD,
        next_usn: 0x9_E500_2958,
    };
    store
        .write(move |w| set_usn_position(w, id, Some(pos)))
        .unwrap();
    assert_eq!(
        store.read(|c| volume(c, id)).unwrap().unwrap().usn,
        Some(pos)
    );
    store.write(move |w| set_usn_position(w, id, None)).unwrap();
    assert_eq!(store.read(|c| volume(c, id)).unwrap().unwrap().usn, None);
}

// ADR-0008 / NF1 / F04-AC7: readers never wait for the writer, even while a write transaction
// is open.
#[test]
fn f04_ac7_reads_do_not_wait_for_an_open_write_transaction() {
    let (_dir, store) = temp_store();
    let id = add_volume(&store, "X:");
    let (entered_tx, entered_rx) = crossbeam_channel::bounded::<()>(1);
    let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(1);
    let writer = {
        let store = store.clone();
        std::thread::spawn(move || {
            store.write(move |w| {
                set_volume_status(w, id, VolumeStatus::Indexing)?;
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
        })
    };
    entered_rx.recv().unwrap();
    // The write transaction is open and holds the write lock. Reads proceed immediately and
    // see the last committed state.
    let started = Instant::now();
    for _ in 0..20 {
        let v = store.read(|c| volume(c, id)).unwrap().unwrap();
        assert_eq!(v.status, VolumeStatus::New);
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    release_tx.send(()).unwrap();
    writer.join().unwrap().unwrap();
    let v = store.read(|c| volume(c, id)).unwrap().unwrap();
    assert_eq!(v.status, VolumeStatus::Indexing);
}

#[test]
fn failed_write_rolls_back_and_publishes_nothing() {
    let (_dir, store) = temp_store();
    let events = store.subscribe();
    let id = add_volume(&store, "X:");
    assert_eq!(
        events.try_recv().unwrap(),
        StoreEvent::VolumeChanged { volume: id }
    );
    let result: Result<(), StoreError> = store.write(move |w| {
        set_volume_status(w, id, VolumeStatus::Ignored)?;
        Err(StoreError::NotFound("forced failure".into()))
    });
    assert!(result.is_err());
    assert!(events.try_recv().is_err());
    let v = store.read(|c| volume(c, id)).unwrap().unwrap();
    assert_eq!(v.status, VolumeStatus::New);
}

#[test]
fn a_panicking_job_does_not_stop_the_writer() {
    let (_dir, store) = temp_store();
    let result: Result<(), StoreError> = store.write(|_| panic!("boom"));
    assert!(matches!(result, Err(StoreError::WriterGone)));
    // The writer is still alive.
    add_volume(&store, "Y:");
}

#[test]
fn session_failure_is_sticky_and_skips_later_batches() {
    let (_dir, store) = temp_store();
    let volume = add_volume(&store, "X:");
    let session = store.session();
    let bad = store.allocate_entry_ids(1);
    // Parent does not exist: the foreign key rejects the insert.
    session
        .submit(
            volume,
            vec![WriteOp::Insert {
                id: bad,
                volume,
                parent: Some(EntryId(123_456)),
                name: "orphan".into(),
                is_dir: false,
                meta: file(1),
            }],
        )
        .unwrap();
    let root = store.allocate_entry_ids(1);
    let later = vec![WriteOp::Insert {
        id: root,
        volume,
        parent: None,
        name: String::new(),
        is_dir: true,
        meta: EntryMeta::default(),
    }];
    // The submit may or may not see the error yet, but the batch must never be applied.
    let _ = session.submit(volume, later);
    assert!(matches!(
        session.flush(),
        Err(StoreError::Sqlite(_) | StoreError::SessionFailed)
    ));
    assert!(matches!(session.flush(), Err(StoreError::SessionFailed)));
    assert!(store.read(|c| root_entry(c, volume)).unwrap().is_none());
}

fn session_flag(path: &std::path::Path) -> String {
    let conn = Connection::open(path).unwrap();
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [SESSION_KEY],
        |r| r.get(0),
    )
    .unwrap()
}

// NF4: a clean close is recorded; after an unclean one the next open runs an integrity check
// and still opens a healthy catalog.
#[test]
fn nf4_unclean_shutdown_is_detected_and_checked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let store = Store::open(&path).unwrap();
        let v = add_volume(&store, "X:");
        insert_chain(&store, v, 3, 1);
    }
    assert_eq!(session_flag(&path), "0", "clean close");

    let store = Store::open(&path).unwrap();
    assert_eq!(session_flag(&path), "1", "open session");
    // Simulate a crash: the store is never closed.
    std::mem::forget(store);
    let reopened = Store::open(&path).unwrap();
    assert!(reopened.read(check_consistency).unwrap().is_empty());
}

// NF4: the application exits without dropping the store (Tauri ends the process): an explicit
// close records the clean exit, and the store keeps working until then.
#[test]
fn nf4_explicit_close_records_a_clean_exit_without_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    let store = Store::open(&path).unwrap();
    let v = add_volume(&store, "X:");
    insert_chain(&store, v, 3, 1);
    assert_eq!(session_flag(&path), "1");
    assert!(store.close(Duration::from_secs(10)));
    assert_eq!(session_flag(&path), "0", "clean exit recorded");
    assert_eq!(
        std::fs::metadata(path.with_extension("db-wal")).map_or(0, |m| m.len()),
        0,
        "WAL folded back into the catalog"
    );
    // Still usable afterwards (jobs may finish while the process ends).
    add_volume(&store, "Y:");
    // The process ends here: nothing is dropped.
    std::mem::forget(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.read(volumes).unwrap().len(), 2);
}

// NF4: after an unclean shutdown, a damaged catalog is reported instead of being used.
#[test]
fn nf4_damaged_catalog_is_refused_after_unclean_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let store = Store::open(&path).unwrap();
        let v = add_volume(&store, "X:");
        let ids = insert_chain(&store, v, 1, 1);
        let start = store.allocate_entry_ids(20_000);
        let ops = (0..20_000)
            .map(|i| WriteOp::Insert {
                id: EntryId(start.0 + i),
                volume: v,
                parent: Some(ids[0]),
                name: format!("file-{i:05}.bin"),
                is_dir: false,
                meta: file(1),
            })
            .collect();
        let session = store.session();
        session.submit(v, ops).unwrap();
        session.flush().unwrap();
    }
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE settings SET value = '1' WHERE key = ?1",
            [SESSION_KEY],
        )
        .unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
    }
    // Overwrite a page in the middle of the file (a temporary test file, S1).
    let guard =
        crate::sandbox::GuardedFs::new(crate::sandbox::SandboxGuard::new([dir.path()]).unwrap());
    let mut bytes = std::fs::read(&path).unwrap();
    let page = 4096;
    assert!(bytes.len() > 40 * page);
    bytes[20 * page..21 * page].fill(0xAB);
    guard.write(&path, &bytes).unwrap();

    let result = Store::open(&path);
    assert!(
        matches!(result, Err(StoreError::Corrupt(_) | StoreError::Sqlite(_))),
        "a damaged catalog must not open silently"
    );
}

#[test]
fn interrupted_indexing_becomes_reindex_needed_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    let id = {
        let store = Store::open(&path).unwrap();
        let id = add_volume(&store, "X:");
        store
            .write(move |w| set_volume_status(w, id, VolumeStatus::Indexing))
            .unwrap();
        id
    };
    let store = Store::open(&path).unwrap();
    let v = store.read(|c| volume(c, id)).unwrap().unwrap();
    assert_eq!(v.status, VolumeStatus::ReindexNeeded);
}

#[test]
fn entry_ids_keep_increasing_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    let last = {
        let store = Store::open(&path).unwrap();
        let v = add_volume(&store, "X:");
        *insert_chain(&store, v, 2, 10).last().unwrap()
    };
    let store = Store::open(&path).unwrap();
    assert!(store.allocate_entry_ids(1).0 > last.0);
}

// F04-AC4: folder aggregates are available right after the writes, along the whole chain.
#[test]
fn f04_ac4_dir_stats_cover_the_whole_subtree() {
    let (_dir, store) = temp_store();
    let v = add_volume(&store, "X:");
    let ids = insert_chain(&store, v, 3, 1000);
    for (depth, dir) in ids[..4].iter().enumerate() {
        let s = store.read(|c| dir_stats(c, *dir)).unwrap().unwrap();
        assert_eq!(s.total_size, 1000, "depth {depth}");
        assert_eq!(s.file_count, 1);
        assert_eq!(s.dir_count, 3 - depth as u64);
    }
    assert!(store.read(check_consistency).unwrap().is_empty());
    assert_eq!(
        store.read(|c| relative_path(c, ids[4])).unwrap(),
        r"d1\d2\d3\f.bin"
    );
    let found = store
        .read(|c| resolve_relative_path(c, v, r"D1\d2\D3\F.BIN"))
        .unwrap()
        .unwrap();
    assert_eq!(found.id, ids[4]);
}

// Deleting a very deep subtree works (no cascade depth limit) and updates the aggregates.
#[test]
fn deletes_deep_subtrees() {
    let (_dir, store) = temp_store();
    let v = add_volume(&store, "X:");
    let ids = insert_chain(&store, v, 3000, 7);
    let session = store.session();
    session
        .submit(
            v,
            vec![
                WriteOp::DeleteSubtree { id: ids[1] },
                WriteOp::RecomputeDirStats { dirs: vec![ids[0]] },
            ],
        )
        .unwrap();
    session.flush().unwrap();
    assert_eq!(store.read(|c| entry_count(c, v)).unwrap(), 1);
    let s = store.read(|c| dir_stats(c, ids[0])).unwrap().unwrap();
    assert_eq!(s, crate::model::DirStats::default());
    assert!(store.read(check_consistency).unwrap().is_empty());
}

// F01-AC5: removing a volume removes only its catalog data.
#[test]
fn f01_ac5_remove_volume_deletes_only_its_catalog_data() {
    let (_dir, store) = temp_store();
    let a = add_volume(&store, "A:");
    let b = add_volume(&store, "B:");
    insert_chain(&store, a, 5, 1);
    insert_chain(&store, b, 5, 1);
    store.write(move |w| remove_volume(w, a)).unwrap();
    assert!(store.read(|c| volume(c, a)).unwrap().is_none());
    assert_eq!(store.read(|c| entry_count(c, a)).unwrap(), 0);
    assert_eq!(store.read(|c| entry_count(c, b)).unwrap(), 7);
    assert!(store.read(check_consistency).unwrap().is_empty());
}

#[test]
fn check_consistency_detects_wrong_aggregates() {
    let (_dir, store) = temp_store();
    let v = add_volume(&store, "X:");
    let ids = insert_chain(&store, v, 1, 5);
    let root = ids[0];
    store
        .write(move |w| {
            w.conn().execute(
                "UPDATE dir_stats SET total_size = 999 WHERE entry_id = ?1",
                [root.0],
            )?;
            Ok(())
        })
        .unwrap();
    let problems = store.read(check_consistency).unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
}

// Interactive writes have their own lane and are not stuck behind queued bulk batches.
#[test]
fn interactive_writes_are_not_queued_behind_bulk_batches() {
    let (_dir, store) = temp_store();
    let v = add_volume(&store, "X:");
    let session = store.session();
    let root = store.allocate_entry_ids(1);
    session
        .submit(
            v,
            vec![WriteOp::Insert {
                id: root,
                volume: v,
                parent: None,
                name: String::new(),
                is_dir: true,
                meta: EntryMeta::default(),
            }],
        )
        .unwrap();
    for b in 0..3 {
        let start = store.allocate_entry_ids(5_000);
        let ops = (0..5_000)
            .map(|i| WriteOp::Insert {
                id: EntryId(start.0 + i),
                volume: v,
                parent: Some(root),
                name: format!("b{b}-{i}"),
                is_dir: false,
                meta: file(1),
            })
            .collect();
        session.submit(v, ops).unwrap();
    }
    // When the interactive write runs, at least one bulk batch is still pending.
    let seen_pending = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&seen_pending);
    store
        .write(move |w| {
            let n: i64 = w
                .conn()
                .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?;
            flag.store(n < 15_001, std::sync::atomic::Ordering::SeqCst);
            set_volume_status(w, v, VolumeStatus::Indexing)
        })
        .unwrap();
    session.flush().unwrap();
    assert!(seen_pending.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(store.read(|c| entry_count(c, v)).unwrap(), 15_001);
}
