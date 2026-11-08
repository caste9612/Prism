//! The MFT reader on the PRISMTEST test volume, compared with the walker on the same tree.
//! Needs administrator rights; run by the admin runner (docs/TESTING.md §4). Files are
//! created and removed only in a folder of the test volume, through the sandbox guard.
#![cfg(all(windows, feature = "admin-tests"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::time::{Duration, SystemTime};

use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::model::{BusType, EntryMeta};
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::scan::{Enumerator, ScanRequest, ScanTree, WalkEnumerator};
use prism_core::volumes::VolumeSource;
use prism_win::mft::MftEnumerator;
use prism_win::volumes::WinVolumeSource;

/// Root of the test volume, checked to be PRISMTEST on a virtual disk before any write.
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

/// A folder on the test volume, removed at the end.
struct TestDir {
    path: PathBuf,
    name: String,
    fs: GuardedFs,
}

impl TestDir {
    fn new(volume: &Path) -> Self {
        let fs = GuardedFs::new(SandboxGuard::new([volume]).unwrap());
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!("prism-mft-{}-{nanos}", std::process::id());
        let path = std::fs::canonicalize(volume).unwrap().join(&name);
        fs.create_dir_all(&path).unwrap();
        Self { path, name, fs }
    }

    fn file(&self, rel: &str, size: usize, secs: u64) -> PathBuf {
        let p = self.path.join(rel);
        if let Some(parent) = p.parent() {
            self.fs.create_dir_all(parent).unwrap();
        }
        self.fs.write(&p, &vec![b'm'; size]).unwrap();
        self.fs
            .set_modified(
                &p,
                SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000 + secs),
            )
            .unwrap();
        p
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = self.fs.remove_dir_all(&self.path);
    }
}

/// Runs a Windows tool that changes `file` (compression, sparse ranges), after the sandbox
/// guard of the test volume accepted the path.
fn file_tool(volume: &Path, program: &str, before: &[&str], file: &Path, after: &[&str]) {
    let checked = SandboxGuard::new([volume]).unwrap().check(file).unwrap();
    // The tools do not take `\\?\` paths.
    let text = checked.to_string_lossy();
    let plain = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let status = std::process::Command::new(program)
        .args(before)
        .arg(plain)
        .args(after)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "{program} {before:?} {after:?} failed");
}

/// Writes cached metadata of the test volume to disk, so that the raw MFT read sees it.
fn flush_volume(volume: &Path) {
    let letter = volume.to_string_lossy().chars().next().unwrap();
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(r"\\.\{letter}:"))
        .unwrap()
        .sync_all()
        .unwrap();
}

fn scan(e: &dyn Enumerator, root: &Path, relative: &[String]) -> ScanTree {
    e.scan(&ScanRequest {
        root,
        relative,
        policy: &ExclusionPolicy::with_defaults(),
        cancel: &CancelToken::new(),
        progress: &AtomicU64::new(0),
    })
    .unwrap()
}

fn by_path(t: &ScanTree) -> BTreeMap<String, (bool, EntryMeta)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![(ScanTree::ROOT, String::new())];
    while let Some((n, path)) = stack.pop() {
        for c in t.children(n) {
            let node = t.node(c);
            let p = if path.is_empty() {
                node.name.to_string()
            } else {
                format!("{path}\\{}", node.name)
            };
            out.insert(p.clone(), (node.is_dir, node.meta));
            stack.push((c, p));
        }
    }
    out
}

