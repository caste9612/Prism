//! `prism-shell.exe`: started by the Explorer menu (F18) as `prism-shell analyze "<path>"` or
//! `prism-shell duplicates "<path>"`. The installer also runs it once with `--install` and the
//! uninstaller with `--uninstall` (F18-AC3, F22-AC1/AC2): they register or remove the menu
//! entries and the scheduled task. It is not elevated, so it never shows a UAC prompt: it
//! hands the request to the running Prism through its named pipe; if Prism is not running it
//! starts it through its scheduled task (which runs with the highest privileges without asking,
//! ADR-0003) and then hands the request over (F18-AC2).
#![windows_subsystem = "windows"]

use std::process::ExitCode;
use std::time::Duration;

/// The actions of the menu.
const ACTIONS: [&str; 2] = ["analyze", "duplicates"];

/// The request sent on the pipe: the action and the path, separated by a tab.
fn request(args: &[String]) -> Option<String> {
    match args {
        [_, action, path] if ACTIONS.contains(&action.as_str()) && !path.trim().is_empty() => {
            Some(format!("{action}\t{}", path.trim()))
        }
        _ => None,
    }
}

/// What the installer asks for.
#[derive(Debug, PartialEq, Eq)]
enum Setup {
    Install,
    Uninstall,
}

fn setup(args: &[String]) -> Option<Setup> {
    match args {
        [_, flag] if flag == "--install" => Some(Setup::Install),
        [_, flag] if flag == "--uninstall" => Some(Setup::Uninstall),
        _ => None,
    }
}

/// Registers the Explorer menu and the scheduled task of Prism, which sits next to this file.
/// The task starts Prism at logon by default (F19-AC3) and on demand from the menu (F18-AC2).
fn install() -> Result<(), String> {
    let shell = std::env::current_exe().map_err(|e| e.to_string())?;
    let prism = shell.with_file_name("prism.exe");
    let texts = if prism_win::system::ui_is_italian() {
        prism_win::shell_menu::MenuTexts {
            analyze: "Analizza con Prism",
            duplicates: "Cerca duplicati qui",
        }
    } else {
        prism_win::shell_menu::MenuTexts {
            analyze: "Analyze with Prism",
            duplicates: "Find duplicates here",
        }
    };
    prism_win::shell_menu::register(prism_win::shell_menu::CLASSES, &shell, &prism, &texts)
        .map_err(|e| e.to_string())?;
    prism_win::autostart::install(&prism_win::autostart::AutostartTask {
        name: prism_win::autostart::TASK_NAME.to_owned(),
        exe: prism,
        args: "--autostart".to_owned(),
        user: prism_win::autostart::current_user().map_err(|e| e.to_string())?,
        enabled: true,
        at_logon: true,
    })
    .map_err(|e| e.to_string())
}

/// Removes what [`install`] registered; what is already gone is not an error.
fn uninstall() -> Result<(), String> {
    prism_win::shell_menu::unregister(prism_win::shell_menu::CLASSES).map_err(|e| e.to_string())?;
    prism_win::autostart::remove(prism_win::autostart::TASK_NAME).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if let Some(what) = setup(&args) {
        let result = match what {
            Setup::Install => install(),
            Setup::Uninstall => uninstall(),
        };
        return if result.is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(5)
        };
    }
    let Some(request) = request(&args) else {
        return ExitCode::from(2);
    };
    let pipe = prism_win::pipe::name(cfg!(debug_assertions));
    if prism_win::pipe::send(&pipe, &request, Duration::from_millis(300)).is_ok() {
        return ExitCode::SUCCESS;
    }
    // Prism is not running: start it without a UAC prompt, then wait for its pipe.
    if prism_win::autostart::run(prism_win::autostart::TASK_NAME).is_err() {
        return ExitCode::from(3);
    }
    match prism_win::pipe::send(&pipe, &request, Duration::from_secs(30)) {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(4),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_from_the_command_line() {
        let args = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            request(&args(&["prism-shell", "analyze", r"D:\Foto"])).as_deref(),
            Some("analyze\tD:\\Foto")
        );
        assert_eq!(
            request(&args(&["prism-shell", "duplicates", "C:\\"])).as_deref(),
            Some("duplicates\tC:\\")
        );
        assert!(request(&args(&["prism-shell", "delete", "C:\\"])).is_none());
        assert!(request(&args(&["prism-shell", "analyze", "  "])).is_none());
        assert!(request(&args(&["prism-shell"])).is_none());
        // The installer's flags are not menu requests.
        assert!(request(&args(&["prism-shell", "--install"])).is_none());
        assert_eq!(
            setup(&args(&["prism-shell", "--install"])),
            Some(Setup::Install)
        );
        assert_eq!(
            setup(&args(&["prism-shell", "--uninstall"])),
            Some(Setup::Uninstall)
        );
        assert_eq!(setup(&args(&["prism-shell", "analyze", "C:\\"])), None);
    }
}
