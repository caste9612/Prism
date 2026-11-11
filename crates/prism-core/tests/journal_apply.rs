//! Applying change-journal records to the catalog (ADR-0018) on the in-memory file system:
//! the result must equal a full indexing of the same state (F05-AC3), and every file that
//! still exists keeps its entry (F05-AC4).
// Test code: unwrap/expect are allowed (CLAUDE.md), also in helpers outside #[test].
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicU64;

use common::TestCatalog;
use common::fakefs::{FakeFs, ROOT, Rng};
use prism_core::cancel::CancelToken;
use prism_core::changes::{ApplyOutcome, apply_changes, name_claimed_twice};
use prism_core::exclusion::ExclusionPolicy;
use prism_core::model::{EntryId, VolumeId};
use proptest::prelude::*;

const ROOT_PATH: &str = r"X:\";

fn index(cat: &TestCatalog, volume: VolumeId, fs: &FakeFs) {
    cat.index_with(
        volume,
        Path::new(ROOT_PATH),
        fs,
        &ExclusionPolicy::with_defaults(),
        &CancelToken::new(),
    )
    .unwrap();
}

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

/// Applies the recorded changes (or a full reconciliation when they cannot be applied) and
/// returns whether the incremental path was used.
fn update(cat: &TestCatalog, volume: VolumeId, fs: &mut FakeFs) -> bool {
    let changes = fs.take_changes();
    let outcome = apply_changes(
        &cat.store,
        volume,
        &ExclusionPolicy::with_defaults(),
        &changes,
        &*fs,
        &CancelToken::new(),
        &AtomicU64::new(0),
    )
    .unwrap();
    match outcome {
        ApplyOutcome::Applied(_) => true,
        ApplyOutcome::FullReconcileNeeded(_) => {
            index(cat, volume, fs);
            false
        }
    }
}

fn assert_same_as_fresh_index(cat: &TestCatalog, volume: VolumeId, fs: &FakeFs) {
    cat.assert_consistent();
    let fresh = TestCatalog::new();
    let v = fresh.add_volume(ROOT_PATH);
    index(&fresh, v, fs);
    let got = cat.snapshot(volume);
    let want = fresh.snapshot(v);
    let mut diffs = Vec::new();
    for (k, w) in &want {
        match got.get(k) {
            None => diffs.push(format!("missing {k:?}: {w:?}")),
            Some(g) if g != w => diffs.push(format!("differs {k:?}:\n  got  {g:?}\n  want {w:?}")),
            Some(_) => {}
        }
    }
    for (k, g) in &got {
        if !want.contains_key(k) {
            diffs.push(format!("extra {k:?}: {g:?}"));
        }
    }
    assert!(
        diffs.is_empty(),
        "catalog differs from a fresh index:\n{}",
        diffs.join("\n")
    );
}

fn check(seed: u64, rounds: usize, steps: usize) -> (usize, usize) {
    let mut rng = Rng::new(seed);
    let mut fs = FakeFs::new();
    fs.ensure_recycle_bin();
    fs.populate(&mut rng, 40);
    fs.take_changes();
    let cat = TestCatalog::new();
    let volume = cat.add_volume(ROOT_PATH);
    index(&cat, volume, &fs);
    let (mut incremental, mut full) = (0, 0);
    for _ in 0..rounds {
        let before = ids_by_ref(&cat, volume);
        fs.mutate(&mut rng, steps);
        if update(&cat, volume, &mut fs) {
            incremental += 1;
        } else {
            full += 1;
        }
        assert_same_as_fresh_index(&cat, volume, &fs);
        for (r, id) in ids_by_ref(&cat, volume) {
            if let Some(old) = before.get(&r) {
                assert_eq!(*old, id, "file reference {r} changed entry id");
            }
        }
    }
    (incremental, full)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(160))]

    // F05-AC3 / F05-AC4 through the change journal: several rounds of random changes
    // (creations, deletions, renames, moves, swaps, replacements, recycle and restore).
    #[test]
    fn f05_ac3_journal_updates_equal_full_reindex(seed in any::<u64>(), steps in 1usize..12) {
        check(seed, 3, steps);
    }
}

// Most rounds must take the incremental path (the full reconciliation is a rare fallback).
#[test]
fn journal_updates_are_mostly_incremental() {
    let (mut incremental, mut full) = (0, 0);
    for seed in 0..40 {
        let (i, f) = check(seed, 3, 5);
        incremental += i;
        full += f;
    }
    assert!(
        incremental > full * 4,
        "incremental {incremental}, full {full}"
    );
}

