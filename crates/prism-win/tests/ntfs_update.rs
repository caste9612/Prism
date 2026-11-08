//! F05-AC1 / F05-AC3 on a real NTFS volume: after random changes on the PRISMTEST test volume,
//! updating the catalog from the change journal gives the same catalog as indexing the volume
//! from scratch. Needs administrator rights; run by the admin runner (docs/TESTING.md §4).
//! Changes are made only in a folder of the test volume, through the sandbox guard.
#![cfg(all(windows, feature = "admin-tests"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::indexer::IndexProgress;
use prism_core::model::{BusType, VolumeId, VolumeInfo, VolumeStatus};
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::store::{self, Store};
use prism_core::volumes::VolumeSource;
use prism_win::ntfs::{UpdateOutcome, index_ntfs, update_ntfs};
use prism_win::usn::query_journal;
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

fn letter(volume: &Path) -> char {
    volume.to_string_lossy().chars().next().unwrap()
}

/// Creates the journal on the TEST volume only, when it has none.
fn ensure_journal(volume: &Path) {
    let handle = File::open(format!(r"\\.\{}:", letter(volume))).unwrap();
    if query_journal(&handle).unwrap().is_none() {
        let out = std::process::Command::new("fsutil")
            .args(["usn", "createjournal", "m=33554432", "a=4194304"])
            .arg(format!("{}:", letter(volume)))
            .output()
            .unwrap();
        assert!(out.status.success(), "fsutil createjournal: {out:?}");
    }
}

/// Writes the cached metadata of the test volume to disk (the full MFT read reads the disk).
fn flush(volume: &Path) {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(r"\\.\{}:", letter(volume)))
        .unwrap()
        .sync_all()
        .unwrap();
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

const NAMES: &[&str] = &["a", "b", "Photos", "doc.txt", "img.jpg", "été", "日本", "z"];

/// A folder of the test volume where the random changes happen, removed at the end.
struct Playground {
    root: PathBuf,
    fs: GuardedFs,
}

impl Playground {
    fn new(volume: &Path) -> Self {
        let fs = GuardedFs::new(SandboxGuard::new([volume]).unwrap());
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::fs::canonicalize(volume)
            .unwrap()
            .join(format!("prism-update-{}-{nanos}", std::process::id()));
        fs.create_dir_all(&root).unwrap();
        Self { root, fs }
    }

    fn all(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut dirs = vec![self.root.clone()];
        let mut files = Vec::new();
        let mut i = 0;
        while i < dirs.len() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&dirs[i])
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

    fn mutate(&self, rng: &mut Rng, steps: usize) {
        for _ in 0..steps {
            let (dirs, files) = self.all();
            let entries: Vec<PathBuf> = dirs[1..].iter().chain(&files).cloned().collect();
            let name = NAMES[rng.below(NAMES.len())];
            let dir = &dirs[rng.below(dirs.len())];
            match rng.below(8) {
                0 | 1 => {
                    let p = dir.join(name);
                    if !p.exists() {
                        self.fs.write(&p, &vec![b'u'; rng.below(5000)]).unwrap();
                    }
                }
                2 => {
                    let p = dir.join(format!("{name}-dir"));
                    if !p.exists() {
                        self.fs.create_dir_all(&p).unwrap();
                    }
                }
                3 => {
                    if let Some(p) = entries.get(rng.below(entries.len().max(1))) {
                        if p.is_dir() {
                            self.fs.remove_dir_all(p).unwrap();
                        } else {
                            self.fs.remove_file(p).unwrap();
                        }
                    }
                }
                4 => {
                    if let Some(p) = files.get(rng.below(files.len().max(1))) {
                        self.fs.write(p, &vec![b'm'; rng.below(9000)]).unwrap();
                    }
                }
                5 => {
                    if let Some(p) = entries.get(rng.below(entries.len().max(1))) {
                        let target = p.with_file_name(format!("{name}-{}", rng.below(99)));
                        if !target.exists() {
                            self.fs.rename(p, &target).unwrap();
                        }
                    }
                }
                6 => {
                    if let Some(p) = entries.get(rng.below(entries.len().max(1))) {
                        let target = dir.join(p.file_name().unwrap());
                        if !dir.starts_with(p) && !target.exists() {
                            self.fs.rename(p, &target).unwrap();
                        }
                    }
                }
                _ => {
                    if let Some(p) = files.get(rng.below(files.len().max(1))) {
                        let target = dir.join(format!("{name}-link{}", rng.below(99)));
                        // The guard checks the new name like any other write.
                        let checked = SandboxGuard::new([&self.root])
                            .unwrap()
                            .check_no_follow(&target)
                            .unwrap();
                        if !target.exists() {
                            std::fs::hard_link(p, checked).unwrap();
                        }
                    }
                }
            }
        }
    }
}

impl Drop for Playground {
    fn drop(&mut self) {
        let _ = self.fs.remove_dir_all(&self.root);
    }
}

fn catalog() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("catalog.db")).unwrap();
    (dir, store)
}

fn add_volume(store: &Store, mount: &str) -> VolumeId {
    let info = VolumeInfo::dev_folder(mount);
    store
        .write(move |w| store::insert_volume(w, &info, VolumeStatus::New))
        .unwrap()
}

#[test]
fn f05_ac3_journal_update_equals_full_reindex_on_the_test_volume() {
    let volume = test_volume();
    ensure_journal(&volume);
    let policy = ExclusionPolicy::with_defaults();
    let (_d, store) = catalog();
    let v = add_volume(&store, &volume.to_string_lossy());
    index_ntfs(
        &store,
        v,
        &volume,
        &policy,
        &CancelToken::new(),
        &IndexProgress::default(),
    )
    .unwrap();

    let playground = Playground::new(&volume);
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let mut from_journal = 0;
    for round in 0..4 {
        playground.mutate(&mut rng, 20);
        // NTFS writes folder timestamps into the MFT records lazily: write everything first,
        // so that the update and the fresh index below read the same disk state.
        flush(&volume);
        let outcome = update_ntfs(
            &store,
            v,
            &volume,
            &policy,
            &CancelToken::new(),
            &IndexProgress::default(),
        )
        .unwrap();
        if let UpdateOutcome::Journal { records, .. } = outcome {
            assert!(records > 0, "round {round}: the journal saw the changes");
            from_journal += 1;
        }
        let problems = store.read(store::check_consistency).unwrap();
        assert!(problems.is_empty(), "round {round}: {problems:?}");

        let (_f, fresh) = catalog();
        let fv = add_volume(&fresh, &volume.to_string_lossy());
        index_ntfs(
            &fresh,
            fv,
            &volume,
            &policy,
            &CancelToken::new(),
            &IndexProgress::default(),
        )
        .unwrap();
        let got = store.read(|c| store::snapshot(c, v)).unwrap();
        let want = fresh.read(|c| store::snapshot(c, fv)).unwrap();
        let mut diffs = Vec::new();
        for (k, w) in &want {
            match got.get(k) {
                None => diffs.push(format!("missing {k}")),
                Some(g) if g != w => diffs.push(format!("differs {k}: {g:?} / {w:?}")),
                Some(_) => {}
            }
        }
        diffs.extend(
            got.keys()
                .filter(|k| !want.contains_key(*k))
                .map(|k| format!("extra {k}")),
        );
        assert!(diffs.is_empty(), "round {round}:\n{}", diffs.join("\n"));
    }
    assert!(from_journal >= 3, "updates should come from the journal");
}
