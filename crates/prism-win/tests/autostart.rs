//! The start-with-Windows task (F19-AC3, ADR-0003). Registering a task that runs with the
//! highest privileges needs administrator rights: run by the admin runner (docs/TESTING.md §4).
//!
//! The test registers a **disabled** task in the `\PrismDevTests\` folder (it can never run),
//! reads it back and removes it. It never touches the real `Prism` task.
#![cfg(all(windows, feature = "admin-tests"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use prism_win::autostart::{self, AutostartTask};

/// Removes the test task even if an assertion fails.
struct Cleanup(String);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = autostart::remove(&self.0);
    }
}

#[test]
fn f19_ac3_logon_task_with_highest_privileges() {
    let name = format!(r"\PrismDevTests\autostart-{}", std::process::id());
    let _cleanup = Cleanup(name.clone());
    let exe = PathBuf::from(r"C:\Windows\System32\cmd.exe");
    let task = AutostartTask {
        name: name.clone(),
        exe: exe.clone(),
        args: "--autostart".into(),
        user: autostart::current_user().unwrap(),
        enabled: false,
    };

    assert!(!autostart::exists(&name).unwrap());
    autostart::install(&task).unwrap();
    assert!(autostart::exists(&name).unwrap());

    let xml = autostart::definition(&name).unwrap();
    assert!(
        xml.contains("<RunLevel>HighestAvailable</RunLevel>"),
        "{xml}"
    );
    assert!(xml.contains("<LogonTrigger>"), "{xml}");
    assert!(xml.contains(r"C:\Windows\System32\cmd.exe"), "{xml}");
    assert!(xml.contains("<Arguments>--autostart</Arguments>"), "{xml}");

    // Registering again replaces the task (the setting can be applied at every start).
    autostart::install(&task).unwrap();
    assert!(autostart::exists(&name).unwrap());

    autostart::remove(&name).unwrap();
    assert!(!autostart::exists(&name).unwrap());
}
