//! Notification-area icon (F19): open Prism, search, the jobs in progress, pause or resume the
//! content hashes (F19-AC2, F06-AC5), quit. Closing the window only hides it; jobs go on (F19-AC1). Labels come
//! from the interface in the chosen language.

use std::sync::{Arc, Mutex};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::dto::NativeTexts;
use crate::state::AppState;

/// The menu items whose text the interface sets.
pub struct TrayItems {
    open: MenuItem<Wry>,
    search: MenuItem<Wry>,
    status: MenuItem<Wry>,
    hashing: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    pause_text: String,
    resume_text: String,
    idle_text: String,
    running_text: String,
    jobs: usize,
}

impl TrayItems {
    fn status_text(&self) -> String {
        if self.jobs == 0 {
            self.idle_text.clone()
        } else {
            self.running_text.replace("{count}", &self.jobs.to_string())
        }
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Pauses or resumes the hashes from the menu; the setting is saved and the interface told.
fn toggle_hashing(app: &AppHandle) {
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return;
    };
    let mut settings = state.settings();
    settings.hashing_paused = !settings.hashing_paused;
    if let Err(e) = state.save_settings(&settings) {
        tracing::warn!(code = %e.code, message = %e.message, "hashing pause not saved");
        return;
    }
    state.apply_hashing(&settings);
    set_hashing_paused(app, settings.hashing_paused);
    let _ = app.emit("settings-changed", ());
    let _ = app.emit("volumes-changed", ());
}

/// Creates the tray icon.
///
/// # Errors
/// Tauri errors creating the menu or the icon.
pub fn create(app: &AppHandle, hashing_paused: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Prism", true, None::<&str>)?;
    let search = MenuItem::with_id(app, "search", "Search", true, None::<&str>)?;
    let (pause_text, resume_text) = ("Pause content analysis", "Resume content analysis");
    let hashing = MenuItem::with_id(
        app,
        "hashing",
        if hashing_paused {
            resume_text
        } else {
            pause_text
        },
        true,
        None::<&str>,
    )?;
    let (idle_text, running_text) = ("No jobs in progress", "Jobs in progress: {count}");
    // Information only: not clickable.
    let status = MenuItem::with_id(app, "status", idle_text, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &search,
            &separator,
            &status,
            &hashing,
            &separator2,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("prism")
        .tooltip("Prism")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "search" => crate::search_window::toggle(app),
            "hashing" => toggle_hashing(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(Mutex::new(TrayItems {
        open,
        search,
        status,
        hashing,
        quit,
        pause_text: pause_text.to_owned(),
        resume_text: resume_text.to_owned(),
        idle_text: idle_text.to_owned(),
        running_text: running_text.to_owned(),
        jobs: 0,
    }));
    Ok(())
}

pub fn set_labels(app: &AppHandle, texts: &NativeTexts, hashing_paused: bool) {
    if let Some(items) = app.try_state::<Mutex<TrayItems>>()
        && let Ok(mut items) = items.lock()
    {
        let _ = items.open.set_text(&texts.open);
        let _ = items.search.set_text(&texts.search);
        let _ = items.quit.set_text(&texts.quit);
        if !texts.pause_hashing.is_empty() {
            items.pause_text.clone_from(&texts.pause_hashing);
        }
        if !texts.resume_hashing.is_empty() {
            items.resume_text.clone_from(&texts.resume_hashing);
        }
        if !texts.jobs_idle.is_empty() {
            items.idle_text.clone_from(&texts.jobs_idle);
        }
        if !texts.jobs_running.is_empty() {
            items.running_text.clone_from(&texts.jobs_running);
        }
        let _ = items.status.set_text(items.status_text());
        let label = if hashing_paused {
            &items.resume_text
        } else {
            &items.pause_text
        };
        let _ = items.hashing.set_text(label);
    }
}

/// Shows "Resume" while the hashes are paused and "Pause" otherwise.
pub fn set_hashing_paused(app: &AppHandle, paused: bool) {
    if let Some(items) = app.try_state::<Mutex<TrayItems>>()
        && let Ok(items) = items.lock()
    {
        let label = if paused {
            &items.resume_text
        } else {
            &items.pause_text
        };
        let _ = items.hashing.set_text(label);
    }
}

/// Shows how many jobs are queued or running (F19-AC2); the menu changes only when the number
/// does.
pub fn set_job_count(app: &AppHandle, jobs: usize) {
    if let Some(items) = app.try_state::<Mutex<TrayItems>>()
        && let Ok(mut items) = items.lock()
        && items.jobs != jobs
    {
        items.jobs = jobs;
        let _ = items.status.set_text(items.status_text());
    }
}
