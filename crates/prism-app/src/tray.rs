//! Notification-area icon (F19): open Prism, quit. Closing the window only hides it; jobs go on
//! (F19-AC1). Labels come from the interface in the chosen language.

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

/// The menu items whose text the interface sets.
pub struct TrayItems {
    open: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Creates the tray icon.
///
/// # Errors
/// Tauri errors creating the menu or the icon.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Prism", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id("prism")
        .tooltip("Prism")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
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
    app.manage(Mutex::new(TrayItems { open, quit }));
    Ok(())
}

pub fn set_labels(app: &AppHandle, open: &str, quit: &str) {
    if let Some(items) = app.try_state::<Mutex<TrayItems>>()
        && let Ok(items) = items.lock()
    {
        let _ = items.open.set_text(open);
        let _ = items.quit.set_text(quit);
    }
}
