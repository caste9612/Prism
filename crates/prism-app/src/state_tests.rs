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
        names: Arc::new(crate::netnames::ServerNames::new(
            Arc::new(|_| None),
            Arc::new(|_: &str| Vec::new()),
            Vec::new(),
        )),
        notices: Mutex::new(Notices::default()),
        app_id: String::new(),
        search: Arc::new(prism_core::search::live::LiveIndexes::default()),
        shortcut_error: Mutex::new(None),
        hashing: crate::hash_jobs::HashingState::default(),
        backup: crate::backup_state::BackupState::default(),
        thumbnails: crate::similar_views::Thumbnails::default(),
        identities: crate::identity::Identities::default(),
        health: crate::health_state::HealthState::default(),
        nas: crate::nas_state::NasState::new(
            Arc::new(crate::nas_state::MemoryLogins::default()),
            Arc::new(|_: &str, _: &str, _: &str| Err("unreachable".to_owned())),
        ),
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
            hashed: 0,
            images: 0,
            printed: 0,
            videos: 0,
            videos_printed: 0
        })
    );
    assert!(app.engine.jobs().is_empty());
    let waiting = |app: &AppState| {
        app.volume_views().unwrap()[0]
            .hashes
            .as_ref()
            .unwrap()
            .waiting
            .clone()
    };
    assert_eq!(waiting(&app).as_deref(), Some("paused"));
    // What is left, in the whole catalog and on one disk (the screen of another disk: nothing).
    assert_eq!(app.hashing.pending_files(None), 4);
    assert_eq!(app.hashing.pending_files(Some(volume)), 4);
    assert_eq!(app.hashing.pending_files(Some(VolumeId(volume.0 + 1))), 0);

    // Resumed but excluded by the user (F21-AC3): still nothing.
    hash_jobs::configure(&app, false, &[volume.0]);
    let _ = hash_jobs::tick(&app);
    assert!(app.engine.jobs().is_empty());
    assert_eq!(waiting(&app).as_deref(), Some("excluded"));

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
            hashed: 4,
            images: 0,
            printed: 0,
            videos: 0,
            videos_printed: 0
        })
    );
    let card = app.volume_views().unwrap();
    let hashes = card[0].hashes.as_ref().unwrap();
    assert_eq!((hashes.total, hashes.hashed, hashes.running), (4, 4, false));
    assert_eq!((hashes.waiting.as_deref(), hashes.skipped), (None, 0));
    let dups = app.store.read(|c| store::duplicate_files(c, 1)).unwrap();
    assert_eq!(dups.len(), 2, "a.bin and sub/b.bin");

    // Nothing left to do: no new job.
    let _ = hash_jobs::tick(&app);
    assert_eq!(app.engine.jobs().len(), 1);

    // A new file and an update of the disk: counted at the next tick, not minutes later.
    std::thread::sleep(std::time::Duration::from_millis(5));
    std::fs::write(disk.join("d.bin"), vec![9u8; 100 * 1024]).unwrap();
    indexer::index(
        &app.store,
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
    app.store
        .write(move |w| store::set_volume_status(w, volume, VolumeStatus::Indexed))
        .unwrap();
    assert!(hash_jobs::tick(&app));
    assert_eq!(
        app.hashing.progress(volume).map(|s| (s.total, s.hashed)),
        Some((5, 4))
    );
    // Hashed less than two minutes ago: the new file waits for the next round.
    assert_eq!(waiting(&app).as_deref(), Some("next"));
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
    // The same record for the "last actions" panel of the catalog.
    let recent = app.recent_actions(5).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].volume_id, Some(log[0].volume.unwrap().0));
    assert_eq!(
        (recent[0].size, recent[0].outcome.as_str()),
        (300 * 1024, "done")
    );
    assert_eq!(recent[0].path, log[0].path);
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

