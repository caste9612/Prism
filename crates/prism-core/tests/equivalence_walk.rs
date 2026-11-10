//! F05-AC3 on real folders: after random changes on disk, updating the catalog gives the same
//! result as indexing from scratch. All changes go through the sandbox guard.
// Test code: unwrap/expect are allowed (CLAUDE.md), also in helpers outside #[test].
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::fakefs::{NAMES, Rng};
use common::{Sandbox, TestCatalog, mtime};
use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::indexer::IndexProgress;
use prism_core::names;
use prism_core::refresh;
use prism_core::scan::{Enumerator, ScanError, ScanRequest, ScanTree, WalkEnumerator};
use proptest::prelude::*;

/// Lists folders (root included) and files below `root`, read-only.
fn list_all(root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut dirs = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut i = 0;
    while i < dirs.len() {
        let mut entries: Vec<_> = std::fs::read_dir(&dirs[i])
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                dirs.push(p);
            } else {
                files.push(p);
            }
        }
        i += 1;
    }
    (dirs, files)
}

struct Disk<'a> {
    sb: &'a Sandbox,
    clock: u64,
    /// Paths touched since the last `take_changed`, as a folder watcher reports them.
    changed: Vec<PathBuf>,
}

impl Disk<'_> {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn take_changed(&mut self) -> Vec<Vec<String>> {
        let root = self.sb.root().to_path_buf();
        std::mem::take(&mut self.changed)
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .components()
                    .map(|c| names::encode(c.as_os_str()))
                    .collect()
            })
            .collect()
    }

    fn write(&mut self, path: &Path, size: usize) {
        self.changed.push(path.to_path_buf());
        let t = self.tick();
        self.sb.fs.write(path, &vec![b'p'; size]).unwrap();
        self.sb.fs.set_modified(path, mtime(t)).unwrap();
    }

    /// One random change. Names that already exist (Windows compares without case) are
    /// skipped, as a user would get an error.
    fn mutate(&mut self, rng: &mut Rng) {
        let root = self.sb.root().to_path_buf();
        let (dirs, files) = list_all(&root);
        let all: Vec<PathBuf> = dirs[1..].iter().chain(files.iter()).cloned().collect();
        let name = *rng.pick(NAMES).unwrap();
        match rng.below(8) {
            0 | 1 => {
                let parent = rng.pick(&dirs).unwrap().clone();
                let p = parent.join(name);
                if !p.exists() {
                    if rng.below(3) == 0 {
                        self.changed.push(p.clone());
                        self.sb.fs.create_dir_all(&p).unwrap();
                    } else {
                        let size = rng.below(3000);
                        self.write(&p, size);
                    }
                }
            }
            2 => {
                if let Some(p) = rng.pick(&all) {
                    self.changed.push(p.clone());
                    if p.is_dir() {
                        self.sb.fs.remove_dir_all(p).unwrap();
                    } else {
                        self.sb.fs.remove_file(p).unwrap();
                    }
                }
            }
            3 | 4 => {
                if let Some(p) = rng.pick(&files) {
                    let size = if rng.below(2) == 0 {
                        std::fs::metadata(p).unwrap().len() as usize
                    } else {
                        rng.below(3000)
                    };
                    let p = p.clone();
                    self.write(&p, size);
                }
            }
            5 => {
                // Rename in place, sometimes changing only the case.
                if let Some(p) = rng.pick(&all) {
                    let old = p.file_name().unwrap().to_string_lossy().into_owned();
                    let new = if rng.below(3) == 0 {
                        old.to_uppercase()
                    } else {
                        name.to_owned()
                    };
                    let target = p.with_file_name(&new);
                    let same_entry = new.to_lowercase() == old.to_lowercase();
                    if new != old && (same_entry || !target.exists()) {
                        self.changed.push(p.clone());
                        self.changed.push(target.clone());
                        self.sb.fs.rename(p, &target).unwrap();
                    }
                }
            }
            6 => {
                // Move to another folder (never into its own subtree).
                if let (Some(p), Some(to)) = (rng.pick(&all), rng.pick(&dirs)) {
                    let target = to.join(p.file_name().unwrap());
                    if !to.starts_with(p) && !target.exists() {
                        self.changed.push(p.clone());
                        self.changed.push(target.clone());
                        self.sb.fs.rename(p, &target).unwrap();
                    }
                }
            }
            _ => {
                // Replace a file with a folder of the same name.
                if let Some(p) = rng.pick(&files) {
                    let p = p.clone();
                    self.changed.push(p.clone());
                    self.sb.fs.remove_file(&p).unwrap();
                    self.sb.fs.create_dir_all(&p).unwrap();
                }
            }
        }
    }
}

