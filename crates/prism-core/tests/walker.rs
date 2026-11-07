//! The parallel walker on real temporary folders, end to end through the catalog.
// Test code: unwrap/expect are allowed (CLAUDE.md), also in helpers outside #[test].
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

use common::{Sandbox, TestCatalog, mtime};
use prism_core::cancel::CancelToken;
use prism_core::exclusion::{ExclusionPolicy, ExclusionRule};
use prism_core::model::{ATTR_DIRECTORY, VolumeStatus};
use prism_core::names;
use prism_core::scan::{Enumerator, ScanError, ScanRequest, ScanTree, WalkEnumerator};
use prism_core::store::{self, SnapshotEntry};

fn scan(root: &Path, policy: &ExclusionPolicy, threads: usize) -> Result<ScanTree, ScanError> {
    WalkEnumerator::with_threads(threads).scan(&ScanRequest {
        root,
        relative: &[],
        policy,
        cancel: &CancelToken::new(),
        progress: &AtomicU64::new(0),
    })
}

fn sample_tree(sb: &Sandbox) {
    sb.file(r"docs\report.pdf", 1200, 10);
    sb.file(r"docs\notes.txt", 30, 20);
    sb.file(r"docs\old\2019.txt", 7, 30);
    sb.file(r"photos\a.jpg", 5000, 40);
    sb.file("empty.bin", 0, 50);
    sb.mkdir(r"empty dir");
}

// F04-AC3: name, parent, kind, size, dates and attributes are stored as on disk.
#[cfg(windows)]
#[test]
fn f04_ac3_metadata_matches_the_file_system() {
    use std::os::windows::fs::MetadataExt;
    let sb = Sandbox::new();
    sample_tree(&sb);
    let cat = TestCatalog::new();
    let v = cat.add_volume(&sb.root().display().to_string());
    cat.index(v, sb.root());
    cat.assert_consistent();
    let snap = cat.snapshot(v);
    assert_eq!(snap.len(), 10, "{:#?}", snap.keys().collect::<Vec<_>>());
    for (rel, e) in &snap {
        let md = std::fs::metadata(sb.path(rel)).unwrap();
        assert_eq!(e.is_dir, md.is_dir(), "{rel}");
        if !e.is_dir {
            assert_eq!(e.meta.size, md.len(), "{rel}");
        }
        assert_eq!(e.meta.attrs, md.file_attributes(), "{rel}");
        assert_eq!(
            e.meta.modified.map(|t| t.0 as u64),
            Some(md.last_write_time()),
            "{rel}"
        );
        assert_eq!(
            e.meta.created.map(|t| t.0 as u64),
            Some(md.creation_time()),
            "{rel}"
        );
        assert_eq!(e.meta.alloc_size, None);
        assert_eq!(e.meta.file_ref, None);
    }
    assert_eq!(
        snap[r"docs\old\2019.txt"]
            .meta
            .modified
            .unwrap()
            .to_unix_millis(),
        mtime(30)
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    );
}

// F04-AC4: folder sizes and counts are available right after indexing.
#[test]
fn f04_ac4_folder_aggregates_after_indexing() {
    let sb = Sandbox::new();
    sample_tree(&sb);
    let cat = TestCatalog::new();
    let v = cat.add_volume("sandbox");
    cat.index(v, sb.root());
    let snap = cat.snapshot(v);
    let root = snap[""].stats.unwrap();
    assert_eq!(root.total_size, 1200 + 30 + 7 + 5000);
    assert_eq!(root.file_count, 5);
    assert_eq!(root.dir_count, 4);
    let docs = snap["docs"].stats.unwrap();
    assert_eq!(docs.total_size, 1237);
    assert_eq!((docs.file_count, docs.dir_count), (3, 1));
    assert_eq!(snap["empty dir"].stats.unwrap().file_count, 0);
    assert_eq!(cat.status(v), VolumeStatus::Indexed);
}