// F11 in the app: the background job prints the images after the hashes; similar images form a
// group, the largest first, with thumbnails of the connected files.
#[test]
fn similar_images_after_the_background_job() {
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use prism_core::jobs::JobState;

    let pattern = |w: u32, h: u32| {
        DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            Rgb([
                (x * 255 / w) as u8,
                (y * 255 / h) as u8,
                ((x + y) % 97) as u8,
            ])
        }))
    };
    let tmp = tempfile::tempdir().unwrap();
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(disk.join("Foto")).unwrap();
    pattern(400, 300)
        .save_with_format(disk.join("Foto").join("big.png"), ImageFormat::Png)
        .unwrap();
    pattern(400, 300)
        .resize_exact(200, 150, image::imageops::FilterType::Triangle)
        .save_with_format(disk.join("small.jpg"), ImageFormat::Jpeg)
        .unwrap();
    DynamicImage::ImageRgb8(RgbImage::from_fn(300, 300, |x, _| {
        Rgb([if x % 50 < 25 { 0 } else { 255 }, 0, 0])
    }))
    .save_with_format(disk.join("stripes.png"), ImageFormat::Png)
    .unwrap();

    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let (volume, detected) = add_folder_volume(&store, &disk);
    let app = Arc::new(state(store, true, tmp.path()));
    app.online().insert(volume, detected);
    let _ = crate::hash_jobs::tick(&app);
    let job = app.engine.jobs()[0].id;
    assert_eq!(app.engine.wait(job).unwrap().state, JobState::Done);
    let status = app.hashing.progress(volume).unwrap();
    assert_eq!((status.images, status.printed), (3, 3));

    let view = app.similar_images(8, None).unwrap();
    assert_eq!(view.total_groups, 1);
    assert_eq!(view.pending, 0);
    let files = &view.groups[0].files;
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, r"Foto\big.png");
    assert_eq!((files[0].width, files[0].height), (Some(400), Some(300)));
    assert_eq!(files[0].distance, 0);
    assert!(files[1].distance <= 8);
    assert!(files[1].online);
    // Filtered on another volume: nothing.
    assert_eq!(
        app.similar_images(8, Some(VolumeId(999)))
            .unwrap()
            .total_groups,
        0
    );

    let thumb = app
        .image_thumbnail(volume, EntryId(files[0].entry_id))
        .unwrap();
    let img = image::load_from_memory_with_format(&thumb, ImageFormat::Jpeg).unwrap();
    assert_eq!(img.width(), crate::similar_views::THUMB_SIDE);
    // Disconnected: no thumbnail (the group stays, from the catalog).
    app.online().remove(&volume);
    assert_eq!(
        app.image_thumbnail(volume, EntryId(files[1].entry_id))
            .unwrap_err()
            .code,
        "volume_offline"
    );
    assert_eq!(app.similar_images(8, None).unwrap().total_groups, 1);
}

// F12 in the app: the background job prints the videos after the images; the same clip encoded
// twice forms a group, another clip does not; thumbnails come from the middle frame.
#[test]
fn similar_videos_after_the_background_job() {
    use prism_core::jobs::JobState;
    use prism_win::video::testing::{MFVideoFormat_H264, MFVideoFormat_WMV3, write_clip};

    let tmp = tempfile::tempdir().unwrap();
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(disk.join("Video")).unwrap();
    write_clip(
        &disk.join("Video").join("gita.mp4"),
        640,
        360,
        2_000_000,
        &MFVideoFormat_H264,
        false,
    );
    write_clip(
        &disk.join("gita piccola.wmv"),
        320,
        180,
        400_000,
        &MFVideoFormat_WMV3,
        false,
    );
    write_clip(
        &disk.join("altro.mp4"),
        640,
        360,
        2_000_000,
        &MFVideoFormat_H264,
        true,
    );

    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let (volume, detected) = add_folder_volume(&store, &disk);
    let app = Arc::new(state(store, true, tmp.path()));
    app.online().insert(volume, detected);
    let _ = crate::hash_jobs::tick(&app);
    let job = app.engine.jobs()[0].id;
    assert_eq!(app.engine.wait(job).unwrap().state, JobState::Done);
    let status = app.hashing.progress(volume).unwrap();
    assert_eq!((status.videos, status.videos_printed), (3, 3));

    let view = app.similar_videos(6, None).unwrap();
    assert_eq!(view.total_groups, 1);
    let files = &view.groups[0].files;
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, r"Video\gita.mp4", "the largest first");
    assert_eq!((files[0].width, files[0].height), (Some(640), Some(360)));
    assert!(
        files[0]
            .duration_ms
            .is_some_and(|d| (3_900..=4_100).contains(&d))
    );
    assert!(files[1].distance <= 2);
    let thumb = app
        .video_thumbnail(volume, EntryId(files[0].entry_id))
        .unwrap();
    let img = image::load_from_memory_with_format(&thumb, image::ImageFormat::Jpeg).unwrap();
    assert_eq!(img.width(), crate::similar_views::THUMB_SIDE);
}

