//! Smoke tests for the administrator test runner (docs/TESTING.md section 4).
//!
//! Compiled only with `--features admin-tests` and run by `scripts/admin/run-admin-tests.ps1`.
//! Read-only: nothing on any volume is modified.
#![cfg(all(windows, feature = "admin-tests"))]

use std::fs::File;
use std::path::Path;

/// Opening a raw volume handle requires administrator rights, so this proves the runner is
/// elevated. The handle is opened for reading only and closed immediately.
#[test]
fn runner_is_elevated() {
    let handle = File::open(r"\\.\C:");
    assert!(
        handle.is_ok(),
        "cannot open the C: volume handle; is the runner elevated? {:?}",
        handle.err()
    );
}

/// The dedicated NTFS test volume (label PRISMTEST) is mounted and its root is readable.
#[test]
fn test_volume_is_mounted() {
    let root =
        std::env::var("PRISM_TEST_VOLUME").expect("PRISM_TEST_VOLUME must be set by the runner");
    assert!(
        Path::new(&root).is_dir(),
        "test volume root {root} is not a directory"
    );
}