// F04-AC5: default exclusions apply at the volume root only, user rules by exact name.
#[test]
fn f04_ac5_exclusions_are_applied_by_the_walker() {
    let sb = Sandbox::new();
    sb.file(r"$RECYCLE.BIN\S-1-5\deleted.txt", 10, 1);
    sb.file(r"System Volume Information\tracking.log", 10, 1);
    sb.file(r"Users\me\$Recycle.Bin\kept.txt", 10, 1);
    sb.file(r"Users\me\node_modules\pkg.js", 10, 1);
    sb.file(r"Users\me\Templates\t.dotx", 10, 1);
    sb.file(r"Users\me\Temp\x.tmp", 10, 1);

    let defaults = scan(sb.root(), &ExclusionPolicy::with_defaults(), 4).unwrap();
    assert_eq!(defaults.stats.excluded, 2);
    let names: Vec<&str> = defaults
        .children(ScanTree::ROOT)
        .map(|n| &*defaults.node(n).name)
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names.contains(&"Users"));

    let mut rules = ExclusionPolicy::default_rules();
    rules.push(ExclusionRule::DirName("Temp".into()));
    let custom = ExclusionPolicy::new(rules);
    let cat = TestCatalog::new();
    let v = cat.add_volume("sandbox");
    cat.index_with(
        v,
        sb.root(),
        &WalkEnumerator::default(),
        &custom,
        &CancelToken::new(),
    )
    .unwrap();
    let snap = cat.snapshot(v);
    assert!(snap.contains_key(r"Users\me\$Recycle.Bin\kept.txt"));
    assert!(snap.contains_key(r"Users\me\node_modules\pkg.js"));
    assert!(snap.contains_key(r"Users\me\Templates\t.dotx"));
    assert!(!snap.contains_key(r"Users\me\Temp"));
    assert!(!snap.keys().any(|k| k.starts_with("$RECYCLE.BIN")));
}

// Junctions are catalog entries but are not followed (no double counting, no loops).
#[cfg(windows)]
#[test]
fn junctions_are_recorded_but_not_followed() {
    let sb = Sandbox::new();
    sb.file(r"real\big.bin", 4096, 1);
    common::junction(&sb.path("link"), &sb.path("real"));
    common::junction(&sb.path(r"real\loop"), sb.root());
    let cat = TestCatalog::new();
    let v = cat.add_volume("sandbox");
    cat.index(v, sb.root());
    cat.assert_consistent();
    let snap = cat.snapshot(v);
    assert!(snap["link"].is_dir);
    assert_ne!(
        snap["link"].meta.attrs & 0x400,
        0,
        "reparse point attribute"
    );
    assert_eq!(snap["link"].stats.unwrap().file_count, 0);
    assert!(!snap.contains_key(r"link\big.bin"));
    assert_eq!(snap[""].stats.unwrap().total_size, 4096);
}

// Paths longer than MAX_PATH are enumerated.
#[test]
fn long_paths_are_indexed() {
    let sb = Sandbox::new();
    let segment = "a".repeat(60);
    let rel: Vec<String> = (0..6).map(|i| format!("{segment}{i}")).collect();
    let rel_dir = rel.join("\\");
    let file = format!(r"{rel_dir}\deep.txt");
    assert!(sb.path(&file).as_os_str().len() > 300);
    sb.file(&file, 3, 1);
    let cat = TestCatalog::new();
    let v = cat.add_volume("sandbox");
    cat.index(v, sb.root());
    let snap = cat.snapshot(v);
    assert_eq!(snap[&file].meta.size, 3);
}

