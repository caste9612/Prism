//! Reconciler tests on an in-memory file system with file references (as on NTFS via MFT).
// Test code: unwrap/expect are allowed (CLAUDE.md), also in helpers outside #[test].
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicU64;

use common::TestCatalog;
use common::fakefs::{FakeFs, FixedTree, ROOT, Rng};
use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::model::{EntryId, VolumeId, VolumeStatus};
use prism_core::reconcile::{self, ReconcileTarget};
use prism_core::store;
use proptest::prelude::*;

const ROOT_PATH: &str = r"X:\";

fn index(cat: &TestCatalog, volume: VolumeId, fs: &FakeFs) {
    cat.index_with(
        volume,
        Path::new(ROOT_PATH),
        fs,
        &ExclusionPolicy::default(),
        &CancelToken::new(),
    )
    .unwrap();
}

/// Entry id of every file reference in the catalog.
fn ids_by_ref(cat: &TestCatalog, volume: VolumeId) -> HashMap<u64, EntryId> {
    cat.store
        .read(|c| {
            let mut stmt = c.prepare("SELECT file_ref, id FROM entries WHERE volume_id = ?1")?;
            let rows = stmt.query_map([volume.0], |r| {
                Ok((r.get::<_, i64>(0)? as u64, EntryId(r.get(1)?)))
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .unwrap()
}

fn assert_same_as_fresh_index(updated: &TestCatalog, volume: VolumeId, fs: &FakeFs) {
    updated.assert_consistent();
    let fresh = TestCatalog::new();
    let v = fresh.add_volume(ROOT_PATH);
    index(&fresh, v, fs);
    fresh.assert_consistent();
    assert_eq!(updated.snapshot(volume), fresh.snapshot(v));
}

fn check_update_equals_full_reindex(seed: u64, steps: usize) {
    let mut rng = Rng::new(seed);
    let mut fs = FakeFs::new();
    fs.populate(&mut rng, 40);
    let cat = TestCatalog::new();
    let volume = cat.add_volume(ROOT_PATH);
    index(&cat, volume, &fs);
    assert_same_as_fresh_index(&cat, volume, &fs);
    let before = ids_by_ref(&cat, volume);

    fs.mutate(&mut rng, steps);
    index(&cat, volume, &fs);
    assert_eq!(cat.status(volume), VolumeStatus::Indexed);
    // F05-AC3: same catalog as a fresh full indexing of the same state.
    assert_same_as_fresh_index(&cat, volume, &fs);
    // F05-AC4: every file or folder that still exists keeps its entry, wherever it moved.
    let after = ids_by_ref(&cat, volume);
    for (r, id) in &after {
        if let Some(old) = before.get(r) {
            assert_eq!(old, id, "file reference {r} changed entry id");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    // F05-AC3 + F05-AC4 on random trees and random changes (renames, moves, swaps,
    // replacements, deletions, case-only renames).
    #[test]
    fn f05_ac3_update_equals_full_reindex_with_refs(seed in any::<u64>(), steps in 1usize..25) {
        check_update_equals_full_reindex(seed, steps);
    }

    // F05-AC3 with partial updates: only the folders reported as changed are reconciled (as a
    // change journal would do), each with a scan of its subtree.
    #[test]
    fn f05_ac3_partial_updates_equal_full_reindex(seed in any::<u64>(), steps in 1usize..15) {
        let mut rng = Rng::new(seed);
        let mut fs = FakeFs::new();
        fs.populate(&mut rng, 40);
        let cat = TestCatalog::new();
        let volume = cat.add_volume(ROOT_PATH);
        index(&cat, volume, &fs);

        let dirty = fs.mutate(&mut rng, steps);
        let mut dirs: Vec<u64> = dirty.into_iter().filter(|d| fs.exists(*d)).collect();
        dirs.sort_by_key(|d| (fs.depth(*d), *d));
        for dir in dirs {
            let target = if dir == ROOT {
                Some(ReconcileTarget::VolumeRoot)
            } else {
                cat.store
                    .read(|c| store::entries_by_ref(c, volume, dir))
                    .unwrap()
                    .first()
                    .map(|e| ReconcileTarget::Folder(e.id))
            };
            let Some(target) = target else { continue };
            reconcile::reconcile(
                &cat.store,
                volume,
                target,
                &fs.tree(dir),
                &CancelToken::new(),
                &AtomicU64::new(0),
            )
            .unwrap();
        }
        assert_same_as_fresh_index(&cat, volume, &fs);
    }
}

// F05-AC4: renaming and moving a folder keeps the entries of everything inside it.
#[test]
fn f05_ac4_folder_rename_and_move_keep_entry_ids() {
    let mut fs = FakeFs::new();
    let photos = fs.create(ROOT, "Photos", true, 0).unwrap();
    let y2024 = fs.create(photos, "2024", true, 0).unwrap();
    let img = fs.create(y2024, "img.jpg", false, 5000).unwrap();
    let archive = fs.create(ROOT, "Archive", true, 0).unwrap();
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);
    let before = ids_by_ref(&cat, v);

    assert!(fs.rename(photos, archive, "Old Photos"));
    let report = cat
        .index_with(
            v,
            Path::new(ROOT_PATH),
            &fs,
            &ExclusionPolicy::default(),
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(report.reconcile.moved, 1);
    assert_eq!(report.reconcile.inserted, 0);
    assert_eq!(report.reconcile.deleted, 0);
    let after = ids_by_ref(&cat, v);
    for r in [photos, y2024, img] {
        assert_eq!(before[&r], after[&r]);
    }
    let path = cat
        .store
        .read(|c| store::relative_path(c, after[&img]))
        .unwrap();
    assert_eq!(path, r"Archive\Old Photos\2024\img.jpg");
    assert_same_as_fresh_index(&cat, v, &fs);
}

// Names exchanged between two files: each entry follows its file reference.
#[test]
fn swapped_names_follow_file_refs() {
    let mut fs = FakeFs::new();
    let a = fs.create(ROOT, "a.txt", false, 1).unwrap();
    let b = fs.create(ROOT, "b.txt", false, 2).unwrap();
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);
    let before = ids_by_ref(&cat, v);
    assert!(fs.rename(a, ROOT, "tmp"));
    assert!(fs.rename(b, ROOT, "a.txt"));
    assert!(fs.rename(a, ROOT, "b.txt"));
    index(&cat, v, &fs);
    assert_eq!(before, ids_by_ref(&cat, v));
    let snap = cat.snapshot(v);
    assert_eq!(snap["a.txt"].meta.size, 2);
    assert_eq!(snap["b.txt"].meta.size, 1);
    assert_same_as_fresh_index(&cat, v, &fs);
}

// A folder moved under one of its former children (X/B -> B, then X -> B/X) must not create
// a cycle in the catalog.
#[test]
fn moving_a_folder_under_its_former_child_is_handled() {
    let mut fs = FakeFs::new();
    let x = fs.create(ROOT, "X", true, 0).unwrap();
    let b = fs.create(x, "B", true, 0).unwrap();
    fs.create(b, "f", false, 3).unwrap();
    fs.create(x, "g", false, 4).unwrap();
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);
    assert!(fs.rename(b, ROOT, "B"));
    assert!(fs.rename(x, b, "X"));
    index(&cat, v, &fs);
    assert!(cat.snapshot(v).contains_key(r"B\X\g"));
    assert_same_as_fresh_index(&cat, v, &fs);
}

// A file replaced by a folder with the same name: different entry, no name clash.
#[test]
fn file_replaced_by_folder_with_same_name() {
    let mut fs = FakeFs::new();
    let f = fs.create(ROOT, "data", false, 10).unwrap();
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);
    fs.remove(f);
    let d = fs.create(ROOT, "data", true, 0).unwrap();
    fs.create(d, "inside.bin", false, 7).unwrap();
    index(&cat, v, &fs);
    let snap = cat.snapshot(v);
    assert!(snap["data"].is_dir);
    assert_eq!(snap[""].stats.unwrap().total_size, 7);
    assert_same_as_fresh_index(&cat, v, &fs);
}

// Folders that cannot be read keep their catalog content instead of losing it; new entries
// seen elsewhere are still added.
#[test]
fn unreadable_folder_keeps_its_catalog_children() {
    let mut fs = FakeFs::new();
    let private = fs.create(ROOT, "private", true, 0).unwrap();
    fs.create(private, "secret.txt", false, 100).unwrap();
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);

    fs.unreadable.insert(private);
    fs.create(ROOT, "new.txt", false, 5).unwrap();
    index(&cat, v, &fs);
    cat.assert_consistent();
    let snap = cat.snapshot(v);
    assert!(snap.contains_key(r"private\secret.txt"));
    assert!(snap.contains_key("new.txt"));
    assert_eq!(snap[""].stats.unwrap().total_size, 105);
}

// F04-AC6: an update cancelled while writing leaves a consistent tree, marks the volume
// "reindex needed", and the next indexing repairs everything.
#[test]
fn f04_ac6_cancel_during_reconciliation_marks_reindex_needed() {
    let mut rng = Rng::new(7);
    let mut fs = FakeFs::new();
    fs.populate(&mut rng, 60);
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);
    fs.mutate(&mut rng, 20);

    let cancel = CancelToken::new();
    cancel.cancel();
    let err = cat
        .index_with(
            v,
            Path::new(ROOT_PATH),
            &FixedTree(fs.tree(ROOT)),
            &ExclusionPolicy::default(),
            &cancel,
        )
        .unwrap_err();
    assert!(err.is_cancelled());
    assert_eq!(cat.status(v), VolumeStatus::ReindexNeeded);

    index(&cat, v, &fs);
    assert_eq!(cat.status(v), VolumeStatus::Indexed);
    assert_same_as_fresh_index(&cat, v, &fs);
}

// F04-AC6: a cancelled enumeration writes nothing: data and status stay as they were.
#[test]
fn f04_ac6_cancel_during_enumeration_keeps_previous_data() {
    let mut rng = Rng::new(11);
    let mut fs = FakeFs::new();
    fs.populate(&mut rng, 30);
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);
    let before = cat.snapshot(v);
    fs.mutate(&mut rng, 10);

    let cancel = CancelToken::new();
    cancel.cancel();
    let err = cat
        .index_with(
            v,
            Path::new(ROOT_PATH),
            &fs,
            &ExclusionPolicy::default(),
            &cancel,
        )
        .unwrap_err();
    assert!(err.is_cancelled());
    assert_eq!(cat.status(v), VolumeStatus::Indexed);
    assert_eq!(cat.snapshot(v), before);
}

