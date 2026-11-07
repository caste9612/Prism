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
use prism_core::model::{EntryId, VolumeId, VolumeInfo, VolumeStatus};
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
        hashing: crate::hash_jobs::HashingState::default(),
        backup: crate::backup_state::BackupState::default(),
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

// F06: the background hashes of a connected, indexed volume; nothing while paused, one job of
// background priority otherwise, progress counted per volume and shown on the card.
#[test]
fn background_hashing_runs_pauses_and_counts() {
    use crate::hash_jobs::{self, HashStatus};
    use prism_core::jobs::{JobState, Priority};
    use std::sync::atomic::Ordering;

    let tmp = tempfile::tempdir().unwrap();
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(disk.join("sub")).unwrap();
    let same = vec![7u8; 200 * 1024];
    std::fs::write(disk.join("a.bin"), &same).unwrap();
    std::fs::write(disk.join("sub").join("b.bin"), &same).unwrap();
    std::fs::write(disk.join("c.bin"), vec![8u8; 200 * 1024]).unwrap();
    std::fs::write(disk.join("notes.txt"), b"unique").unwrap();

    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let info = VolumeInfo::dev_folder(&disk.to_string_lossy());
    let detected = DetectedVolume {
        info: info.clone(),
        mounts: vec![disk.to_string_lossy().into_owned()],
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
            path: &disk,
            relative: &[],
            target: ReconcileTarget::VolumeRoot,
            policy: &ExclusionPolicy::with_defaults(),
        },
        &CancelToken::new(),
        &IndexProgress::default(),
    )
    .unwrap();
    let app = Arc::new(state(store, true, tmp.path()));
    app.online().insert(volume, detected);

    // Paused: the progress is counted, no job starts.
    app.hashing.paused.store(true, Ordering::Relaxed);
    assert!(hash_jobs::tick(&app));
    assert_eq!(
        app.hashing.progress(volume),
        Some(HashStatus {
            total: 4,
            hashed: 0
        })
    );
    assert!(app.engine.jobs().is_empty());

    // Resumed but excluded by the user (F21-AC3): still nothing.
    hash_jobs::configure(&app, false, &[volume.0]);
    let _ = hash_jobs::tick(&app);
    assert!(app.engine.jobs().is_empty());

    // Resumed: one background job hashes the volume.
    hash_jobs::configure(&app, false, &[]);
    let _ = hash_jobs::tick(&app);
    let jobs = app.engine.jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, "hash");
    assert_eq!(jobs[0].priority, Priority::Background);
    assert!(
        app.foreground_jobs(volume).is_empty(),
        "never blocks an update"
    );
    assert_eq!(app.engine.wait(jobs[0].id).unwrap().state, JobState::Done);
    assert_eq!(
        app.hashing.progress(volume),
        Some(HashStatus {
            total: 4,
            hashed: 4
        })
    );
    let card = app.volume_views().unwrap();
    let hashes = card[0].hashes.as_ref().unwrap();
    assert_eq!((hashes.total, hashes.hashed, hashes.running), (4, 4, false));
    let dups = app.store.read(|c| store::duplicate_files(c, 1)).unwrap();
    assert_eq!(dups.len(), 2, "a.bin and sub/b.bin");

    // Nothing left to do: no new job.
    let _ = hash_jobs::tick(&app);
    assert_eq!(app.engine.jobs().len(), 1);
}