// F20-AC2: the package is shown before being written, contains the system report and the
// logs, and never replaces an existing file; development builds keep it in their data folder.
#[test]
fn diagnostic_package_preview_and_export() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let app = state(store, true, tmp.path());
    let preview = app.diagnostics_preview().unwrap();
    assert_eq!(preview.files[0].name, "system.txt");
    assert!(preview.system.starts_with("Prism "));
    assert!(preview.system.contains("Catalog: 0 disks"));
    assert!(preview.system.contains("Windows"));
    assert!(
        preview
            .destination
            .starts_with(&*tmp.path().to_string_lossy())
    );

    let first = app.export_diagnostics().unwrap();
    let second = app.export_diagnostics().unwrap();
    assert_ne!(first, second);
    assert!(first.starts_with(tmp.path().join("diagnostics")));
    let zip = std::fs::read(&first).unwrap();
    assert_eq!(&zip[..4], b"PK\x03\x04");
    assert!(zip.windows(10).any(|w| w == b"system.txt"));
}

// F21-AC4 in the app: the reset empties the catalog and the state kept in memory, keeps the
// settings except the choices tied to removed disks, and touches no file.
#[test]
fn reset_of_the_catalog() {
    use crate::backup_views::SideInput;

    let tmp = tempfile::tempdir().unwrap();
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(&disk).unwrap();
    std::fs::write(disk.join("a.txt"), b"content").unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let (volume, detected) = add_folder_volume(&store, &disk);
    let app = Arc::new(state(store, true, tmp.path()));
    app.online().insert(volume, detected);
    let _ = crate::hash_jobs::tick(&app);
    let mut settings = app.settings();
    settings.language = "it".into();
    settings.hashing_excluded = vec![volume.0];
    app.save_settings(&settings).unwrap();
    let side = SideInput {
        volume_id: volume.0,
        folder: String::new(),
    };
    let report = app.backup_compare(&side, &side, false);
    assert!(report.is_err(), "same folder: overlap");

    assert_eq!(app.reset_catalog().unwrap(), 1);
    assert!(app.engine.jobs().iter().all(|j| j.state.is_finished()));
    assert!(app.volume_views().unwrap().is_empty());
    assert!(app.online().is_empty());
    assert!(app.hashing.progress(volume).is_none());
    let after = app.settings();
    assert_eq!(after.language, "it");
    assert!(after.hashing_excluded.is_empty());
    assert!(disk.join("a.txt").is_file());
}

