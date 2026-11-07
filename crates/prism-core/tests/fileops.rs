//! Safe deletion (ADR-0012, F16) on real temporary folders: every item is checked against the
//! catalog before it is touched, outcomes are logged and the catalog follows. The remover used
//! here deletes through the sandbox guard (the Recycle Bin itself is tested in prism-win, on the
//! test volume only).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;
use std::sync::Mutex;

use common::{Sandbox, TestCatalog};
use prism_core::fileops::{
    DeleteRequest, Disposal, FileOpsError, FolderCheck, Outcome, Remover, delete_entries,
};
use prism_core::model::{EntryId, VolumeId};
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::store;

/// Removes through the sandbox guard and remembers what it was asked.
struct GuardedRemover {
    fs: GuardedFs,
    calls: Mutex<Vec<(Vec<PathBuf>, Disposal)>>,
    /// Paths it pretends it cannot remove (file in use).
    refuse: Vec<PathBuf>,
}

impl GuardedRemover {
    fn new(sb: &Sandbox) -> Self {
        Self {
            fs: GuardedFs::new(SandboxGuard::new([sb.root()]).unwrap()),
            calls: Mutex::new(Vec::new()),
            refuse: Vec::new(),
        }
    }
}

impl Remover for GuardedRemover {
    fn remove(&self, paths: &[PathBuf], disposal: Disposal) -> Vec<(PathBuf, String)> {
        self.calls.lock().unwrap().push((paths.to_vec(), disposal));
        let mut failed = Vec::new();
        for p in paths {
            if self.refuse.contains(p) {
                failed.push((p.clone(), "in use".to_owned()));
                continue;
            }
            let r = if p.is_dir() {
                self.fs.remove_dir_all(p)
            } else {
                self.fs.remove_file(p)
            };
            if let Err(e) = r {
                failed.push((p.clone(), e.to_string()));
            }
        }
        failed
    }
}

fn setup() -> (Sandbox, TestCatalog, VolumeId) {
    let sb = Sandbox::new();
    sb.file(r"a\x.txt", 100, 10);
    sb.file(r"a\y.txt", 200, 20);
    sb.file(r"c\z.txt", 300, 30);
    sb.file(r"c\keep.txt", 5, 40);
    sb.mkdir(r"b");
    sb.mkdir(r"d\inner");
    sb.file(r"d\inner\cache.bin", 50, 50);
    let cat = TestCatalog::new();
    let volume = cat.add_volume(&sb.root().to_string_lossy());
    cat.index(volume, sb.root());
    (sb, cat, volume)
}

fn id(cat: &TestCatalog, volume: VolumeId, rel: &str) -> EntryId {
    cat.store
        .read(|c| store::resolve_relative_path(c, volume, rel))
        .unwrap()
        .unwrap_or_else(|| panic!("{rel} not in the catalog"))
        .id
}

fn request<'a>(
    sb: &'a Sandbox,
    volume: VolumeId,
    entries: &'a [EntryId],
    folders: FolderCheck,
) -> DeleteRequest<'a> {
    DeleteRequest {
        volume,
        root: sb.root(),
        entries,
        disposal: Disposal::RecycleBin,
        folders,
    }
}

// F16-AC3: changed and missing items are skipped and reported; the others are removed.
#[test]
fn f16_ac3_items_are_checked_against_the_catalog() {
    let (sb, cat, volume) = setup();
    let ids = [
        id(&cat, volume, r"a\x.txt"),
        id(&cat, volume, r"a\y.txt"),
        id(&cat, volume, r"c\z.txt"),
        id(&cat, volume, "b"),
    ];
    sb.file(r"a\y.txt", 201, 20); // size changed after indexing
    sb.fs.remove_file(&sb.path(r"c\z.txt")).unwrap(); // gone
    let remover = GuardedRemover::new(&sb);
    let guard = SandboxGuard::new([sb.root()]).unwrap();
    let report = delete_entries(
        &cat.store,
        &request(&sb, volume, &ids, FolderCheck::Empty),
        &remover,
        Some(&guard),
    )
    .unwrap();

    let outcomes: Vec<&Outcome> = report.items.iter().map(|i| &i.outcome).collect();
    assert_eq!(
        outcomes,
        [
            &Outcome::Done,
            &Outcome::SkippedChanged,
            &Outcome::SkippedMissing,
            &Outcome::Done
        ]
    );
    assert_eq!(report.freed(), 100);
    assert!(!sb.path(r"a\x.txt").exists());
    assert!(
        sb.path(r"a\y.txt").exists(),
        "a changed file is never removed"
    );
    assert!(!sb.path("b").exists());
    // Only verified paths reach the remover, in the Recycle Bin (F16-AC1).
    let calls = remover.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, [sb.path(r"a\x.txt"), sb.path("b")]);
    assert_eq!(calls[0].1, Disposal::RecycleBin);

    // The catalog follows: removed entries are gone, totals recomputed.
    let snap = cat.snapshot(volume);
    assert!(!snap.keys().any(|k| k.ends_with("x.txt")));
    assert!(snap.keys().any(|k| k.ends_with("y.txt")));
    cat.assert_consistent();

    // F16-AC6: every outcome is in the actions log.
    let log = cat.store.read(|c| store::actions_log(c, 10)).unwrap();
    let mut outcomes: Vec<&str> = log.iter().map(|r| r.outcome.as_str()).collect();
    outcomes.sort_unstable();
    assert_eq!(
        outcomes,
        ["done", "done", "skipped_changed", "skipped_missing"]
    );
    assert!(
        log.iter()
            .all(|r| r.action == "recycle" && r.volume == Some(volume))
    );
    assert!(
        log.iter()
            .any(|r| r.path.ends_with("x.txt") && r.size == 100)
    );
}