// F10/F16 in the app: groups with the copies the rules propose, the last-copy rule, the
// deletion of a copy (permanent: a removable test volume in the temporary folder, inside the
// development sandbox) and the catalog updated after it.
#[test]
fn duplicates_are_grouped_selected_and_deleted_safely() {
    use crate::duplicates_views::{DupFilterInput, RuleInput};
    use prism_core::jobs::JobState;
    use prism_core::model::VolumeKind;

    let tmp = tempfile::tempdir().unwrap();
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(disk.join("copies").join("old")).unwrap();
    let same = vec![3u8; 300 * 1024];
    std::fs::write(disk.join("photo.jpg"), &same).unwrap();
    let copy = disk.join("copies").join("old").join("photo.jpg");
    std::fs::write(&copy, &same).unwrap();
    std::fs::write(disk.join("unique.txt"), b"only one").unwrap();

    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let mut info = VolumeInfo::dev_folder(&disk.to_string_lossy());
    info.kind = VolumeKind::Removable;
    let detected = DetectedVolume {
        info: info.clone(),
        mounts: vec![disk.to_string_lossy().into_owned()],
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
            path: &disk,
            relative: &[],
            target: ReconcileTarget::VolumeRoot,
            policy: &ExclusionPolicy::with_defaults(),
        },
        &CancelToken::new(),
        &IndexProgress::default(),
    )
    .unwrap();
    let app = Arc::new(state(store, true, tmp.path()));
    app.online().insert(volume, detected);
    let _ = crate::hash_jobs::tick(&app);
    let job = app.engine.jobs()[0].id;
    assert_eq!(app.engine.wait(job).unwrap().state, JobState::Done);

    let keep_short = [RuleInput {
        kind: "keepShortestPath".into(),
        volume_id: None,
        text: None,
    }];
    let view = app
        .duplicates(&DupFilterInput::default(), &keep_short)
        .unwrap();
    assert_eq!(view.total_groups, 1);
    assert_eq!(view.total_recoverable, 300 * 1024);
    assert_eq!(view.pending_files, 0);
    let group = &view.groups[0];
    assert_eq!(group.kind, "images");
    assert_eq!(group.files.len(), 2);
    let deep = group
        .files
        .iter()
        .find(|f| f.path == r"copies\old\photo.jpg")
        .unwrap();
    assert_eq!(group.selected, [deep.entry_id]);
    assert!(deep.full_path.ends_with(r"disk\copies\old\photo.jpg"));
    assert!(
        deep.permanent && deep.online,
        "a removable disk has no Recycle Bin"
    );
    // A filter that matches nothing.
    let videos = DupFilterInput {
        category: Some("videos".into()),
        ..DupFilterInput::default()
    };
    assert_eq!(app.duplicates(&videos, &[]).unwrap().total_groups, 0);

    // F16-AC4: every copy, or a unique file, is refused before anything is touched.
    let all: Vec<EntryId> = group.files.iter().map(|f| EntryId(f.entry_id)).collect();
    assert_eq!(
        app.delete_duplicates(&all, false, true).unwrap_err().code,
        "last_copy"
    );
    let unique = app
        .store
        .read(|c| store::resolve_relative_path(c, volume, "unique.txt"))
        .unwrap()
        .unwrap()
        .id;
    assert_eq!(
        app.delete_duplicates(&[unique], false, true)
            .unwrap_err()
            .code,
        "last_copy"
    );
    assert!(copy.exists() && disk.join("photo.jpg").exists());
    // No Recycle Bin on a removable disk: only with the permanent deletion confirmed.
    let doomed = [EntryId(deep.entry_id)];
    assert_eq!(
        app.delete_duplicates(&doomed, false, false)
            .unwrap_err()
            .code,
        "needs_permanent"
    );
    assert!(copy.exists());

    let done = app.delete_duplicates(&doomed, false, true).unwrap();
    assert_eq!((done.done, done.skipped, done.failed), (1, 0, 0));
    assert_eq!(done.freed, 300 * 1024);
    assert!(!copy.exists());
    assert!(disk.join("photo.jpg").exists(), "the copy kept stays");
    let after = app.duplicates(&DupFilterInput::default(), &[]).unwrap();
    assert_eq!(after.total_groups, 0);
    let log = app.store.read(|c| store::actions_log(c, 10)).unwrap();
    assert_eq!(log.len(), 1);
}

