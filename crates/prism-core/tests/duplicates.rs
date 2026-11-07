//! Duplicates end to end on temporary folders (F10, F16-AC3/AC4): groups from real hashes, the
//! last-copy rule, and the content check right before deleting.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashSet;
use std::path::PathBuf;

use common::{Sandbox, TestCatalog};
use prism_core::cancel::CancelToken;
use prism_core::duplicates::{DupFilter, LastCopyError, check_last_copies, groups};
use prism_core::fileops::{DeleteRequest, Disposal, FolderCheck, Outcome, Remover, delete_entries};
use prism_core::hashing::{HashProgress, hash_volume};
use prism_core::model::{EntryId, VolumeId};
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::store;

struct GuardedRemover(GuardedFs);

impl Remover for GuardedRemover {
    fn remove(&self, paths: &[PathBuf], _: Disposal) -> Vec<(PathBuf, String)> {
        paths
            .iter()
            .filter_map(|p| {
                self.0
                    .remove_file(p)
                    .err()
                    .map(|e| (p.clone(), e.to_string()))
            })
            .collect()
    }
}

fn setup() -> (Sandbox, TestCatalog, VolumeId) {
    let sb = Sandbox::new();
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    for (rel, secs) in [
        (r"Foto\mare.jpg", 10),
        (r"Download\mare.jpg", 20),
        (r"Backup\mare.jpg", 30),
    ] {
        sb.file(rel, 0, secs);
        sb.fs.write(&sb.path(rel), &data).unwrap();
        sb.fs
            .set_modified(&sb.path(rel), common::mtime(secs))
            .unwrap();
    }
    sb.file(r"Altro\unico.txt", 999, 40);
    let cat = TestCatalog::new();
    let volume = cat.add_volume(&sb.root().to_string_lossy());
    cat.index(volume, sb.root());
    hash_volume(
        &cat.store,
        volume,
        sb.root(),
        &CancelToken::new(),
        &|| false,
        &HashProgress::default(),
    )
    .unwrap();
    (sb, cat, volume)
}

fn id(cat: &TestCatalog, volume: VolumeId, rel: &str) -> EntryId {
    cat.store
        .read(|c| store::resolve_relative_path(c, volume, rel))
        .unwrap()
        .unwrap()
        .id
}

#[test]
fn f10_groups_with_filters() {
    let (_sb, cat, volume) = setup();
    let online = HashSet::from([volume]);
    let all = cat
        .store
        .read(|c| groups(c, &online, &DupFilter::default()))
        .unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].files.len(), 3);
    assert_eq!(all[0].recoverable(), 400_000);
    assert_eq!((all[0].online(), all[0].offline()), (3, 0));
    // F10-AC5: filters.
    let big = DupFilter {
        min_size: 1_000_000,
        ..DupFilter::default()
    };
    assert!(
        cat.store
            .read(|c| groups(c, &online, &big))
            .unwrap()
            .is_empty()
    );
    let folder = DupFilter {
        folder: Some("backup".into()),
        ..DupFilter::default()
    };
    assert_eq!(
        cat.store
            .read(|c| groups(c, &online, &folder))
            .unwrap()
            .len(),
        1
    );
    let elsewhere = DupFilter {
        folder: Some("musica".into()),
        ..DupFilter::default()
    };
    assert!(
        cat.store
            .read(|c| groups(c, &online, &elsewhere))
            .unwrap()
            .is_empty()
    );
    // Offline: the copies stay listed, marked (F06-AC6, F10-AC1).
    let none = HashSet::new();
    let offline = cat
        .store
        .read(|c| groups(c, &none, &DupFilter::default()))
        .unwrap();
    assert_eq!(offline[0].offline(), 3);
}

// F16-AC4 / F10-AC4: never every copy; copies left only offline need a confirmation.
#[test]
fn f16_ac4_last_copy_rule() {
    let (_sb, cat, volume) = setup();
    let ids = [
        id(&cat, volume, r"Foto\mare.jpg"),
        id(&cat, volume, r"Download\mare.jpg"),
        id(&cat, volume, r"Backup\mare.jpg"),
    ];
    let online = HashSet::from([volume]);
    let check = |entries: &[EntryId], online: &HashSet<VolumeId>, allow: bool| {
        cat.store
            .read(|c| check_last_copies(c, entries, online, allow))
            .unwrap()
    };
    assert_eq!(check(&ids[1..], &online, false), Ok(()));
    assert_eq!(
        check(&ids, &online, false),
        Err(LastCopyError::LastCopy(ids[0]))
    );
    // The disk is not connected: what is left would be offline only.
    assert_eq!(
        check(&ids[1..], &HashSet::new(), false),
        Err(LastCopyError::OnlyOffline(ids[1]))
    );
    assert_eq!(check(&ids[1..], &HashSet::new(), true), Ok(()));
    // A file with no duplicate is a last copy.
    let unique = id(&cat, volume, r"Altro\unico.txt");
    assert_eq!(
        check(&[unique], &online, true),
        Err(LastCopyError::LastCopy(unique))
    );
}

// F16-AC3: same size and date but another content: the file is not deleted.
#[test]
fn f16_ac3_content_is_checked_before_deleting() {
    let (sb, cat, volume) = setup();
    let download = id(&cat, volume, r"Download\mare.jpg");
    let backup = id(&cat, volume, r"Backup\mare.jpg");
    // Overwrite the backup copy with other bytes of the same size, then restore its date.
    let other: Vec<u8> = (0..200_000u32).map(|i| (i % 7) as u8).collect();
    sb.fs.write(&sb.path(r"Backup\mare.jpg"), &other).unwrap();
    sb.fs
        .set_modified(&sb.path(r"Backup\mare.jpg"), common::mtime(30))
        .unwrap();

    let remover = GuardedRemover(GuardedFs::new(SandboxGuard::new([sb.root()]).unwrap()));
    let entries = [download, backup];
    let report = delete_entries(
        &cat.store,
        &DeleteRequest {
            volume,
            root: sb.root(),
            entries: &entries,
            disposal: Disposal::RecycleBin,
            folders: FolderCheck::Exists,
            verify_hash: true,
        },
        &remover,
        Some(&SandboxGuard::new([sb.root()]).unwrap()),
    )
    .unwrap();
    let outcomes: Vec<&Outcome> = report.items.iter().map(|i| &i.outcome).collect();
    assert_eq!(outcomes, [&Outcome::Done, &Outcome::SkippedChanged]);
    assert!(!sb.path(r"Download\mare.jpg").exists());
    assert!(
        sb.path(r"Backup\mare.jpg").exists(),
        "a changed duplicate is never deleted"
    );
}
