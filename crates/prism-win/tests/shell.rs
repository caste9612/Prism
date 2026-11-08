//! Opening without administrator rights (ADR-0003): from the elevated runner (docs/TESTING.md
//! §4), a program started through the desktop shell must run at medium integrity, like one
//! started from Explorer.
//!
//! The program is `cmd.exe` with a hidden window, which writes its own groups (`whoami
//! /groups`) to a file in a temporary folder of the test and exits: nothing visible on the
//! desktop, nothing written elsewhere.
#![cfg(all(windows, feature = "admin-tests"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::{Duration, Instant};

use prism_win::shell::{Show, execute_unelevated};

/// Well-known SIDs of the mandatory integrity levels.
const MEDIUM: &str = "S-1-16-8192";
const HIGH: &str = "S-1-16-12288";

#[test]
fn programs_start_unelevated_through_the_desktop_shell() {
    assert!(
        prism_win::process::is_elevated(),
        "the test must run elevated"
    );
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("groups.txt");
    let args = format!("/c whoami /groups > \"{}\"", out.display());
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let cmd = Path::new(&system).join("System32").join("cmd.exe");

    // Control: the same program started directly by this process runs elevated.
    let direct = std::process::Command::new(&cmd)
        .args(["/c", "whoami", "/groups"])
        .output()
        .unwrap();
    let direct = String::from_utf8_lossy(&direct.stdout);
    assert!(
        direct.contains(HIGH),
        "control run is not elevated:\n{direct}"
    );

    execute_unelevated(&cmd, &args, Some(dir.path()), Show::Hidden).unwrap();

    // The program runs asynchronously: wait for its complete output.
    let start = Instant::now();
    let text = loop {
        let text = std::fs::read(&out)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        if text.contains("S-1-16-") {
            break text;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "no output from the unelevated program: {text:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(text.contains(MEDIUM), "expected medium integrity:\n{text}");
    assert!(!text.contains(HIGH), "the program runs elevated:\n{text}");
}
