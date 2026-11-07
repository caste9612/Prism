//! Safe deletion end to end on the PRISMTEST test volume (ADR-0012, F16): a folder of the test
//! volume is indexed into a temporary catalog, changed, then cleaned through
//! `prism_core::fileops` with the real Recycle Bin remover. Only that folder and the test
//! volume's Recycle Bin are touched. Run by the admin runner (docs/TESTING.md §4).
#![cfg(all(windows, feature = "admin-tests"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::fileops::{DeleteRequest, Disposal, FolderCheck, Outcome, delete_entries};
use prism_core::indexer::{self, IndexProgress, IndexRequest};
use prism_core::model::{BusType, VolumeInfo, VolumeStatus};
use prism_core::reconcile::ReconcileTarget;
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::scan::WalkEnumerator;
use prism_core::store::{self, Store};
use prism_core::volumes::VolumeSource;
use prism_win::trash::{ShellRemover, empty_recycle_bin_of, recycle_bin_info};
use prism_win::volumes::WinVolumeSource;

fn test_volume() -> PathBuf {
    let root = std::env::var("PRISM_TEST_VOLUME").expect("PRISM_TEST_VOLUME is set by the runner");
    let v = WinVolumeSource
        .detect()
        .unwrap()
        .into_iter()
        .find(|d| {
            d.mounts.iter().any(|m| {
                m.trim_end_matches('\\')
                    .eq_ignore_ascii_case(root.trim_end_matches('\\'))
            })
        })
        .expect("test volume detected");
    assert_eq!(
        v.info.os_label.as_deref(),
        Some("PRISMTEST"),
        "safety check"
    );
    assert_eq!(v.info.bus, Some(BusType::Virtual), "safety check");
    PathBuf::from(root)
}

#[test]
fn f16_safe_deletion_end_to_end_on_the_test_volume() {
    let root = test_volume();
    let fs = GuardedFs::new(SandboxGuard::new([&root]).unwrap());
    let dir = root.join(format!("prism-fileops-{}", std::process::id()));
    fs.create_dir_all(&dir.join("empty").join("deeper"))
        .unwrap();
    fs.write(&dir.join("old.bin"), &[7u8; 4096]).unwrap();
    fs.write(&dir.join("changed.txt"), b"before").unwrap();

    // A catalog of that folder.
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let info = VolumeInfo::dev_folder(&dir.to_string_lossy());
    let volume = store
        .write(move |w| store::insert_volume(w, &info, VolumeStatus::New))
        .unwrap();
    indexer::index(
        &store,
        &WalkEnumerator::default(),
        &IndexRequest {
            volume,
            path: &dir,
            relative: &[],
            target: ReconcileTarget::VolumeRoot,
            policy: &ExclusionPolicy::with_defaults(),
        },
        &CancelToken::new(),
        &IndexProgress::default(),
    )
    .unwrap();
    let id = |rel: &str| {
        store
            .read(|c| store::resolve_relative_path(c, volume, rel))
            .unwrap()
            .unwrap()
            .id
    };
    let ids = [id("old.bin"), id("changed.txt"), id("empty")];
    fs.write(&dir.join("changed.txt"), b"after, and longer")
        .unwrap();

    empty_recycle_bin_of(&root).ok();
    let before = recycle_bin_info(&root).unwrap();
    let report = delete_entries(
        &store,
        &DeleteRequest {
            volume,
            root: &dir,
            entries: &ids,
            disposal: Disposal::RecycleBin,
            folders: FolderCheck::NoFiles,
            verify_hash: false,
        },
        &ShellRemover,
        Some(&SandboxGuard::new([&dir]).unwrap()),
    )
    .unwrap();

    let outcomes: Vec<&Outcome> = report.items.iter().map(|i| &i.outcome).collect();
    assert_eq!(
        outcomes,
        [&Outcome::Done, &Outcome::SkippedChanged, &Outcome::Done]
    );
    assert!(!dir.join("old.bin").exists());
    assert!(dir.join("changed.txt").exists());
    assert!(!dir.join("empty").exists());
    assert_eq!(recycle_bin_info(&root).unwrap().0, before.0 + 2);
    let log = store.read(|c| store::actions_log(c, 10)).unwrap();
    assert_eq!(log.len(), 3);
    assert!(store.read(store::check_consistency).unwrap().is_empty());

    fs.remove_dir_all(&dir).unwrap();
    empty_recycle_bin_of(&root).unwrap();
}