#[test]
fn mft_and_walker_agree_on_the_test_volume() {
    let volume = test_volume();
    let dir = TestDir::new(&volume);
    dir.file(r"docs\empty.txt", 0, 1);
    dir.file(r"docs\small.txt", 300, 2); // resident in the MFT record
    dir.file(r"docs\report.pdf", 50_000, 3);
    dir.file(r"docs\deep\a\b\c\big.bin", 300_000, 4);
    dir.file(r"日本語\写真 😀.jpg", 1234, 5);
    dir.file(r"trap.\inside.txt", 10, 6);
    let shared = dir.file(r"links\original.bin", 8192, 7);
    // Enough hard links to push the names into extension records ($ATTRIBUTE_LIST).
    for i in 0..40 {
        std::fs::hard_link(
            &shared,
            dir.path.join(format!(r"links\hard-link-number-{i:02}.bin")),
        )
        .unwrap();
    }
    std::fs::hard_link(&shared, dir.path.join("docs").join("another-link.bin")).unwrap();
    let ads = dir.file("ads.txt", 100, 8);
    let stream = PathBuf::from(format!("{}:extra", ads.display()));
    dir.fs.write(&stream, &vec![b's'; 20_000]).unwrap();
    // REVIEW L2: a compressed file and a sparse one, whose allocation is far below their
    // size (the walker reports neither attribute differently from the MFT).
    let compressed = dir.file(r"special\compressed.bin", 1 << 20, 9);
    file_tool(&volume, "compact", &["/c", "/q"], &compressed, &[]);
    let sparse = dir.file(r"special\sparse.bin", 1 << 20, 10);
    file_tool(&volume, "fsutil", &["sparse", "setflag"], &sparse, &[]);
    file_tool(
        &volume,
        "fsutil",
        &["sparse", "setrange"],
        &sparse,
        &["0", "1048576"],
    );
    flush_volume(&volume);

    let relative = vec![dir.name.clone()];
    let mft = scan(&MftEnumerator, &dir.path, &relative);
    let walk = scan(&WalkEnumerator::with_threads(2), &dir.path, &relative);
    let m = by_path(&mft);
    let w = by_path(&walk);
    assert_eq!(
        m.keys().collect::<Vec<_>>(),
        w.keys().collect::<Vec<_>>(),
        "same entries"
    );
    for (path, (is_dir, wm)) in &w {
        let (m_dir, mm) = &m[path];
        assert_eq!(is_dir, m_dir, "{path}");
        assert_eq!(mm.size, wm.size, "size of {path}");
        assert_eq!(mm.modified, wm.modified, "modified time of {path}");
        assert_eq!(mm.created, wm.created, "creation time of {path}");
        assert_eq!(mm.attrs, wm.attrs, "attributes of {path}");
        assert!(mm.file_ref.is_some(), "{path}");
    }
    assert_eq!(mft.root().meta.modified, walk.root().meta.modified);

    // Hard links: 42 names, one file reference, the allocation counted once.
    let links: Vec<&EntryMeta> = m
        .iter()
        .filter(|(_, (d, meta))| !d && meta.size == 8192)
        .map(|(_, (_, meta))| meta)
        .collect();
    assert_eq!(links.len(), 42);
    assert!(links.iter().all(|l| l.file_ref == links[0].file_ref));
    let alloc: u64 = links.iter().map(|l| l.alloc_size.unwrap()).sum();
    assert_eq!(alloc, 8192);
    // Alternate data streams occupy space: counted in the allocation, not in the size.
    let ads = m["ads.txt"].1;
    assert_eq!(ads.size, 100);
    assert!(ads.alloc_size.unwrap() >= 20_000, "{ads:?}");
    // Compressed and sparse files: the logical size, the attribute, and the space really used.
    const COMPRESSED: u32 = 0x800;
    const SPARSE: u32 = 0x200;
    let c = m[r"special\compressed.bin"].1;
    assert_eq!(c.size, 1 << 20);
    assert_ne!(c.attrs & COMPRESSED, 0, "{c:?}");
    assert!(
        c.alloc_size.unwrap() < 1 << 18,
        "compressed allocation {c:?}"
    );
    let s = m[r"special\sparse.bin"].1;
    assert_eq!(s.size, 1 << 20);
    assert_ne!(s.attrs & SPARSE, 0, "{s:?}");
    assert!(s.alloc_size.unwrap() < 1 << 16, "sparse allocation {s:?}");
}

#[test]
fn reading_a_missing_folder_fails_cleanly() {
    let volume = test_volume();
    let missing = vec!["prism-no-such-folder".to_owned()];
    let result = MftEnumerator.scan(&ScanRequest {
        root: &volume.join("prism-no-such-folder"),
        relative: &missing,
        policy: &ExclusionPolicy::with_defaults(),
        cancel: &CancelToken::new(),
        progress: &AtomicU64::new(0),
    });
    assert!(result.is_err());
}