// D31: the periodic journal update does not reserve the disk, so the content analysis does
// not stop for it every 15 s; indexing and updates do.
#[test]
fn journal_updates_do_not_reserve_the_disk() {
    use crate::state::IndexKind;
    let detected = DetectedVolume {
        info: VolumeInfo::dev_folder(r"Q:\"),
        mounts: vec![r"Q:\".to_owned()],
        disks: vec![3],
    };
    assert!(IndexKind::Journal.disks(&detected).is_empty());
    assert_eq!(IndexKind::Update.disks(&detected), ["disk3"]);
    assert_eq!(IndexKind::Index.disks(&detected), ["disk3"]);
}

/// Indexes a folder as a development volume of the catalog and returns it with its detection.
fn add_folder_volume(store: &Store, path: &Path) -> (VolumeId, DetectedVolume) {
    let info = VolumeInfo::dev_folder(&path.to_string_lossy());
    let detected = DetectedVolume {
        info: info.clone(),
        mounts: vec![path.to_string_lossy().into_owned()],
        disks: Vec::new(),
    };
    let volume: VolumeId = store
        .write(move |w| store::insert_volume(w, &info, VolumeStatus::New))
        .unwrap();
    indexer::index(
        store,
        &WalkEnumerator::default(),
        &IndexRequest {
            volume,
            path,
            relative: &[],
            target: ReconcileTarget::VolumeRoot,
            policy: &ExclusionPolicy::with_defaults(),
        },
        &CancelToken::new(),
        &IndexProgress::default(),
    )
    .unwrap();
    (volume, detected)
}

fn wait_all(app: &AppState) {
    while let Some(job) = app
        .engine
        .jobs()
        .into_iter()
        .find(|j| !j.state.is_finished())
    {
        let _ = app.engine.wait(job.id);
    }
}

// F13/F15 in the app: a check finds the missing files, the tree groups them, the preview
// counts them, the copy job copies them into the sandbox (temporary folder) and refreshes the
// destination, and the same check then finds nothing missing.
#[test]
fn backup_check_then_copy_of_the_missing_files() {
    use crate::backup_views::{ReportFilterInput, SideInput};
    use prism_core::jobs::JobState;

    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(src.join("Foto").join("2019")).unwrap();
    std::fs::create_dir_all(dst.join("Vecchio")).unwrap();
    std::fs::write(
        src.join("Foto").join("2019").join("mare.jpg"),
        vec![1u8; 40_000],
    )
    .unwrap();
    std::fs::write(src.join("Foto").join("monti.jpg"), vec![2u8; 30_000]).unwrap();
    std::fs::write(src.join("nota.txt"), b"hello").unwrap();
    // Already backed up elsewhere on the destination (same name, size and date).
    std::fs::copy(src.join("nota.txt"), dst.join("Vecchio").join("nota.txt")).unwrap();
    let modified = std::fs::metadata(src.join("nota.txt"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(dst.join("Vecchio").join("nota.txt"))
        .unwrap()
        .set_modified(modified)
        .unwrap();

    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let (a, da) = add_folder_volume(&store, &src);
    let (b, db) = add_folder_volume(&store, &dst);
    let app = Arc::new(state(store, true, tmp.path()));
    app.online().insert(a, da);
    app.online().insert(b, db);
    let source = SideInput {
        volume_id: a.0,
        folder: String::new(),
    };
    let dest = SideInput {
        volume_id: b.0,
        folder: String::new(),
    };

    let check = app
        .backup_verify(&source, std::slice::from_ref(&dest))
        .unwrap();
    assert_eq!(check.total.files, 3);
    assert_eq!(check.by_metadata.files, 1);
    assert_eq!((check.missing.files, check.missing.bytes), (2, 70_000));
    // F13-AC4: by folder, with search.
    let top = app
        .backup_folder(check.report_id, "", &ReportFilterInput::default())
        .unwrap();
    assert_eq!(top.folders.len(), 1);
    assert_eq!(top.folders[0].name, "Foto");
    assert_eq!(top.folders[0].tallies[0].files, 2);
    let foto = app
        .backup_folder(check.report_id, "Foto", &ReportFilterInput::default())
        .unwrap();
    assert_eq!(foto.files[0].name, "monti.jpg");
    assert_eq!(foto.folders[0].key, "2019");
    let searched = app
        .backup_folder(
            check.report_id,
            "",
            &ReportFilterInput {
                text: Some("MARE".into()),
                ..ReportFilterInput::default()
            },
        )
        .unwrap();
    assert_eq!(searched.tallies[0].files, 1);

    // F15-AC1/AC2: preview, then the copy below a chosen folder (F15-AC3).
    let target = SideInput {
        volume_id: b.0,
        folder: r"Copia\Prism".into(),
    };
    let preview = app.copy_preview(check.report_id, &target).unwrap();
    assert_eq!((preview.files, preview.bytes), (2, 70_000));
    assert!(preview.dest_path.ends_with(r"dst\Copia\Prism"));
    let job = app.copy_start(check.report_id, &target, false).unwrap();
    assert_eq!(app.engine.wait(job).unwrap().state, JobState::Done);
    wait_all(&app);
    let result = app.copy_result(job.0).unwrap();
    assert_eq!((result.copied, result.bytes), (2, 70_000));
    assert!(result.failed.is_empty() && !result.interrupted);
    assert!(
        dst.join("Copia")
            .join("Prism")
            .join("Foto")
            .join("2019")
            .join("mare.jpg")
            .is_file()
    );
    let log = app.store.read(|c| store::actions_log(c, 10)).unwrap();
    assert_eq!(log.iter().filter(|r| r.action == "copy").count(), 2);

    // The destination catalog was refreshed: nothing is missing any more.
    let again = app
        .backup_verify(&source, std::slice::from_ref(&dest))
        .unwrap();
    assert_eq!(again.missing.files, 0);

    // A copy into the source itself is refused.
    let inside = SideInput {
        volume_id: a.0,
        folder: "Foto".into(),
    };
    assert_eq!(
        app.copy_preview(check.report_id, &inside).unwrap_err().code,
        "copy_overlap"
    );
}

// CLAUDE.md: a development build copies only into its sandbox.
#[test]
fn debug_build_refuses_copies_outside_the_sandbox() {
    use crate::backup_views::SideInput;

    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.txt"), b"abc").unwrap();
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join(format!("copy-guard-test-{}", std::process::id()));
    std::fs::create_dir_all(&target).unwrap();
    let target = target.canonicalize().unwrap();

    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let (a, da) = add_folder_volume(&store, &src);
    let (b, db) = add_folder_volume(&store, &target);
    let app = Arc::new(state(store, true, tmp.path()));
    app.online().insert(a, da);
    app.online().insert(b, db);
    let check = app
        .backup_verify(
            &SideInput {
                volume_id: a.0,
                folder: String::new(),
            },
            &[SideInput {
                volume_id: b.0,
                folder: String::new(),
            }],
        )
        .unwrap();
    let dest = SideInput {
        volume_id: b.0,
        folder: "Copia".into(),
    };
    assert_eq!(
        app.copy_preview(check.report_id, &dest).unwrap_err().code,
        "outside_sandbox"
    );
    assert_eq!(
        app.copy_start(check.report_id, &dest, false)
            .unwrap_err()
            .code,
        "outside_sandbox"
    );
    assert!(!target.join("Copia").exists());
    let parent = target.parent().unwrap().to_path_buf();
    GuardedFs::new(SandboxGuard::new([&parent]).unwrap())
        .remove_dir_all(&target)
        .unwrap();
}
