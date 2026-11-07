//! Safety of the app's destructive operations (CLAUDE.md, "App di sviluppo protetta"): a
//! development build refuses to delete anything outside its sandbox, before touching the disk.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::indexer::{self, IndexProgress, IndexRequest};
use prism_core::jobs::JobEngine;
use prism_core::model::{VolumeId, VolumeInfo, VolumeStatus};
use prism_core::reconcile::ReconcileTarget;
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::scan::WalkEnumerator;
use prism_core::store::{self, Store};
use prism_core::volumes::DetectedVolume;

use crate::netprobe::{NetProbe, ProbeTiming};
use crate::notices::Notices;
use crate::state::{AppState, debug_guard_roots};

fn state(store: Store, dev: bool, data_dir: &Path) -> AppState {
    AppState {
        store,
        engine: JobEngine::start(1),
        online: Mutex::new(HashMap::new()),
        elevated: false,
        data_dir: data_dir.to_path_buf(),
        dev,
        net: Arc::new(NetProbe::new(
            Arc::new(|_: &str| None),
            ProbeTiming::default(),
        )),
        notices: Mutex::new(Notices::default()),
        app_id: String::new(),
        search: Arc::new(prism_core::search::live::LiveIndexes::default()),
        shortcut_error: Mutex::new(None),
    }
}

/// A folder outside every root of the development sandbox: in the build folder of the
/// workspace (not the temporary folder, not the development data).
fn outside_folder() -> PathBuf {
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target");
    let dir = target.join(format!("guard-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn debug_build_refuses_deletions_outside_the_sandbox() {
    let dir = outside_folder();
    let file = dir.join("precious.txt");
    std::fs::write(&file, b"must survive").unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let info = VolumeInfo::dev_folder(&dir.to_string_lossy());
    let detected = DetectedVolume {
        info: info.clone(),
        mounts: vec![dir.to_string_lossy().into_owned()],
        disks: Vec::new(),
    };
    let volume: VolumeId = store
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
    let id = store
        .read(|c| store::resolve_relative_path(c, volume, "precious.txt"))
        .unwrap()
        .unwrap()
        .id;

    let app = state(store, true, tmp.path());
    app.online().insert(volume, detected);
    let err = app.delete_entries(volume, &[id], false, false).unwrap_err();
    assert_eq!(err.code, "delete_failed");
    assert!(err.message.contains("sandbox"), "{}", err.message);
    assert!(file.exists(), "nothing touched");
    let log = app.store.read(|c| store::actions_log(c, 10)).unwrap();
    assert!(log.is_empty(), "refused before acting: {log:?}");

    // Clean up the test folder through a guard of its own.
    let parent = dir.parent().unwrap().to_path_buf();
    GuardedFs::new(SandboxGuard::new([&parent]).unwrap())
        .remove_dir_all(&dir)
        .unwrap();
}

#[test]
fn release_builds_have_no_guard_and_debug_roots_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    assert!(
        state(store, false, tmp.path())
            .deletion_guard()
            .unwrap()
            .is_none()
    );

    let roots = debug_guard_roots(
        Some(PathBuf::from(r"Q:\no-such-test-volume")),
        std::env::temp_dir(),
        Some(tmp.path().to_path_buf()),
    );
    assert_eq!(roots, [std::env::temp_dir(), tmp.path().to_path_buf()]);
}