// The parent folder's modification time changes without a record of its own: the update
// must still bring it up to date (it is read again).
#[test]
fn parent_folder_times_follow_their_content() {
    let mut fs = FakeFs::new();
    let docs = fs.create(ROOT, "Docs", true, 0).unwrap();
    fs.take_changes();
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);
    fs.create(docs, "new.txt", false, 10).unwrap();
    let changes = fs.take_changes();
    assert_eq!(changes.len(), 1, "only the new file is in the journal");
    fs.touched
        .insert(changes[0].file_ref, (true, changes[0].parents_seen.clone()));
    assert!(update(&cat, v, &mut fs));
    assert_same_as_fresh_index(&cat, v, &fs);
}

// A folder restored from the recycle bin brings content that the journal does not describe:
// the update falls back to a full reconciliation, and the result is still exact.
#[test]
fn restoring_a_folder_with_content_needs_a_full_reconciliation() {
    let mut fs = FakeFs::new();
    let bin = fs.ensure_recycle_bin();
    let photos = fs.create(ROOT, "Photos", true, 0).unwrap();
    fs.create(photos, "a.jpg", false, 100).unwrap();
    fs.take_changes();
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);

    assert!(fs.rename(photos, bin, "$R1"));
    assert!(update(&cat, v, &mut fs), "moving to the bin is incremental");
    assert!(!cat.snapshot(v).contains_key("Photos"));

    assert!(fs.rename(photos, ROOT, "Photos"));
    assert!(
        !update(&cat, v, &mut fs),
        "restoring needs a full reconciliation"
    );
    assert!(cat.snapshot(v).contains_key(r"Photos\a.jpg"));
    assert_same_as_fresh_index(&cat, v, &fs);
}

#[test]
fn no_changes_is_a_no_op() {
    let mut rng = Rng::new(5);
    let mut fs = FakeFs::new();
    fs.populate(&mut rng, 20);
    fs.take_changes();
    let cat = TestCatalog::new();
    let v = cat.add_volume(ROOT_PATH);
    index(&cat, v, &fs);
    let before = cat.snapshot(v);
    assert!(update(&cat, v, &mut fs));
    assert_eq!(cat.snapshot(v), before);
}

// Wider random search, run by hand: `cargo test --release -p prism-core --test journal_apply
// -- --ignored`.
#[test]
#[ignore = "long: thousands of random cases"]
fn journal_updates_stress() {
    for seed in 0..3000u64 {
        let steps = 1 + (seed % 15) as usize;
        check(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), 4, steps);
    }
}

/// The states of the changed files are read one after the other: a file renamed over another
/// between the two reads leaves the name to both (REVIEW L30, seen on a system disk during a
/// program update). Applied as they are, the two collide in the catalog; the journal update
/// checks for it first and tries again later.
#[test]
fn a_name_read_for_two_changed_files_is_found() {
    let mut fs = FakeFs::new();
    let docs = fs.create(ROOT, "docs", true, 0).unwrap();
    let old = fs.create(docs, "a.txt", false, 10).unwrap();
    fs.take_changes();
    let cat = TestCatalog::new();
    let volume = cat.add_volume(ROOT_PATH);
    index(&cat, volume, &fs);

    // A program writes a new version beside the file, then renames it over the file.
    let new = fs.create(docs, "a.txt.tmp", false, 20).unwrap();
    let old_read_first = fs.state_of(old);
    fs.remove(old);
    assert!(fs.rename(new, docs, "a.txt"));
    let mut changes = fs.take_changes();
    for c in &mut changes {
        if c.file_ref == old {
            c.state.clone_from(&old_read_first);
        }
    }
    assert_eq!(name_claimed_twice(&changes), Some(docs));
    let err = apply_changes(
        &cat.store,
        volume,
        &ExclusionPolicy::with_defaults(),
        &changes,
        &fs,
        &CancelToken::new(),
        &AtomicU64::new(0),
    )
    .unwrap_err();
    assert!(err.to_string().contains("UNIQUE"), "{err}");

    // Read again, the states agree.
    let changes: Vec<_> = changes
        .into_iter()
        .map(|mut c| {
            c.state = fs.state_of(c.file_ref);
            c
        })
        .collect();
    assert_eq!(name_claimed_twice(&changes), None);
}