/// A sandbox with a random tree, and the generator to continue from.
fn setup(seed: u64) -> (Rng, Sandbox) {
    let mut rng = Rng::new(seed);
    let sb = Sandbox::new();
    let mut disk = Disk {
        sb: &sb,
        clock: 0,
        changed: Vec::new(),
    };
    for _ in 0..30 {
        let (dirs, _) = list_all(sb.root());
        let parent = rng.pick(&dirs).unwrap().clone();
        let p = parent.join(rng.pick(NAMES).unwrap());
        if p.exists() {
            continue;
        }
        if rng.below(3) == 0 {
            sb.fs.create_dir_all(&p).unwrap();
        } else {
            let size = rng.below(3000);
            disk.write(&p, size);
        }
    }
    (rng, sb)
}

// F05-AC3 for volumes without a change journal: refreshing only the folders reported by the
// folder watcher gives the same catalog as a full indexing.
fn check_refresh(seed: u64, steps: usize) {
    let (mut rng, sb) = setup(seed);
    let cat = TestCatalog::new();
    let v = cat.add_volume("sandbox");
    cat.index(v, sb.root());
    let mut disk = Disk {
        sb: &sb,
        clock: 10_000,
        changed: Vec::new(),
    };
    for _ in 0..steps {
        disk.mutate(&mut rng);
    }
    let changed = disk.take_changed();
    refresh::refresh_folders(
        &cat.store,
        v,
        sb.root(),
        &WalkEnumerator::with_threads(2),
        &ExclusionPolicy::with_defaults(),
        &changed,
        false,
        &CancelToken::new(),
        &IndexProgress::default(),
    )
    .unwrap();
    cat.assert_consistent();
    let fresh = TestCatalog::new();
    let fv = fresh.add_volume("sandbox");
    fresh.index(fv, sb.root());
    assert_eq!(cat.snapshot(v), fresh.snapshot(fv));
}

fn check(seed: u64, steps: usize) {
    let mut rng = Rng::new(seed);
    let sb = Sandbox::new();
    let mut disk = Disk {
        sb: &sb,
        clock: 0,
        changed: Vec::new(),
    };
    for _ in 0..30 {
        let (dirs, _) = list_all(sb.root());
        let parent = rng.pick(&dirs).unwrap().clone();
        let p = parent.join(rng.pick(NAMES).unwrap());
        if p.exists() {
            continue;
        }
        if rng.below(3) == 0 {
            sb.fs.create_dir_all(&p).unwrap();
        } else {
            let size = rng.below(3000);
            disk.write(&p, size);
        }
    }

    let cat = TestCatalog::new();
    let v = cat.add_volume("sandbox");
    cat.index(v, sb.root());
    for _ in 0..steps {
        disk.mutate(&mut rng);
    }
    cat.index(v, sb.root());
    cat.assert_consistent();

    let fresh = TestCatalog::new();
    let fv = fresh.add_volume("sandbox");
    fresh.index(fv, sb.root());
    fresh.assert_consistent();
    assert_eq!(cat.snapshot(v), fresh.snapshot(fv));
}

/// The walker, counting the folders it reads in full and the listings it reads alone.
#[derive(Default)]
struct Counting {
    inner: WalkEnumerator,
    scans: AtomicUsize,
    lists: AtomicUsize,
}

impl Counting {
    fn take(&self) -> (usize, usize) {
        (
            self.scans.swap(0, Ordering::SeqCst),
            self.lists.swap(0, Ordering::SeqCst),
        )
    }
}

