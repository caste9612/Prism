//! F05-AC3 on real folders: after random changes on disk, updating the catalog gives the same
//! result as indexing from scratch. All changes go through the sandbox guard.
// Test code: unwrap/expect are allowed (CLAUDE.md), also in helpers outside #[test].
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};

use common::fakefs::{NAMES, Rng};
use common::{Sandbox, TestCatalog, mtime};
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
}

impl Disk<'_> {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn write(&mut self, path: &Path, size: usize) {
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
                        self.sb.fs.create_dir_all(&p).unwrap();
                    } else {
                        let size = rng.below(3000);
                        self.write(&p, size);
                    }
                }
            }
            2 => {
                if let Some(p) = rng.pick(&all) {
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
                        self.sb.fs.rename(p, &target).unwrap();
                    }
                }
            }
            6 => {
                // Move to another folder (never into its own subtree).
                if let (Some(p), Some(to)) = (rng.pick(&all), rng.pick(&dirs)) {
                    let target = to.join(p.file_name().unwrap());
                    if !to.starts_with(p) && !target.exists() {
                        self.sb.fs.rename(p, &target).unwrap();
                    }
                }
            }
            _ => {
                // Replace a file with a folder of the same name.
                if let Some(p) = rng.pick(&files) {
                    let p = p.clone();
                    self.sb.fs.remove_file(&p).unwrap();
                    self.sb.fs.create_dir_all(&p).unwrap();
                }
            }
        }
    }
}

fn check(seed: u64, steps: usize) {
    let mut rng = Rng::new(seed);
    let sb = Sandbox::new();
    let mut disk = Disk { sb: &sb, clock: 0 };
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn f05_ac3_update_equals_full_reindex_on_disk(seed in any::<u64>(), steps in 1usize..20) {
        check(seed, steps);
    }
}