#[test]
fn reindexing_an_unchanged_volume_changes_nothing() {
    let mut rng = Rng::new(3);
    let mut fs = FakeFs::new();
    fs.populate(&mut rng, 80);
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);
    let before = ids_by_ref(&cat, v);
    let report = cat
        .index_with(
            v,
            Path::new(ROOT_PATH),
            &fs,
            &ExclusionPolicy::default(),
            &CancelToken::new(),
        )
        .unwrap();
    let r = report.reconcile;
    assert_eq!((r.inserted, r.updated, r.moved, r.deleted), (0, 0, 0, 0));
    assert_eq!(r.unchanged as usize, fs.refs().len());
    assert_eq!(before, ids_by_ref(&cat, v));
}

/// A file with several names in one folder (hard links, as `git.exe` and the `git-*.exe` of
/// Git for Windows): the names share one file reference. Seen in a diagnostic package: every
/// full reading of a system disk moved the same 1,881 entries.
#[test]
fn hard_links_in_one_folder_keep_their_entries() {
    use prism_core::model::{ATTR_DIRECTORY, EntryMeta, FileTime};
    use prism_core::scan::{ScanChild, ScanStats, ScanTree, ScanTreeBuilder};
    let meta = |r: u64, dir: bool| EntryMeta {
        size: if dir { 0 } else { 100 },
        alloc_size: Some(4096),
        created: Some(FileTime(1_000)),
        modified: Some(FileTime(2_000)),
        attrs: if dir { ATTR_DIRECTORY } else { 0x20 },
        file_ref: Some(r),
    };
    let tree = || {
        let mut b = ScanTreeBuilder::new(meta(5, true));
        let names = ["a.exe", "b.exe", "c.exe", "other.txt"];
        b.add_listing(
            ScanTree::ROOT,
            names.iter().map(|n| ScanChild {
                name: (*n).to_owned(),
                is_dir: false,
                meta: meta(if *n == "other.txt" { 41 } else { 40 }, false),
            }),
        );
        b.finish(ScanStats::default())
    };
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    let policy = ExclusionPolicy::default();
    let run = || {
        cat.index_with(
            v,
            Path::new(ROOT_PATH),
            &FixedTree(tree()),
            &policy,
            &CancelToken::new(),
        )
        .unwrap()
        .reconcile
    };
    run();
    let names_of = || {
        cat.store
            .read(|c| {
                let mut stmt = c.prepare("SELECT name, id FROM entries WHERE volume_id = ?1")?;
                let rows =
                    stmt.query_map([v.0], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
                Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
            })
            .unwrap()
    };
    let before = names_of();
    for _ in 0..3 {
        let r = run();
        assert_eq!(
            (r.inserted, r.updated, r.moved, r.deleted),
            (0, 0, 0, 0),
            "{r:?}"
        );
        assert_eq!(names_of(), before);
    }
}