// Unusual names (Unicode, unpaired surrogates) round-trip: the catalog path leads back to
// the same file on disk.
#[cfg(windows)]
#[test]
fn unusual_names_roundtrip_to_the_real_file() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let sb = Sandbox::new();
    sb.file("日本語 ファイル.txt", 1, 1);
    sb.file("emoji 😀.png", 2, 1);
    let lone = OsString::from_wide(&[0x0062, 0xD83D, 0x0063]); // "b", lone high surrogate, "c"
    let lone_path = sb.root().join(&lone);
    sb.fs.write(&lone_path, b"xyz").unwrap();

    let cat = TestCatalog::new();
    let v = cat.add_volume("sandbox");
    cat.index(v, sb.root());
    cat.assert_consistent();
    let snap = cat.snapshot(v);
    let encoded = names::encode(&lone);
    assert_eq!(snap[&encoded].meta.size, 3);

    let id = cat
        .store
        .read(|c| store::resolve_relative_path(c, v, &encoded))
        .unwrap()
        .unwrap()
        .id;
    let rel = cat.store.read(|c| store::relative_path(c, id)).unwrap();
    let real: PathBuf = rel.split('\\').fold(sb.root().to_path_buf(), |p, part| {
        p.join(names::decode(part))
    });
    assert_eq!(std::fs::read(real).unwrap(), b"xyz");
}

// Names ending with a dot or a space are valid on NTFS (created by WSL or macOS) but Win32
// path normalization strips them: the walker must still enumerate those folders.
#[cfg(windows)]
#[test]
fn names_with_trailing_dot_or_space_are_enumerated() {
    let sb = Sandbox::new();
    let verbatim_root = std::fs::canonicalize(sb.root()).unwrap();
    for dir in ["trap.", "space "] {
        let d = verbatim_root.join(dir);
        sb.fs.create_dir_all(&d).unwrap();
        sb.fs.write(&d.join("inside.txt"), b"1234").unwrap();
    }
    let tree = scan(sb.root(), &ExclusionPolicy::with_defaults(), 2).unwrap();
    assert_eq!(tree.stats.unreadable_dirs, 0, "{:?}", tree.stats.errors);
    let cat = TestCatalog::new();
    let v = cat.add_volume("sandbox");
    cat.index(v, sb.root());
    let snap = cat.snapshot(v);
    assert_eq!(snap[r"trap.\inside.txt"].meta.size, 4);
    assert_eq!(snap[r"space \inside.txt"].meta.size, 4);
}

#[test]
fn a_cancelled_walk_returns_cancelled() {
    let sb = Sandbox::new();
    sample_tree(&sb);
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = WalkEnumerator::default().scan(&ScanRequest {
        root: sb.root(),
        relative: &[],
        policy: &ExclusionPolicy::with_defaults(),
        cancel: &cancel,
        progress: &AtomicU64::new(0),
    });
    assert!(matches!(result, Err(ScanError::Cancelled(_))));
}

#[test]
fn root_errors_are_reported() {
    let sb = Sandbox::new();
    sb.file("f.txt", 1, 1);
    let policy = ExclusionPolicy::with_defaults();
    assert!(matches!(
        scan(&sb.path("missing"), &policy, 1),
        Err(ScanError::RootUnreadable { .. })
    ));
    assert!(matches!(
        scan(&sb.path("f.txt"), &policy, 1),
        Err(ScanError::RootNotADirectory(_))
    ));
}

// The number of threads changes the speed, never the result.
#[test]
fn thread_count_does_not_change_the_catalog() {
    let sb = Sandbox::new();
    for d in 0..8 {
        for f in 0..20 {
            sb.file(&format!(r"d{d}\sub{}\f{f}.txt", f % 3), f, f as u64);
        }
    }
    let snapshot_with = |threads: usize| -> BTreeMap<String, SnapshotEntry> {
        let cat = TestCatalog::new();
        let v = cat.add_volume("sandbox");
        cat.index_with(
            v,
            sb.root(),
            &WalkEnumerator::with_threads(threads),
            &ExclusionPolicy::with_defaults(),
            &CancelToken::new(),
        )
        .unwrap();
        cat.assert_consistent();
        cat.snapshot(v)
    };
    let one = snapshot_with(1);
    assert_eq!(one.len(), 1 + 8 + 24 + 160);
    assert_eq!(one, snapshot_with(8));
    assert!(
        one.values()
            .filter(|e| e.is_dir)
            .all(|e| e.meta.attrs & ATTR_DIRECTORY != 0)
    );
}