// F02-AC3: a disk that could be two catalog disks (clones with the same serial number) waits for
// the user's choice; the choice is applied at the next detection and then recognised by itself.
#[test]
fn ambiguous_disks_wait_for_the_users_choice() {
    use crate::monitor::apply_detection;
    use prism_core::model::VolumeKind;

    let info = |guid: &str, capacity: u64, mount: &str| {
        let mut i = VolumeInfo::dev_folder(mount);
        i.kind = VolumeKind::Fixed;
        i.fs_type = Some("NTFS".into());
        i.fs_serial = Some(42);
        i.volume_guid = Some(guid.into());
        i.capacity = Some(capacity);
        i
    };
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let (a, b) = (info("A", 100, "E:\\"), info("B", 200, "F:\\"));
    let a = store
        .write(move |w| store::insert_volume(w, &a, VolumeStatus::Indexed))
        .unwrap();
    let b = store
        .write(move |w| store::insert_volume(w, &b, VolumeStatus::Indexed))
        .unwrap();
    let app = state(store, true, tmp.path());
    let detected = |guid: &str| DetectedVolume {
        info: info(guid, 300, "G:\\"),
        mounts: vec!["G:\\".into()],
        disks: Vec::new(),
    };

    let (changed, _) = apply_detection(&app, vec![detected("C")]).unwrap();
    assert!(changed);
    assert!(
        app.online().is_empty(),
        "not recorded until the user decides"
    );
    let waiting = app.ambiguous_volumes().unwrap();
    assert_eq!(waiting.len(), 1);
    let ids: Vec<i64> = waiting[0].candidates.iter().map(|c| c.volume_id).collect();
    assert_eq!(ids, [a.0, b.0]);
    assert_eq!(
        app.resolve_ambiguous(&waiting[0].key, Some(VolumeId(999)))
            .unwrap_err()
            .code,
        "volume_not_found"
    );
    // Nothing changes while the user has not chosen.
    let (changed, _) = apply_detection(&app, vec![detected("C")]).unwrap();
    assert!(!changed);

    // "It is the second one": known from now on, with the GUID of the connected disk.
    app.resolve_ambiguous(&waiting[0].key, Some(b)).unwrap();
    let (changed, came_back) = apply_detection(&app, vec![detected("C")]).unwrap();
    assert!(changed);
    assert_eq!(came_back, [b], "an indexed disk that comes back is updated");
    assert!(app.online().contains_key(&b));
    assert!(app.ambiguous_volumes().unwrap().is_empty());
    let recorded = app.store.read(|c| store::volume(c, b)).unwrap().unwrap();
    assert_eq!(recorded.info.volume_guid.as_deref(), Some("C"));
    let (changed, _) = apply_detection(&app, vec![detected("C")]).unwrap();
    assert!(!changed, "recognised by itself");

    // Another clone, declared a new disk.
    let (_, _) = apply_detection(&app, vec![detected("C"), detected("D")]).unwrap();
    let waiting = app.ambiguous_volumes().unwrap();
    assert_eq!(waiting.len(), 1);
    app.resolve_ambiguous(&waiting[0].key, None).unwrap();
    let _ = apply_detection(&app, vec![detected("C"), detected("D")]).unwrap();
    assert_eq!(app.online().len(), 2);
    assert_eq!(app.store.read(store::volumes).unwrap().len(), 3);
    assert!(app.ambiguous_volumes().unwrap().is_empty());
}

// F17 in the app: the disk of a connected volume is read (the system disk of this machine, read
// only); a reading is stored once and not again while nothing changes; the card shows a badge.
#[test]
fn disk_health_of_a_connected_volume() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let (volume, mut detected) = add_folder_volume(&store, tmp.path());
    detected.disks = vec![0];
    let app = state(store, true, tmp.path());
    app.online().insert(volume, detected);

    let (_, changed) = crate::health_state::tick(&app);
    let Some(view) = app.disk_health(volume).unwrap() else {
        // No physical disk 0 visible from here (unusual): nothing else to check.
        return;
    };
    assert!(changed);
    assert!(["good", "warning", "critical", "unavailable"].contains(&view.badge.status.as_str()));
    let card = app.volume_views().unwrap();
    assert_eq!(
        card[0].health.as_ref().map(|h| h.status.clone()),
        Some(view.badge.status.clone())
    );
    if view.badge.status == "unavailable" {
        assert!(
            view.badge.unavailable.is_some(),
            "F17-AC4: the reason is said"
        );
        assert!(view.history.is_empty());
    } else {
        assert_eq!(view.history.len(), 1);
        let _ = crate::health_state::tick(&app);
        let again = app.disk_health(volume).unwrap().unwrap();
        assert_eq!(
            again.history.len(),
            1,
            "nothing changed: no new reading stored"
        );
    }
}

