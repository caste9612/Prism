//! The USN journal and single-file fetches on the PRISMTEST test volume. Needs administrator
//! rights; run by the admin runner (docs/TESTING.md §4). The journal is created only on the
//! test volume if missing (Prism never creates journals on user volumes); files are created
//! and removed in a folder of the test volume through the sandbox guard.
#![cfg(all(windows, feature = "admin-tests"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::time::SystemTime;

use prism_core::cancel::CancelToken;
use prism_core::exclusion::ExclusionPolicy;
use prism_core::model::BusType;
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::scan::{Enumerator, ScanRequest, ScanTree};
use prism_core::volumes::VolumeSource;
use prism_win::mft::{MftEnumerator, fetch_file};
use prism_win::usn::{
    REASON_FILE_CREATE, REASON_FILE_DELETE, REASON_HARD_LINK_CHANGE, REASON_RENAME_NEW_NAME,
    REASON_RENAME_OLD_NAME, UsnError, UsnRecord, check_position, query_journal, read_journal,
};
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

fn open_volume(volume: &Path) -> File {
    File::open(format!(r"\\.\{}:", letter(volume))).unwrap()
}

/// Creates the journal on the TEST volume only, when it has none.
fn ensure_journal(volume: &Path) {
    if query_journal(&open_volume(volume)).unwrap().is_none() {
        let out = std::process::Command::new("fsutil")
            .args(["usn", "createjournal", "m=33554432", "a=4194304"])
            .arg(format!("{}:", letter(volume)))
            .output()
            .unwrap();
        assert!(out.status.success(), "fsutil createjournal: {out:?}");
    }
}

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
        let name = format!("prism-usn-{}-{nanos}", std::process::id());
        let path = std::fs::canonicalize(volume).unwrap().join(&name);
        fs.create_dir_all(&path).unwrap();
        Self { path, name, fs }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = self.fs.remove_dir_all(&self.path);
    }
}

fn find<'a>(records: &'a [UsnRecord], name: &str, reason: u32) -> &'a UsnRecord {
    records
        .iter()
        .find(|r| String::from_utf16_lossy(&r.name) == name && r.reason & reason != 0)
        .unwrap_or_else(|| panic!("no record {name} with reason {reason:#x}"))
}

#[test]
fn journal_records_and_single_file_fetches() {
    let volume = test_volume();
    ensure_journal(&volume);
    let handle = open_volume(&volume);
    let info = query_journal(&handle).unwrap().expect("journal");
    let dir = TestDir::new(&volume);

    let a = dir.path.join("a.txt");
    dir.fs.write(&a, b"first").unwrap();
    let b = dir.path.join("renamed.txt");
    dir.fs.rename(&a, &b).unwrap();
    dir.fs.write(&b, &vec![b'x'; 70_000]).unwrap();
    let gone = dir.path.join("gone.txt");
    dir.fs.write(&gone, b"bye").unwrap();
    dir.fs.remove_file(&gone).unwrap();
    let shared = dir.path.join("shared.bin");
    dir.fs.write(&shared, &vec![b's'; 4096]).unwrap();
    for i in 0..40 {
        std::fs::hard_link(
            &shared,
            dir.path.join(format!("hard-link-number-{i:02}.bin")),
        )
        .unwrap();
    }

    let batch = read_journal(&handle, info.id, info.next_usn, 100_000).unwrap();
    assert!(batch.next_usn > info.next_usn);
    let records = &batch.records;
    let created = find(records, "a.txt", REASON_FILE_CREATE);
    let old = find(records, "a.txt", REASON_RENAME_OLD_NAME);
    let new = find(records, "renamed.txt", REASON_RENAME_NEW_NAME);
    assert_eq!(created.file_ref, old.file_ref);
    assert_eq!(
        old.file_ref, new.file_ref,
        "a rename keeps the file reference"
    );
    let deleted = find(records, "gone.txt", REASON_FILE_DELETE);
    find(records, "hard-link-number-39.bin", REASON_HARD_LINK_CHANGE);
    let dir_ref = find(records, &dir.name, REASON_FILE_CREATE).file_ref;
    assert_eq!(new.parent_ref, dir_ref);

    // Nothing new: reading again from the end returns no records.
    let again = read_journal(&handle, info.id, batch.next_usn, 100).unwrap();
    assert!(again.records.is_empty());
    assert!(matches!(
        check_position(&handle, info.id ^ 1, batch.next_usn),
        Err(UsnError::Lost)
    ));
    check_position(&handle, info.id, batch.next_usn).unwrap();

    // Write the cached metadata of the test volume to disk: the non-resident attribute list
    // of the hard-linked file and the full MFT read below come from the disk.
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(r"\\.\{}:", letter(&volume)))
        .unwrap()
        .sync_all()
        .unwrap();

    // Single-file fetches give the same metadata as the full MFT read.
    let renamed = fetch_file(&handle, new.file_ref)
        .unwrap()
        .expect("renamed file");
    assert_eq!(renamed.names.len(), 1);
    assert_eq!(renamed.name(0), "renamed.txt");
    assert_eq!(renamed.names[0].0, dir_ref);
    assert!(fetch_file(&handle, deleted.file_ref).unwrap().is_none());
    let links_ref = find(records, "shared.bin", REASON_FILE_CREATE).file_ref;
    let links = fetch_file(&handle, links_ref)
        .unwrap()
        .expect("hard-linked file");
    assert_eq!(links.names.len(), 41, "names from the extension records");

    let relative = vec![dir.name.clone()];
    let tree = MftEnumerator
        .scan(&ScanRequest {
            root: &dir.path,
            relative: &relative,
            policy: &ExclusionPolicy::with_defaults(),
            cancel: &CancelToken::new(),
            progress: &AtomicU64::new(0),
        })
        .unwrap();
    let in_tree = |name: &str| {
        tree.children(ScanTree::ROOT)
            .map(|n| tree.node(n))
            .find(|n| n.name == name)
            .unwrap_or_else(|| panic!("{name} not in the tree"))
            .meta
    };
    assert_eq!(renamed.meta(0), in_tree("renamed.txt"));
    let alloc: u64 = (0..links.names.len())
        .map(|i| links.meta(i).alloc_size.unwrap())
        .sum();
    assert_eq!(alloc, 4096, "allocation counted once over the 41 names");
    for i in 0..links.names.len() {
        assert_eq!(links.meta(i), in_tree(&links.name(i)), "{}", links.name(i));
    }
}