#[test]
fn empty_folder_check_and_whole_folders() {
    let (sb, cat, volume) = setup();
    let d = id(&cat, volume, "d");
    let inner = id(&cat, volume, r"d\inner");
    let remover = GuardedRemover::new(&sb);
    // Not empty any more: skipped.
    let report = delete_entries(
        &cat.store,
        &request(&sb, volume, &[inner], FolderCheck::Empty),
        &remover,
        None,
    )
    .unwrap();
    assert_eq!(report.items[0].outcome, Outcome::SkippedChanged);
    assert!(sb.path(r"d\inner\cache.bin").exists());

    // A whole folder (cache) with a child also selected: removed once, with its content.
    let report = delete_entries(
        &cat.store,
        &request(&sb, volume, &[inner, d], FolderCheck::Exists),
        &remover,
        None,
    )
    .unwrap();
    assert_eq!(report.items.len(), 1, "the child goes with its folder");
    assert_eq!(report.items[0].entry, d);
    assert_eq!(report.items[0].outcome, Outcome::Done);
    assert_eq!(report.freed(), 50);
    assert!(!sb.path("d").exists());
    cat.assert_consistent();
}

#[test]
fn failures_leave_disk_and_catalog_unchanged() {
    let (sb, cat, volume) = setup();
    let x = id(&cat, volume, r"a\x.txt");
    let mut remover = GuardedRemover::new(&sb);
    remover.refuse.push(sb.path(r"a\x.txt"));
    let report = delete_entries(
        &cat.store,
        &request(&sb, volume, &[x], FolderCheck::Empty),
        &remover,
        None,
    )
    .unwrap();
    assert_eq!(report.items[0].outcome, Outcome::Failed("in use".into()));
    assert!(sb.path(r"a\x.txt").exists());
    assert!(cat.snapshot(volume).keys().any(|k| k.ends_with("x.txt")));
    let log = cat.store.read(|c| store::actions_log(c, 10)).unwrap();
    assert_eq!(log[0].outcome, "failed");
    assert_eq!(log[0].detail.as_deref(), Some("in use"));
}

// TESTING S2: a path outside the sandbox stops everything before any change.
#[test]
fn sandbox_guard_refuses_before_touching_anything() {
    let (sb, cat, volume) = setup();
    let other = Sandbox::new();
    let x = id(&cat, volume, r"a\x.txt");
    let remover = GuardedRemover::new(&sb);
    let guard = SandboxGuard::new([other.root()]).unwrap();
    let err = delete_entries(
        &cat.store,
        &request(&sb, volume, &[x], FolderCheck::Empty),
        &remover,
        Some(&guard),
    )
    .unwrap_err();
    assert!(matches!(err, FileOpsError::Sandbox(_)), "{err}");
    assert!(remover.calls.lock().unwrap().is_empty());
    assert!(sb.path(r"a\x.txt").exists());
}

// ADR-0003: ids of another volume, or the volume root, are refused.
#[test]
fn foreign_entries_and_roots_are_refused() {
    let (sb, cat, volume) = setup();
    let root = cat
        .store
        .read(|c| store::root_entry(c, volume))
        .unwrap()
        .unwrap()
        .id;
    let remover = GuardedRemover::new(&sb);
    for entries in [vec![root], vec![id(&cat, volume, r"a\x.txt")]] {
        let wrong_volume = if entries[0] == root {
            volume
        } else {
            VolumeId(volume.0 + 1)
        };
        let err = delete_entries(
            &cat.store,
            &request(&sb, wrong_volume, &entries, FolderCheck::Empty),
            &remover,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, FileOpsError::WrongEntry(..)), "{err}");
    }
    assert!(remover.calls.lock().unwrap().is_empty());
}
