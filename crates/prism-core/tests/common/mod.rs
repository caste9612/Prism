//! Helpers shared by the integration tests of prism-core.
//!
//! Every file-system change made by these helpers goes through `GuardedFs` (docs/TESTING.md
//! S2): a mistake cannot touch anything outside the temporary folder of the test.
#![allow(dead_code)]

pub mod fakefs;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::indexer::{self, IndexError, IndexProgress, IndexReport, IndexRequest};
use prism_core::model::{VolumeId, VolumeInfo, VolumeStatus};
use prism_core::reconcile::ReconcileTarget;
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::scan::{Enumerator, WalkEnumerator};
use prism_core::store::{self, SnapshotEntry, Store};

/// A catalog in a temporary folder.
pub struct TestCatalog {
    _dir: tempfile::TempDir,
    pub store: Store,
}

impl TestCatalog {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("catalog.db")).unwrap();
        Self { _dir: dir, store }
    }

    pub fn add_volume(&self, mount: &str) -> VolumeId {
        let info = VolumeInfo::dev_folder(mount);
        self.store
            .write(move |w| store::insert_volume(w, &info, VolumeStatus::New))
            .unwrap()
    }

    pub fn status(&self, volume: VolumeId) -> VolumeStatus {
        self.store
            .read(|c| store::volume(c, volume))
            .unwrap()
            .unwrap()
            .status
    }

    pub fn snapshot(&self, volume: VolumeId) -> BTreeMap<String, SnapshotEntry> {
        self.store.read(|c| store::snapshot(c, volume)).unwrap()
    }

    pub fn assert_consistent(&self) {
        let problems = self.store.read(store::check_consistency).unwrap();
        assert!(
            problems.is_empty(),
            "catalog inconsistencies: {problems:#?}"
        );
    }

    /// Indexes (or updates) the whole volume rooted at `path` with `enumerator`.
    pub fn index_with(
        &self,
        volume: VolumeId,
        path: &Path,
        enumerator: &dyn Enumerator,
        policy: &ExclusionPolicy,
        cancel: &CancelToken,
    ) -> Result<IndexReport, IndexError> {
        indexer::index(
            &self.store,
            enumerator,
            &IndexRequest {
                volume,
                path,
                relative: &[],
                target: ReconcileTarget::VolumeRoot,
                policy,
            },
            cancel,
            &IndexProgress::default(),
        )
    }

    /// Indexes the folder `path` with the parallel walker and the default exclusions.
    pub fn index(&self, volume: VolumeId, path: &Path) -> IndexReport {
        self.index_with(
            volume,
            path,
            &WalkEnumerator::default(),
            &ExclusionPolicy::with_defaults(),
            &CancelToken::new(),
        )
        .unwrap()
    }
}

/// A temporary folder that tests modify through the sandbox guard.
pub struct Sandbox {
    dir: tempfile::TempDir,
    pub fs: GuardedFs,
}

impl Sandbox {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let fs = GuardedFs::new(SandboxGuard::new([dir.path()]).unwrap());
        Self { dir, fs }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        let mut p = self.dir.path().to_path_buf();
        for part in rel.split(['\\', '/']).filter(|p| !p.is_empty()) {
            p.push(part);
        }
        p
    }

    pub fn mkdir(&self, rel: &str) {
        self.fs.create_dir_all(&self.path(rel)).unwrap();
    }

    /// Writes a file of `size` bytes with a modification time `secs` after 2020-01-01.
    pub fn file(&self, rel: &str, size: usize, secs: u64) {
        let p = self.path(rel);
        if let Some(parent) = p.parent().filter(|d| *d != self.root()) {
            self.fs.create_dir_all(parent).unwrap();
        }
        self.fs.write(&p, &vec![b'x'; size]).unwrap();
        self.fs.set_modified(&p, mtime(secs)).unwrap();
    }
}

pub fn mtime(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_577_836_800 + secs)
}

/// Creates a directory junction at `link` pointing to `target` (no administrator needed).
#[cfg(windows)]
pub fn junction(link: &Path, target: &Path) {
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(out.status.success(), "mklink /J failed: {out:?}");
}
