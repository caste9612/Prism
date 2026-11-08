//! The Recycle Bin remover (ADR-0012, F16-AC1/AC2) on the PRISMTEST test volume only: files
//! and folders created there by the test go to that volume's Recycle Bin, which the test then
//! empties (only that volume's Bin, for the current user). Run by the admin runner
//! (docs/TESTING.md §4), like the other tests on the test volume.
#![cfg(all(windows, feature = "admin-tests"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use prism_core::fileops::{Disposal, Remover};
use prism_core::model::BusType;
use prism_core::sandbox::{GuardedFs, SandboxGuard};
use prism_core::volumes::VolumeSource;
use prism_win::trash::{ShellRemover, empty_recycle_bin_of, recycle_bin_info};
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

#[test]
fn f16_ac1_recycle_bin_and_permanent_deletion_on_the_test_volume() {
    let root = test_volume();
    let guard = SandboxGuard::new([&root]).unwrap();
    let fs = GuardedFs::new(SandboxGuard::new([&root]).unwrap());
    let dir = root.join(format!("prism-trash-{}", std::process::id()));
    fs.create_dir_all(&dir.join("sub")).unwrap();
    fs.write(&dir.join("a.txt"), b"recycle me").unwrap();
    fs.write(&dir.join("sub").join("b.txt"), b"and me").unwrap();
    fs.write(&dir.join("c.txt"), b"delete me").unwrap();

    empty_recycle_bin_of(&root).ok();
    let before = recycle_bin_info(&root).expect("Recycle Bin of the test volume");

    // To the Recycle Bin: a file and a folder with its content.
    let recycled = [dir.join("a.txt"), dir.join("sub")];
    for p in &recycled {
        guard.check_no_follow(p).unwrap();
    }
    let failed = ShellRemover.remove(&recycled, Disposal::RecycleBin);
    assert!(failed.is_empty(), "{failed:?}");
    assert!(recycled.iter().all(|p| !p.exists()));
    let after = recycle_bin_info(&root).unwrap();
    assert_eq!(after.0, before.0 + 2, "two items in the Recycle Bin");
    assert!(after.1 > before.1);

    // Permanent deletion (F16-AC2): nothing more in the Recycle Bin.
    let deleted = [dir.join("c.txt")];
    guard.check_no_follow(&deleted[0]).unwrap();
    assert!(
        ShellRemover
            .remove(&deleted, Disposal::Permanent)
            .is_empty()
    );
    assert!(!deleted[0].exists());
    assert_eq!(recycle_bin_info(&root).unwrap().0, after.0);

    // Clean up: the test folder and the test volume's Recycle Bin.
    fs.remove_dir_all(&dir).unwrap();
    empty_recycle_bin_of(&root).unwrap();
    assert_eq!(recycle_bin_info(&root).unwrap().0, 0);
}
