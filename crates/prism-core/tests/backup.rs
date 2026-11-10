//! Backups end to end on temporary folders (F13–F15): verification against hashed and
//! unhashed destinations, comparison by path and by content, verified copy with conflicts,
//! cancellation and resumption. Every write is inside the sandbox of the test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{Sandbox, TestCatalog};
use prism_core::backup::copy::{
    Conflict, CopyItem, CopyOutcome, CopyRequest, PART_SUFFIX, copy_files,
};
use prism_core::backup::{
    BackupError, Evidence, MatchMode, Side, Status, Tally, compare, folder_view, verify,
};
use prism_core::cancel::CancelToken;
use prism_core::hashing::{HashProgress, hash_volume};
use prism_core::jobs::JobProgress;
use prism_core::model::VolumeId;
use prism_core::sandbox::SandboxGuard;
use prism_core::store;

fn bytes(seed: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn put(sb: &Sandbox, rel: &str, data: &[u8], secs: u64) {
    let p = sb.path(rel);
    if let Some(parent) = p.parent().filter(|d| *d != sb.root()) {
        sb.fs.create_dir_all(parent).unwrap();
    }
    sb.fs.write(&p, data).unwrap();
    sb.fs.set_modified(&p, common::mtime(secs)).unwrap();
}

fn catalog(cat: &TestCatalog, sb: &Sandbox, hashed: bool) -> VolumeId {
    let v = cat.add_volume(&sb.root().to_string_lossy());
    cat.index(v, sb.root());
    if hashed {
        hash_volume(
            &cat.store,
            v,
            sb.root(),
            None,
            &CancelToken::new(),
            &|| false,
            &HashProgress::default(),
        )
        .unwrap();
    }
    v
}

fn side(cat: &TestCatalog, volume: VolumeId, rel: &str) -> Side {
    let root = cat
        .store
        .read(|c| store::resolve_relative_path(c, volume, rel))
        .unwrap()
        .unwrap()
        .id;
    Side { volume, root }
}

#[test]
fn f13_files_without_a_copy_by_content_or_metadata() {
    let src = Sandbox::new();
    put(&src, r"Foto\mare.jpg", &bytes(1, 70_000), 10);
    put(&src, r"Foto\monti.jpg", &bytes(2, 50_000), 20);
    put(&src, r"Doc\nota.txt", &bytes(3, 900), 30);
    put(&src, r"Doc\vuoto.txt", &bytes(4, 5), 40);
    put(&src, r"Doc\cambiato.txt", &bytes(5, 1000), 50);
    // A hashed backup: the photo renamed and with another date is still a copy; a file with
    // the same name, size and date but other content is not.
    let hashed = Sandbox::new();
    put(
        &hashed,
        r"Backup\2020\mare-copia.jpg",
        &bytes(1, 70_000),
        99,
    );
    put(&hashed, r"Backup\cambiato.txt", &bytes(6, 1000), 50);
    // A backup never hashed: name, size and date (1 s apart is the same date, FAT).
    let plain = Sandbox::new();
    put(&plain, "monti.jpg", &bytes(2, 50_000), 20);
    put(&plain, "nota.txt", &bytes(3, 900), 31);

    let cat = TestCatalog::new();
    let a = catalog(&cat, &src, true);
    let b = catalog(&cat, &hashed, true);
    let c = catalog(&cat, &plain, false);
    let source = side(&cat, a, "");
    let dests = [side(&cat, b, ""), side(&cat, c, "")];
    let report = cat
        .store
        .read(|conn| Ok(verify(conn, &source, &dests, &CancelToken::new())))
        .unwrap()
        .unwrap();
    assert_eq!(report.total.files, 5);
    // F13-AC3: how much was checked by content and how much by name, size and date.
    assert_eq!(
        report.by_content,
        Tally {
            files: 1,
            bytes: 70_000
        }
    );
    assert_eq!(
        report.by_metadata,
        Tally {
            files: 2,
            bytes: 50_900
        }
    );
    let missing: Vec<&str> = report.missing.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(missing, [r"Doc\cambiato.txt", r"Doc\vuoto.txt"]);
    assert_eq!(
        report.missing_tally(),
        Tally {
            files: 2,
            bytes: 1005
        }
    );

    // F13-AC2: the destinations need not be connected; only the catalog is read.
    for rel in ["monti.jpg", "nota.txt"] {
        plain.fs.remove_file(&plain.path(rel)).unwrap();
    }
    hashed.fs.remove_dir_all(&hashed.path("Backup")).unwrap();
    let again = cat
        .store
        .read(|conn| Ok(verify(conn, &source, &dests, &CancelToken::new())))
        .unwrap()
        .unwrap();
    assert_eq!(again, report);

    // F13-AC4: missing files by folder, with sizes.
    let view = folder_view(
        report.missing.iter().map(|f| (f.rel.as_str(), f.size, 0)),
        "",
        1,
    );
    assert_eq!(view.folders.len(), 1);
    assert_eq!(view.folders[0].name, "Doc");
    assert_eq!(
        view.folders[0].tallies[0],
        Tally {
            files: 2,
            bytes: 1005
        }
    );

    // A folder inside the source is not a backup of it.
    let inside = side(&cat, a, "Foto");
    let overlap = cat
        .store
        .read(|conn| Ok(verify(conn, &source, &[inside], &CancelToken::new())))
        .unwrap();
    assert!(matches!(overlap, Err(BackupError::Overlap)));
}

#[test]
fn f14_comparison_by_path_and_by_content() {
    let one = Sandbox::new();
    put(&one, r"a\same.txt", &bytes(1, 4000), 10);
    put(&one, r"a\diff.txt", &bytes(2, 4000), 10);
    put(&one, r"a\onlyA.txt", &bytes(3, 10), 10);
    put(&one, r"a\moved.txt", &bytes(4, 3000), 10);
    let two = Sandbox::new();
    put(&two, r"a\same.txt", &bytes(1, 4000), 10);
    put(&two, r"a\diff.txt", &bytes(9, 4000), 10);
    put(&two, r"b\moved-renamed.txt", &bytes(4, 3000), 77);
    put(&two, "onlyB.txt", &bytes(5, 20), 10);
    let cat = TestCatalog::new();
    let a = catalog(&cat, &one, true);
    let b = catalog(&cat, &two, true);
    let (sa, sb) = (side(&cat, a, ""), side(&cat, b, ""));
    let run = |mode| {
        cat.store
            .read(|conn| Ok(compare(conn, &sa, &sb, mode, &CancelToken::new())))
            .unwrap()
            .unwrap()
    };
    let status = |r: &prism_core::backup::CompareReport, rel: &str| {
        r.items
            .iter()
            .find(|i| i.rel == rel)
            .map(|i| (i.status, i.other.clone(), i.evidence))
            .unwrap()
    };

    // F14-AC1, by path.
    let by_path = run(MatchMode::Path);
    assert_eq!(status(&by_path, r"a\same.txt").0, Status::Same);
    assert_eq!(
        status(&by_path, r"a\diff.txt"),
        (Status::Different, None, Some(Evidence::Content))
    );
    assert_eq!(status(&by_path, r"a\moved.txt").0, Status::OnlyA);
    assert_eq!(status(&by_path, r"b\moved-renamed.txt").0, Status::OnlyB);
    assert_eq!(status(&by_path, "onlyB.txt").0, Status::OnlyB);
    assert_eq!(by_path.tally(Status::OnlyA).files, 2);

    // F14-AC2, by content: the moved and renamed file is found.
    let by_content = run(MatchMode::Content);
    assert_eq!(
        status(&by_content, r"a\moved.txt"),
        (
            Status::Same,
            Some(r"b\moved-renamed.txt".to_owned()),
            Some(Evidence::Content)
        )
    );
    assert_eq!(status(&by_content, r"a\diff.txt").0, Status::Different);
    assert_eq!(status(&by_content, r"a\onlyA.txt").0, Status::OnlyA);
    assert_eq!(
        by_content.tally(Status::OnlyB),
        Tally {
            files: 1,
            bytes: 20
        }
    );
    assert_eq!(by_content.tally(Status::Same).files, 2);

    // F14-AC3: a tree with totals by status.
    let items = || {
        by_content
            .items
            .iter()
            .map(|i| (i.rel.as_str(), i.size, i.status as usize))
    };
    let top = folder_view(items(), "", 4);
    assert_eq!(top.files.len(), 1, "onlyB.txt at the top");
    let folder_a = top.folders.iter().find(|f| f.name == "a").unwrap();
    assert_eq!(folder_a.tallies[Status::Same as usize].files, 2);
    assert_eq!(folder_a.tallies[Status::Different as usize].files, 1);
    assert_eq!(folder_view(items(), "a", 4).files.len(), 4);
}

#[test]
fn f14_without_hashes_names_sizes_and_dates_decide() {
    let one = Sandbox::new();
    put(&one, "x.txt", &bytes(1, 100), 10);
    put(&one, "y.txt", &bytes(2, 100), 10);
    let two = Sandbox::new();
    put(&two, "x.txt", &bytes(7, 100), 11);
    put(&two, "y.txt", &bytes(2, 100), 500);
    let cat = TestCatalog::new();
    let a = catalog(&cat, &one, false);
    let b = catalog(&cat, &two, false);
    let r = cat
        .store
        .read(|conn| {
            Ok(compare(
                conn,
                &side(&cat, a, ""),
                &side(&cat, b, ""),
                MatchMode::Path,
                &CancelToken::new(),
            ))
        })
        .unwrap()
        .unwrap();
    // Without hashes a same-size file with a close date counts as identical (and the report
    // says it was decided by metadata); another date makes it different.
    assert_eq!(r.items[0].status, Status::Same);
    assert_eq!(r.items[0].evidence, Some(Evidence::Metadata));
    assert_eq!(r.items[1].status, Status::Different);
}

fn items(list: &[(&str, usize)]) -> Vec<CopyItem> {
    list.iter()
        .map(|(rel, size)| CopyItem {
            rel: (*rel).to_owned(),
            size: *size as u64,
        })
        .collect()
}

#[test]
fn f15_verified_copy_never_overwrites_and_resumes() {
    let src = Sandbox::new();
    put(&src, r"Foto\mare.jpg", &bytes(1, 300_000), 10);
    put(&src, r"Doc\nota.txt", &bytes(3, 900), 30);
    put(&src, r"Doc\Sub\deep.txt", &bytes(4, 10), 40);
    let dst = Sandbox::new();
    put(&dst, r"Copia\Doc\nota.txt", b"the user's own file", 99);
    put(
        &dst,
        r"Copia\Doc\Sub\deep.txt.prism-part",
        b"left by a crash",
        99,
    );
    let dest_root = dst.path("Copia");
    let guard = SandboxGuard::new([dst.root()]).unwrap();
    let list = items(&[
        (r"Foto\mare.jpg", 300_000),
        (r"Doc\nota.txt", 900),
        (r"Doc\Sub\deep.txt", 10),
    ]);
    let run = |conflict| {
        let progress = JobProgress::default();
        let report = copy_files(
            &CopyRequest {
                source_root: src.root(),
                dest_root: &dest_root,
                items: &list,
                conflict,
            },
            Some(&guard),
            &CancelToken::new(),
            &progress,
        );
        (report, progress.total.load(Ordering::Relaxed))
    };

    // F15-AC3/AC4/AC5: structure recreated, the existing file skipped, the copies verified.
    let (first, total) = run(Conflict::Skip);
    assert_eq!(total, 300_910);
    let outcomes: Vec<&str> = first.results.iter().map(|r| r.outcome.as_str()).collect();
    assert_eq!(outcomes, ["copied", "skipped_conflict", "copied"]);
    assert_eq!(
        std::fs::read(dst.path(r"Copia\Foto\mare.jpg")).unwrap(),
        bytes(1, 300_000)
    );
    assert_eq!(
        std::fs::read(dst.path(r"Copia\Doc\nota.txt")).unwrap(),
        b"the user's own file"
    );
    assert!(!dst.path(r"Copia\Doc\Sub\deep.txt.prism-part").exists());
    // The date is kept, so a later check by metadata recognises the copy.
    let modified = |p: std::path::PathBuf| std::fs::metadata(p).unwrap().modified().unwrap();
    assert_eq!(
        modified(dst.path(r"Copia\Foto\mare.jpg")),
        modified(src.path(r"Foto\mare.jpg"))
    );
    assert_eq!(first.copied_bytes(), 300_010);

    // Again, renaming on conflict: what is there already is recognised, the other file kept.
    let (second, _) = run(Conflict::Rename);
    let outcomes: Vec<&str> = second.results.iter().map(|r| r.outcome.as_str()).collect();
    assert_eq!(outcomes, ["already_there", "renamed", "already_there"]);
    assert_eq!(
        second.results[1].outcome,
        CopyOutcome::Renamed(dst.path(r"Copia\Doc\nota (2).txt"))
    );
    assert_eq!(
        std::fs::read(dst.path(r"Copia\Doc\nota (2).txt")).unwrap(),
        bytes(3, 900)
    );
    assert_eq!(
        std::fs::read(dst.path(r"Copia\Doc\nota.txt")).unwrap(),
        b"the user's own file"
    );
}

#[test]
fn f15_cancelled_copy_leaves_nothing_partial_and_resumes() {
    let src = Sandbox::new();
    put(&src, "big.bin", &bytes(8, 48 * 1024 * 1024), 10);
    let dst = Sandbox::new();
    let guard = SandboxGuard::new([dst.root()]).unwrap();
    let list = items(&[("big.bin", 48 * 1024 * 1024)]);
    let cancel = CancelToken::new();
    let progress = Arc::new(JobProgress::default());
    let watcher = {
        let (cancel, progress) = (cancel.clone(), Arc::clone(&progress));
        std::thread::spawn(move || {
            // Cancel as soon as the copy has started (never wait forever if it fails early).
            let start = std::time::Instant::now();
            while progress.done.load(Ordering::Relaxed) == 0
                && start.elapsed() < Duration::from_secs(20)
            {
                std::thread::sleep(Duration::from_micros(200));
            }
            cancel.cancel();
        })
    };
    let report = copy_files(
        &CopyRequest {
            source_root: src.root(),
            dest_root: dst.root(),
            items: &list,
            conflict: Conflict::Skip,
        },
        Some(&guard),
        &cancel,
        &progress,
    );
    watcher.join().unwrap();
    assert!(report.interrupted);
    assert!(report.results.is_empty());
    assert!(!dst.path("big.bin").exists());
    assert!(!dst.path(&format!("big.bin{PART_SUFFIX}")).exists());

    let resumed = copy_files(
        &CopyRequest {
            source_root: src.root(),
            dest_root: dst.root(),
            items: &list,
            conflict: Conflict::Skip,
        },
        Some(&guard),
        &CancelToken::new(),
        &JobProgress::default(),
    );
    assert!(!resumed.interrupted);
    assert_eq!(resumed.results[0].outcome, CopyOutcome::Copied);
    assert_eq!(
        std::fs::metadata(dst.path("big.bin")).unwrap().len(),
        48 * 1024 * 1024
    );
}

#[test]
fn f15_writes_outside_the_sandbox_are_refused() {
    let src = Sandbox::new();
    put(&src, "a.txt", b"abc", 10);
    let dst = Sandbox::new();
    let elsewhere = Sandbox::new();
    let guard = SandboxGuard::new([elsewhere.root()]).unwrap();
    let report = copy_files(
        &CopyRequest {
            source_root: src.root(),
            dest_root: dst.root(),
            items: &items(&[("a.txt", 3)]),
            conflict: Conflict::Skip,
        },
        Some(&guard),
        &CancelToken::new(),
        &JobProgress::default(),
    );
    assert!(matches!(report.results[0].outcome, CopyOutcome::Failed(_)));
    assert!(!dst.path("a.txt").exists());
    assert!(!dst.path(&format!("a.txt{PART_SUFFIX}")).exists());
}