// F18-AC1: a path of the Explorer menu is found in the catalog: its volume and the deepest
// catalog folder that contains it (case does not matter; a folder not indexed yet gives its
// nearest indexed parent).
#[test]
fn paths_of_the_explorer_menu() {
    let tmp = tempfile::tempdir().unwrap();
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(disk.join("Foto").join("2019")).unwrap();
    std::fs::write(disk.join("Foto").join("2019").join("a.jpg"), b"x").unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let (volume, detected) = add_folder_volume(&store, &disk);
    let app = state(store, true, tmp.path());
    app.online().insert(volume, detected);

    let at = app
        .locate_path(&disk.join("Foto").join("2019").to_string_lossy())
        .unwrap();
    assert_eq!(at.volume_id, volume.0);
    assert_eq!(at.folder, r"Foto\2019");
    let upper = disk.join("FOTO").to_string_lossy().into_owned();
    assert_eq!(app.locate_path(&upper).unwrap().folder, "Foto");
    let newer = disk.join("Foto").join("2019").join("not indexed yet");
    assert_eq!(
        app.locate_path(&newer.to_string_lossy()).unwrap().folder,
        r"Foto\2019"
    );
    assert_eq!(app.locate_path(&disk.to_string_lossy()).unwrap().folder, "");
    assert_eq!(
        app.locate_path(r"Q:\elsewhere").unwrap_err().code,
        "not_in_catalog"
    );
}

