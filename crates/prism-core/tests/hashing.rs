//! Content hashes on real temporary folders (F06, F10-AC1): the three passes, what a change
//! does to a hash, and the duplicate groups that come out of them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use common::{Sandbox, TestCatalog};
use prism_core::cancel::CancelToken;
use prism_core::hashing::{HashProgress, hash_volume};
use prism_core::model::{EntryId, VolumeId};
use prism_core::store;

fn id(cat: &TestCatalog, volume: VolumeId, rel: &str) -> EntryId {
    cat.store
        .read(|c| store::resolve_relative_path(c, volume, rel))
        .unwrap()
        .unwrap()
        .id
}

fn content(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// a and b are duplicates; c has their size and their quick hash but another middle; d has
/// their size and another end; e is alone in its size; z is empty.
fn setup() -> (Sandbox, TestCatalog, VolumeId) {
    let sb = Sandbox::new();
    let base = content(300_000, 7);
    let mut middle = base.clone();
    middle[150_000] ^= 0xFF;
    let mut end = base.clone();
    end[299_999] ^= 0xFF;
    for (rel, data, secs) in [
        (r"a\copy.bin", &base, 10),
        (r"b\copy.bin", &base, 20),
        (r"c\middle.bin", &middle, 30),
        (r"d\end.bin", &end, 40),
    ] {
        sb.file(rel, 0, secs);
        sb.fs.write(&sb.path(rel), data).unwrap();
        sb.fs
            .set_modified(&sb.path(rel), common::mtime(secs))
            .unwrap();
    }
    sb.file(r"e\alone.bin", 1234, 50);
    sb.file("z.bin", 0, 60);
    let cat = TestCatalog::new();
    let volume = cat.add_volume(&sb.root().to_string_lossy());
    cat.index(volume, sb.root());
    (sb, cat, volume)
}

fn run(sb: &Sandbox, cat: &TestCatalog, volume: VolumeId) -> prism_core::hashing::HashReport {
    hash_volume(
        &cat.store,
        volume,
        sb.root(),
        &CancelToken::new(),
        &|| false,
        &HashProgress::default(),
    )
    .unwrap()
}

// F06-AC1/AC4 and F10-AC1.
#[test]
fn f06_hashes_and_f10_ac1_duplicate_groups() {
    let (sb, cat, volume) = setup();
    let report = run(&sb, &cat, volume);
    // Quick: the four files of the shared size; full: every non-empty file.
    assert_eq!(report.quick, 4);
    assert_eq!(report.full, 5);
    assert_eq!(
        (report.changed, report.unreadable, report.interrupted),
        (0, 0, false)
    );

    let h = |rel: &str| {
        cat.store
            .read(|c| store::valid_hashes(c, id(&cat, volume, rel)))
            .unwrap()
    };
    let (a, b, c, d) = (
        h(r"a\copy.bin"),
        h(r"b\copy.bin"),
        h(r"c\middle.bin"),
        h(r"d\end.bin"),
    );
    let (a, b, c, d) = (a.unwrap(), b.unwrap(), c.unwrap(), d.unwrap());
    assert_eq!(a.quick, b.quick);
    assert_eq!(a.quick, c.quick, "the middle is not in the quick hash");
    assert_ne!(a.quick, d.quick);
    assert_eq!(a.full, b.full);
    assert_ne!(a.full, c.full);
    let alone = h(r"e\alone.bin").unwrap();
    assert!(alone.quick.is_none() && alone.full.is_some());
    assert!(h("z.bin").is_none(), "empty files are not hashed");

    // F10-AC1: one group, a and b.
    let dups = cat.store.read(|c| store::duplicate_files(c, 1)).unwrap();
    let ids: BTreeSet<EntryId> = dups.iter().map(|d| d.entry).collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            id(&cat, volume, r"a\copy.bin"),
            id(&cat, volume, r"b\copy.bin")
        ])
    );
    // A second run has nothing left to do.
    let again = run(&sb, &cat, volume);
    assert_eq!((again.quick, again.full), (0, 0));
    let (total, hashed) = cat.store.read(|c| store::hash_progress(c, volume)).unwrap();
    assert_eq!((total, hashed), (5, 5));
}

// F06-AC3: a hash is valid only for the size and time it was computed for.
#[test]
fn f06_ac3_a_changed_file_loses_its_hash_until_hashed_again() {
    let (sb, cat, volume) = setup();
    run(&sb, &cat, volume);
    let b = id(&cat, volume, r"b\copy.bin");
    sb.fs
        .set_modified(&sb.path(r"b\copy.bin"), common::mtime(999))
        .unwrap();
    cat.index(volume, sb.root()); // the catalog learns the new time
    assert!(
        cat.store
            .read(|c| store::valid_hashes(c, b))
            .unwrap()
            .is_none()
    );
    assert!(
        cat.store
            .read(|c| store::duplicate_files(c, 1))
            .unwrap()
            .is_empty()
    );
    let report = run(&sb, &cat, volume);
    assert_eq!(report.full, 1);
    assert_eq!(
        cat.store
            .read(|c| store::duplicate_files(c, 1))
            .unwrap()
            .len(),
        2
    );
}

// A file that changes between indexing and hashing gets no hash; a missing one is unreadable.
#[test]
fn changed_and_missing_files_are_skipped() {
    let (sb, cat, volume) = setup();
    sb.fs
        .set_modified(&sb.path(r"e\alone.bin"), common::mtime(5))
        .unwrap();
    sb.fs.remove_file(&sb.path(r"d\end.bin")).unwrap();
    let report = run(&sb, &cat, volume);
    assert_eq!(report.changed, 1);
    assert!(report.unreadable >= 1);
    let alone = id(&cat, volume, r"e\alone.bin");
    assert!(
        cat.store
            .read(|c| store::valid_hashes(c, alone))
            .unwrap()
            .is_none()
    );
}

// F06-AC5: a pause stops the run between files; nothing half-done is saved.
#[test]
fn f06_ac5_pause_stops_cleanly() {
    let (sb, cat, volume) = setup();
    let progress = HashProgress::default();
    let report = hash_volume(
        &cat.store,
        volume,
        sb.root(),
        &CancelToken::new(),
        &|| true,
        &progress,
    )
    .unwrap();
    assert!(report.interrupted);
    assert_eq!((report.quick, report.full), (0, 0));
    assert!(progress.total.load(Ordering::Relaxed) > 0);
    let (_, hashed) = cat.store.read(|c| store::hash_progress(c, volume)).unwrap();
    assert_eq!(hashed, 0);
}
