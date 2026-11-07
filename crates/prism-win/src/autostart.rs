//! Start with Windows (F19-AC3, ADR-0003): a scheduled task that runs Prism at the user's logon
//! "with the highest privileges", so the elevated application starts without a UAC prompt.
//!
//! The task is registered with `schtasks.exe` from an XML definition (the only way to set every
//! option, such as the run level and the battery settings, without the COM API). Creating a
//! task that runs elevated needs administrator rights, which Prism has. Running it does not:
//! a non-elevated process can start it with `schtasks /run` (verified with the admin test
//! runner, which uses the same mechanism: docs/TESTING.md §4).
//!
//! Only exit codes of `schtasks.exe` are used: its messages are localized.

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Name of the task registered by Prism (and by its installer, T10).
pub const TASK_NAME: &str = "Prism";

/// Do not open a console window for `schtasks.exe` (Prism is a GUI application).
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A logon task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutostartTask {
    /// Task name, optionally with folders (`\PrismDevTests\x`).
    pub name: String,
    /// Program to start.
    pub exe: PathBuf,
    /// Its arguments.
    pub args: String,
    /// `DOMAIN\user` the task belongs to: it starts at this user's logon only.
    pub user: String,
    /// A disabled task is registered but never runs (used by the tests).
    pub enabled: bool,
    /// Whether the task starts at logon; otherwise it runs only on demand (F18-AC2).
    pub at_logon: bool,
}

/// Errors of the autostart task.
#[derive(Debug, thiserror::Error)]
pub enum AutostartError {
    #[error("cannot run schtasks.exe: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("schtasks.exe failed (exit code {code:?}): {stderr}")]
    Failed { code: Option<i32>, stderr: String },
    #[error("the current user is unknown")]
    NoUser,
}