// REVIEW D59: a network share of the catalog is given a new path by hand only when the path is a
// share path, not another share of the catalog, and answers there.
#[test]
fn a_share_moves_to_a_new_path_only_when_it_answers() {
    use prism_core::model::VolumeKind;

    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let share = |unc: &str| VolumeInfo {
        kind: VolumeKind::Network,
        unc_path: Some(unc.to_owned()),
        ..VolumeInfo::dev_folder(unc)
    };
    let (photos, disk) = {
        let (a, b) = (
            share(r"\\192.168.1.65\Photos"),
            share(r"\\192.168.1.65\Music"),
        );
        let c = VolumeInfo::dev_folder(&tmp.path().to_string_lossy());
        store
            .write(move |w| {
                let photos = store::insert_volume(w, &a, VolumeStatus::Indexed)?;
                store::insert_volume(w, &b, VolumeStatus::Indexed)?;
                Ok((photos, store::insert_volume(w, &c, VolumeStatus::Indexed)?))
            })
            .unwrap()
    };
    let app = state(store, true, tmp.path());
    let code = |r: Result<(), crate::dto::ApiError>| r.unwrap_err().code;
    assert_eq!(
        code(app.move_share(photos, r"D:\Photos")),
        "share_path_invalid"
    );
    assert_eq!(code(app.move_share(photos, r"\\nas")), "share_path_invalid");
    assert_eq!(code(app.move_share(disk, r"\\nas\x")), "volume_not_network");
    assert_eq!(
        code(app.move_share(photos, r"\\192.168.1.65\music\")),
        "share_in_catalog"
    );
    // A server that does not exist: checked as unreachable, never saved.
    let check = app
        .check_share_path(photos, r"\\prism-no-such-server.invalid\Photos\")
        .unwrap();
    assert_eq!(check.path, r"\\prism-no-such-server.invalid\Photos");
    assert!(!check.reachable);
    assert_eq!(
        code(app.move_share(photos, r"\\prism-no-such-server.invalid\Photos")),
        "share_unreachable"
    );
    let v = app
        .store
        .read(move |c| store::volume(c, photos))
        .unwrap()
        .unwrap();
    assert_eq!(v.info.unc_path.as_deref(), Some(r"\\192.168.1.65\Photos"));
}

// REVIEW D64: the partitions of one physical disk and the shares of one server are grouped;
// the physical disk is remembered for disconnected disks.
#[test]
fn volumes_are_grouped_by_physical_disk_and_by_server() {
    use prism_core::model::VolumeKind;

    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let share = VolumeInfo {
        kind: VolumeKind::Network,
        unc_path: Some(r"\\ZimaOS\Foto".to_owned()),
        ..VolumeInfo::dev_folder(r"\\ZimaOS\Foto")
    };
    let mut c = VolumeInfo::dev_folder(r"C:\");
    c.fs_serial = Some(1);
    let mut d = VolumeInfo::dev_folder(r"D:\");
    d.fs_serial = Some(2);
    let (nas, c, d) = store
        .write(move |w| {
            Ok((
                store::insert_volume(w, &share, VolumeStatus::Indexed)?,
                store::insert_volume(w, &c, VolumeStatus::Indexed)?,
                store::insert_volume(w, &d, VolumeStatus::Indexed)?,
            ))
        })
        .unwrap();
    let key = "Samsung SSD 990 PRO 2TB|S7KH";
    store
        .write(move |w| {
            store::set_volume_disk(w, c, key)?;
            store::set_volume_disk(w, d, key)
        })
        .unwrap();
    let app = state(store, true, tmp.path());
    let views = app.volume_views().unwrap();
    let view = |id: VolumeId| views.iter().find(|v| v.id == id.0).unwrap();
    assert_eq!(
        view(c).group.as_deref(),
        Some("disk:Samsung SSD 990 PRO 2TB|S7KH")
    );
    assert_eq!(view(c).group, view(d).group);
    assert_eq!(
        view(d).disk_model.as_deref(),
        Some("Samsung SSD 990 PRO 2TB")
    );
    let nas = view(nas);
    assert_eq!(nas.group.as_deref(), Some("net:ZIMAOS"));
    assert_eq!(nas.server.as_deref(), Some("ZimaOS"));
    assert_eq!(nas.server_name, None, "no name learned for it");
    assert_eq!(
        nas.raid.as_ref().map(|r| r.storage.as_str()),
        None,
        "no NAS login"
    );

    // REVIEW D65: SSD or hard disk, also while the disk is away.
    let ssd = store::DiskMedia {
        media: "ssd".into(),
        rpm: None,
    };
    app.store
        .write(move |w| store::set_disk_media(w, key, &ssd))
        .unwrap();
    let views = app.volume_views().unwrap();
    let c = views.iter().find(|v| v.id == c.0).unwrap();
    assert_eq!((c.media.as_deref(), c.rpm), (Some("ssd"), None));
}

/// A ZimaOS with one RAID 1 of two hard disks; the second one healthy or not.
fn zimaos(second_healthy: bool) -> crate::zimaos::NasInfo {
    use crate::zimaos::{NasDisk, NasInfo, NasStorage};
    let disk = |serial: &str, model: &str, healthy: bool| NasDisk {
        serial: serial.into(),
        model: model.into(),
        kind: "hdd".into(),
        healthy,
        storage: Some("Big-Storage".into()),
        temperature: Some(30),
        ..NasDisk::default()
    };
    NasInfo {
        storages: vec![NasStorage {
            name: "Big-Storage".into(),
            mount: "/media/Big-Storage".into(),
            raid_level: Some(1),
            raid_status: Some("ok".into()),
            activity: Some("idle".into()),
            disks: vec!["A".into(), "B".into()],
            ..NasStorage::default()
        }],
        disks: vec![
            disk("A", "WD10EARS", true),
            disk("B", "ST1000DM003", second_healthy),
        ],
    }
}

// REVIEW D66: with the login of the NAS, a share shows its array and the disks of the array;
// a disk that stops being healthy is announced once; without the login nothing is read.
#[test]
fn a_nas_share_shows_its_raid_and_announces_new_problems_once() {
    use std::sync::atomic::{AtomicBool, Ordering};

    use prism_core::model::VolumeKind;

    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("catalog.db")).unwrap();
    let share = VolumeInfo {
        kind: VolumeKind::Network,
        unc_path: Some(r"\\192.168.1.65\Big-Storage".to_owned()),
        ..VolumeInfo::dev_folder(r"\\192.168.1.65\Big-Storage")
    };
    let volume = store
        .write(move |w| store::insert_volume(w, &share, VolumeStatus::Indexed))
        .unwrap();
    let healthy = Arc::new(AtomicBool::new(true));
    let h = Arc::clone(&healthy);
    let mut app = state(store, true, tmp.path());
    app.nas = crate::nas_state::NasState::new(
        Arc::new(crate::nas_state::MemoryLogins::default()),
        Arc::new(move |_: &str, _: &str, password: &str| {
            if password == "right" {
                Ok(zimaos(h.load(Ordering::SeqCst)))
            } else {
                Err("login_failed".to_owned())
            }
        }),
    );

    let servers = app.nas_servers().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(
        (servers[0].server.as_str(), servers[0].shares),
        ("192.168.1.65", 1)
    );
    assert!(!servers[0].connected);
    let code = |r: Result<(), crate::dto::ApiError>| r.unwrap_err().code;
    assert_eq!(
        code(app.nas_connect("10.0.0.9", "me", "right")),
        "nas_unknown_server"
    );
    assert_eq!(
        code(app.nas_connect("192.168.1.65", "me", "wrong")),
        "nas_login_failed"
    );
    assert!(!app.nas_servers().unwrap()[0].connected);

    app.nas_connect("192.168.1.65", "me", "right").unwrap();
    let raid = app.volume_views().unwrap()[0].raid.clone().unwrap();
    assert_eq!(
        (raid.storage.as_str(), raid.level, raid.state.as_str()),
        ("Big-Storage", Some(1), "ok")
    );
    assert_eq!(raid.disks, 2);
    let health = app.nas_health(volume).unwrap().unwrap();
    assert!(health.connected);
    assert_eq!(health.storage.unwrap().name, "Big-Storage");
    assert_eq!(health.disks.len(), 2);
    assert_eq!(app.nas_servers().unwrap()[0].state.as_deref(), Some("ok"));

    assert_eq!(app.nas_tick().0, Vec::<String>::new());
    healthy.store(false, Ordering::SeqCst);
    let (news, changed) = app.nas_tick();
    assert_eq!(news, ["ST1000DM003 (Big-Storage)"]);
    assert!(changed);
    assert_eq!(
        app.volume_views().unwrap()[0].raid.clone().unwrap().state,
        "attention"
    );
    assert!(app.nas_tick().0.is_empty(), "announced once");

    app.nas_disconnect("192.168.1.65").unwrap();
    assert!(app.volume_views().unwrap()[0].raid.is_none());
    assert!(!app.nas_health(volume).unwrap().unwrap().connected);
    assert!(app.nas_tick().0.is_empty());
}

// REVIEW D62: two paths to one file (here a hard link; on a NAS two shares showing one disk)
// look like two copies in the catalog; deleting "one" would delete the only one: refused before
// anything is touched.
#[test]
fn one_file_under_two_paths_is_not_deleted_as_a_duplicate() {
    use prism_core::jobs::JobState;
    use prism_core::model::VolumeKind;

    let tmp = tempfile::tempdir().unwrap();
    let disk = tmp.path().join("disk");
    std::fs::create_dir_all(disk.join("other")).unwrap();
    let file = disk.join("video.mp4");
    std::fs::write(&file, vec![5u8; 300 * 1024]).unwrap();
    let link = disk.join("other").join("video.mp4");
    std::fs::hard_link(&file, &link).unwrap();

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

    let view = app
        .duplicates(&crate::duplicates_views::DupFilterInput::default(), &[])
        .unwrap();
    assert_eq!(view.total_groups, 1, "the catalog sees two copies");
    let other = view.groups[0]
        .files
        .iter()
        .find(|f| f.path.starts_with("other"))
        .unwrap();
    assert_eq!(
        app.delete_duplicates(&[EntryId(other.entry_id)], false, true)
            .unwrap_err()
            .code,
        "same_file"
    );
    assert!(file.exists() && link.exists());
    assert!(
        app.store
            .read(|c| store::actions_log(c, 10))
            .unwrap()
            .is_empty()
    );
}
