//! `prism-shell.exe`: started by the Explorer menu (F18) as `prism-shell analyze "<path>"` or
//! `prism-shell duplicates "<path>"`. It is not elevated, so it never shows a UAC prompt: it
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
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
    }
}
