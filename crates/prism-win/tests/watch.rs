//! The folder watcher end to end, without administrator rights: index a temporary folder,
//! change it, update the catalog from the watcher's batches, and compare with a full indexing
//! (F05-AC1 for volumes without a change journal, F05-AC3). Changes go through the sandbox
//! guard, inside the temporary folder.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::indexer::{self, IndexProgress, IndexRequest};
use prism_core::model::{VolumeId, VolumeInfo, VolumeStatus};
use prism_core::reconcile::ReconcileTarget;
use prism_core::refresh::refresh_folders;
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::scan::WalkEnumerator;
use prism_core::store::{self, Store};
use prism_win::watch::FolderWatcher;

fn catalog() -> (tempfile::TempDir, Store, VolumeId) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("c.db")).unwrap();
    let info = VolumeInfo::dev_folder("sandbox");
    let v = store
        .write(move |w| store::insert_volume(w, &info, VolumeStatus::New))
        .unwrap();
    (dir, store, v)
}

fn index(store: &Store, v: VolumeId, root: &Path) {
    indexer::index(
        store,
        &WalkEnumerator::with_threads(2),
        &IndexRequest {
            volume: v,
            path: root,
            relative: &[],
            target: ReconcileTarget::VolumeRoot,
            policy: &ExclusionPolicy::with_defaults(),
        },
        &CancelToken::new(),
        &IndexProgress::default(),
    )
    .unwrap();
}

#[test]
fn f05_ac1_watcher_batches_keep_the_catalog_up_to_date() {
    let sandbox = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(sandbox.path()).unwrap();
    let fs = GuardedFs::new(SandboxGuard::new([&root]).unwrap());
    fs.create_dir_all(&root.join(r"docs\old")).unwrap();
    fs.write(&root.join(r"docs\a.txt"), b"aaa").unwrap();
    fs.write(&root.join(r"docs\old\b.txt"), b"bbbb").unwrap();
    fs.write(&root.join("top.txt"), b"t").unwrap();

    let (_c, store, v) = catalog();
    index(&store, v, &root);
    let watcher = FolderWatcher::start(&root).unwrap();

    // Changes in subfolders and at the root.
    fs.write(&root.join(r"docs\new.txt"), b"new file").unwrap();
    fs.rename(&root.join(r"docs\old"), &root.join(r"docs\archive"))
        .unwrap();
    fs.write(&root.join(r"docs\archive\b.txt"), b"rewritten, longer")
        .unwrap();
    fs.remove_file(&root.join(r"docs\a.txt")).unwrap();
    fs.create_dir_all(&root.join(r"photos\2024")).unwrap();
    fs.write(&root.join(r"photos\2024\x.jpg"), &[7u8; 5000])
        .unwrap();
    fs.remove_file(&root.join("top.txt")).unwrap();

    let mut changed = Vec::new();
    let mut rescan = false;
    while let Some(batch) = watcher.next_batch(Duration::from_secs(3), Duration::from_millis(400)) {
        changed.extend(batch.paths);
        rescan |= batch.rescan;
    }
    assert!(!changed.is_empty(), "the watcher saw the changes");
    refresh_folders(
        &store,
        v,
        &root,
        &WalkEnumerator::with_threads(2),
        &ExclusionPolicy::with_defaults(),
        &changed,
        rescan,
        &CancelToken::new(),
        &IndexProgress::default(),
    )
    .unwrap();
    assert!(store.read(store::check_consistency).unwrap().is_empty());

    let (_f, fresh, fv) = catalog();
    index(&fresh, fv, &root);
    assert_eq!(
        store.read(|c| store::snapshot(c, v)).unwrap(),
        fresh.read(|c| store::snapshot(c, fv)).unwrap()
    );
}