impl Enumerator for Counting {
    fn scan(&self, request: &ScanRequest<'_>) -> Result<ScanTree, ScanError> {
        self.scans.fetch_add(1, Ordering::SeqCst);
        self.inner.scan(request)
    }

    fn list(&self, request: &ScanRequest<'_>) -> Result<ScanTree, ScanError> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.inner.list(request)
    }
}

// A change directly in a large folder reads the listing of that folder, not its subtree (seen
// on a company file server: each such change re-read a share of 3.2 million entries); a new
// folder is read in full. The catalog still equals a full indexing (F05-AC3).
#[test]
fn a_refresh_reads_the_listing_of_the_changed_folder_only() {
    let sb = Sandbox::new();
    for d in ["big\\a\\x", "big\\a\\y", "big\\b", "other"] {
        sb.mkdir(d);
    }
    for (i, f) in [
        "big\\a\\x\\1.txt",
        "big\\a\\y\\2.txt",
        "big\\b\\3.txt",
        "top.txt",
    ]
    .iter()
    .enumerate()
    {
        sb.file(f, 10 + i, 100);
    }
    let cat = TestCatalog::new();
    let v = cat.add_volume("sandbox");
    cat.index(v, sb.root());
    let walker = Counting::default();
    let refresh = |changed: &[&str]| {
        let changed: Vec<Vec<String>> = changed
            .iter()
            .map(|p| p.split('\\').map(str::to_owned).collect())
            .collect();
        refresh::refresh_folders(
            &cat.store,
            v,
            sb.root(),
            &walker,
            &ExclusionPolicy::with_defaults(),
            &changed,
            false,
            &CancelToken::new(),
            &IndexProgress::default(),
        )
        .unwrap()
    };
    let same_as_full_index = || {
        cat.assert_consistent();
        let fresh = TestCatalog::new();
        let fv = fresh.add_volume("sandbox");
        fresh.index(fv, sb.root());
        assert_eq!(cat.snapshot(v), fresh.snapshot(fv));
    };

    // A file written in "big": one listing, nothing read in full.
    sb.file("big\\new.txt", 7, 200);
    let report = refresh(&["big\\new.txt"]);
    assert_eq!(walker.take(), (0, 1));
    assert_eq!(
        report.scan.files + report.scan.dirs,
        4,
        "big and its 3 children"
    );
    same_as_full_index();

    // A folder created in "big" with content: the listing of "big", then the new folder in
    // full; "big" itself reported as changed (its date) adds the listing of the root.
    sb.mkdir("big\\new\\deep");
    sb.file("big\\new\\deep\\4.txt", 4, 300);
    refresh(&["big\\new", "big"]);
    assert_eq!(walker.take(), (1, 2));
    same_as_full_index();

    // A folder renamed: the listing of its parent, and the new name read in full.
    sb.fs
        .rename(&sb.path("big\\a"), &sb.path("big\\renamed"))
        .unwrap();
    refresh(&["big\\a", "big\\renamed"]);
    assert_eq!(walker.take(), (1, 1));
    same_as_full_index();

    // A folder replaced by another of the same name, reported level by level.
    sb.fs.remove_dir_all(&sb.path("big\\renamed\\x")).unwrap();
    sb.mkdir("big\\renamed\\x");
    sb.file("big\\renamed\\x\\5.txt", 5, 400);
    refresh(&["big\\renamed\\x", "big\\renamed\\x\\5.txt"]);
    assert_eq!(walker.take(), (0, 2));
    same_as_full_index();

    // A junction is recorded and never followed, also when it is reported again once in the
    // catalog: its parent's listing is read instead.
    #[cfg(windows)]
    {
        common::junction(&sb.path("big\\link"), &sb.path("big\\b"));
        for _ in 0..2 {
            refresh(&["big\\link"]);
            assert_eq!(walker.take(), (0, 1));
            same_as_full_index();
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn f05_ac3_update_equals_full_reindex_on_disk(seed in any::<u64>(), steps in 1usize..20) {
        check(seed, steps);
    }

    #[test]
    fn f05_ac3_watcher_refresh_equals_full_reindex_on_disk(
        seed in any::<u64>(),
        steps in 1usize..15,
    ) {
        check_refresh(seed, steps);
    }
}
