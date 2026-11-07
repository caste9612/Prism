//! The search window and its global shortcut (F07-AC1): a small window, created hidden at
//! startup, that the shortcut shows from any application and hides again.

use std::path::Path;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

pub const LABEL: &str = "search";

/// Creates the (hidden) search window; it shares the WebView2 data of the main window.
///
/// # Errors
/// Tauri errors creating the window.
pub fn create(app: &tauri::App, data_dir: &Path) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html?view=search".into()))
        .title("Prism")
        .inner_size(860.0, 580.0)
        .min_inner_size(520.0, 320.0)
        .center()
        .visible(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .data_directory(data_dir.join("webview"))
        .build()?;
    Ok(())
}

/// Shows the search window and gives it the focus, or hides it if it is already in front.
pub fn toggle(app: &AppHandle) {
    let Some(w) = app.get_webview_window(LABEL) else {
        return;
    };
    let in_front = w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false);
    if in_front {
        let _ = w.hide();
    } else {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit_to(LABEL, "search-focus", ());
    }
}

/// Registers `shortcut` for the search window in place of any previous one. Returns why it
/// could not be registered (taken by another program, malformed), for the interface to say so
/// and propose another one (F07-AC1).
pub fn register(app: &AppHandle, shortcut: &str) -> Option<String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let parsed: Shortcut = match shortcut.parse() {
        Ok(s) => s,
        Err(e) => return Some(format!("{shortcut}: {e}")),
    };
    gs.register(parsed)
        .err()
        .map(|e| format!("{shortcut}: {e}"))
}
