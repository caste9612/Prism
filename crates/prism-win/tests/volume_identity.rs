//! F02-AC3 on a real volume: the test VHDX keeps its catalog identity when its drive letter
//! changes. Needs administrator rights (changing a drive letter); run by the admin runner
//! (docs/TESTING.md §4). Only the PRISMTEST test volume is touched, after checking its label
//! and its virtual bus; its letter is restored at the end, even if the test fails.
#![cfg(all(windows, feature = "admin-tests"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use prism_core::model::{BusType, VolumeKind};
use prism_core::store::{self, Store};
use prism_core::volumes::{self, DetectedVolume, VolumeMatch, VolumeSource, match_volumes};
use prism_win::volumes::WinVolumeSource;

fn same_mount(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\')
        .eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

/// The PRISMTEST volume, verified before anything is done to it.
fn test_volume() -> DetectedVolume {
    let root = std::env::var("PRISM_TEST_VOLUME").expect("PRISM_TEST_VOLUME is set by the runner");
    let v = WinVolumeSource
        .detect()
        .unwrap()
        .into_iter()
        .find(|d| d.mounts.iter().any(|m| same_mount(m, &root)))
        .expect("test volume detected");
    assert_eq!(
        v.info.os_label.as_deref(),
        Some("PRISMTEST"),
        "safety check"
    );
    assert_eq!(v.info.bus, Some(BusType::Virtual), "safety check");
    v
}

fn mountvol(args: &[&str]) {
    let out = Command::new("mountvol").args(args).output().unwrap();
    assert!(out.status.success(), "mountvol {args:?}: {out:?}");
}

/// Restores the original letter of the test volume when dropped.
struct LetterGuard {
    guid: String,
    original: String,
    current: String,
}

impl Drop for LetterGuard {
    fn drop(&mut self) {
        if !same_mount(&self.current, &self.original) {
            let _ = Command::new("mountvol")
                .args([&self.current, "/D"])
                .output();
            let _ = Command::new("mountvol")
                .args([&self.original, &self.guid])
                .output();
        }
    }
}

/// A free letter, avoiding the user's mapped network letters.
fn free_letter(taken: &[String]) -> String {
    ('P'..='W')
        .rev()
        .map(|c| format!("{c}:\\"))
        .find(|l| !Path::new(l).exists() && !taken.iter().any(|t| same_mount(t, l)))
        .expect("a free drive letter between P and W")
}

fn wait_for_mount(guid: &str, mount: &str) -> DetectedVolume {
    let started = Instant::now();
    loop {
        let found = WinVolumeSource
            .detect()
            .unwrap()
            .into_iter()
            .find(|d| d.info.volume_guid.as_deref() == Some(guid));
        if let Some(d) = found.filter(|d| d.mounts.iter().any(|m| same_mount(m, mount))) {
            return d;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "volume not seen at {mount}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[test]
fn f02_ac3_letter_change_keeps_the_catalog_identity() {
    let before = test_volume();
    let guid = before.info.volume_guid.clone().unwrap();
    let original = before.info.last_mount.clone().unwrap();
    assert_eq!(original.len(), 3, "the test volume has a drive letter");

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("catalog.db")).unwrap();
    let first = match_volumes(std::slice::from_ref(&before), &[]);
    assert_eq!(first, vec![VolumeMatch::New]);
    let id = volumes::record_detected(&store, &before, &first[0])
        .unwrap()
        .unwrap();

    let taken: Vec<String> = WinVolumeSource
        .detect()
        .unwrap()
        .into_iter()
        .filter(|d| d.info.kind == VolumeKind::Network)
        .flat_map(|d| d.mounts)
        .collect();
    let new_letter = free_letter(&taken);
    let mut guard = LetterGuard {
        guid: guid.clone(),
        original: original.clone(),
        current: original.clone(),
    };
    mountvol(&[&original, "/D"]);
    mountvol(&[&new_letter, &guid]);
    guard.current.clone_from(&new_letter);

    let after = wait_for_mount(&guid, &new_letter);
    let known = store.read(store::volumes).unwrap();
    let matched = match_volumes(std::slice::from_ref(&after), &known);
    assert_eq!(matched, vec![VolumeMatch::Known(id)]);
    volumes::record_detected(&store, &after, &matched[0]).unwrap();
    let v = store.read(|c| store::volume(c, id)).unwrap().unwrap();
    assert!(same_mount(
        v.info.last_mount.as_deref().unwrap(),
        &new_letter
    ));

    drop(guard);
    wait_for_mount(&guid, &original);
}