/// `DOMAIN\user` of the current process.
///
/// # Errors
/// [`AutostartError::NoUser`] if the environment does not name the user.
pub fn current_user() -> Result<String, AutostartError> {
    let user = std::env::var("USERNAME").map_err(|_| AutostartError::NoUser)?;
    Ok(match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    })
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The task definition (Task Scheduler schema 1.2).
#[must_use]
pub fn task_xml(task: &AutostartTask) -> String {
    let user = xml_escape(&task.user);
    let exe = xml_escape(&task.exe.to_string_lossy());
    let args = xml_escape(&task.args);
    let dir = task
        .exe
        .parent()
        .map(|d| xml_escape(&d.to_string_lossy()))
        .unwrap_or_default();
    let enabled = if task.enabled { "true" } else { "false" };
    let at_logon = if task.at_logon { "true" } else { "false" };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Prism at logon (when enabled) or on demand from Explorer, without a UAC prompt.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>{at_logon}</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT10S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>{enabled}</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{exe}"</Command>
      <Arguments>{args}</Arguments>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn schtasks(args: &[&str]) -> Result<Output, AutostartError> {
    Ok(Command::new("schtasks.exe")
        .args(args)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()?)
}

fn check(out: &Output) -> Result<(), AutostartError> {
    if out.status.success() {
        Ok(())
    } else {
        Err(AutostartError::Failed {
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        })
    }
}

/// Registers (or replaces) the task. Needs administrator rights.
///
/// # Errors
/// [`AutostartError`] if `schtasks.exe` fails.
pub fn install(task: &AutostartTask) -> Result<(), AutostartError> {
    // schtasks reads the definition from a file, in UTF-16 with a byte order mark.
    let (path, mut file) = tempfile_in_temp()?;
    let mut bytes = vec![0xFF, 0xFE];
    for unit in task_xml(task).encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let written = file.write_all(&bytes).and_then(|()| file.flush());
    // schtasks opens the file exclusively: close it first.
    drop(file);
    let out = written.map_err(AutostartError::from).and_then(|()| {
        let p = path.to_string_lossy();
        schtasks(&["/create", "/tn", &task.name, "/xml", &p, "/f"])
    });
    let _ = std::fs::remove_file(&path);
    check(&out?)
}

/// A new file in the temporary folder, with a unique name.
fn tempfile_in_temp() -> std::io::Result<(PathBuf, std::fs::File)> {
    let dir = std::env::temp_dir();
    for n in 0..100u32 {
        let path = dir.join(format!("prism-task-{}-{n}.xml", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("no free temporary file name"))
}

/// Whether a task with this name exists.
///
/// # Errors
/// [`AutostartError::Spawn`] if `schtasks.exe` cannot run.
pub fn exists(name: &str) -> Result<bool, AutostartError> {
    Ok(schtasks(&["/query", "/tn", name])?.status.success())
}

/// The registered definition of a task, as XML (for checks and diagnostics).
///
/// # Errors
/// [`AutostartError`] if the task does not exist or cannot be read.
pub fn definition(name: &str) -> Result<String, AutostartError> {
    let out = schtasks(&["/query", "/tn", name, "/xml"])?;
    check(&out)?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Removes the task; nothing to do if it does not exist.
///
/// # Errors
/// [`AutostartError`] if it exists and cannot be removed.
pub fn remove(name: &str) -> Result<(), AutostartError> {
    if !exists(name)? {
        return Ok(());
    }
    check(&schtasks(&["/delete", "/tn", name, "/f"])?)
}

/// Brings the task in line with the setting: registered for `exe` when enabled, absent when
/// disabled. Prism calls it at startup and when the setting changes.
///
/// # Errors
/// [`AutostartError`] from [`install`] or [`remove`].
pub fn apply(enabled: bool, exe: &Path, args: &str) -> Result<(), AutostartError> {
    // The task stays registered either way: the Explorer menu starts Prism through it without a
    // UAC prompt (F18-AC2); only its logon trigger follows the setting (REVIEW D45).
    install(&AutostartTask {
        name: TASK_NAME.to_owned(),
        exe: exe.to_path_buf(),
        args: args.to_owned(),
        user: current_user()?,
        enabled: true,
        at_logon: enabled,
    })
}

/// Starts a registered task now (`schtasks /run`), which a non-elevated process may do.
///
/// # Errors
/// [`AutostartError`] if the task does not exist or cannot start.
pub fn run(name: &str) -> Result<(), AutostartError> {
    check(&schtasks(&["/run", "/tn", name])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_definition() {
        let xml = task_xml(&AutostartTask {
            name: TASK_NAME.into(),
            exe: PathBuf::from(r"C:\Program Files\Prism & Co\prism.exe"),
            args: "--autostart".into(),
            user: r"PC\Mario".into(),
            enabled: true,
            at_logon: false,
        });
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<LogonTrigger>\n      <Enabled>false</Enabled>"));
        assert_eq!(xml.matches(r"<UserId>PC\Mario</UserId>").count(), 2);
        assert!(xml.contains(r#"<Command>"C:\Program Files\Prism &amp; Co\prism.exe"</Command>"#));
        assert!(xml.contains("<Arguments>--autostart</Arguments>"));
        assert!(
            xml.contains(r"<WorkingDirectory>C:\Program Files\Prism &amp; Co</WorkingDirectory>")
        );
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(
            xml.contains("<Settings>") && xml.contains("<Enabled>true</Enabled>\n    <Hidden>")
        );
    }

    #[test]
    fn current_user_is_known() {
        let user = current_user().unwrap_or_default();
        assert!(!user.is_empty());
    }

    // Querying a task that does not exist is not an error, and removing it is a no-op. Neither
    // needs administrator rights.
    #[test]
    fn missing_task() {
        let name = r"\PrismDevTests\does-not-exist";
        assert!(!exists(name).unwrap_or(true));
        assert!(remove(name).is_ok());
        assert!(definition(name).is_err());
    }
}
