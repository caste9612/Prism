//! Prism desktop application (Tauri 2). Only wiring: commands, events, windows, tray; the
//! domain logic lives in `prism-core` and `prism-win` (ARCHITECTURE §2, ADR-0014).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cleanup_views;
mod commands;
mod dto;
mod monitor;
mod netprobe;
mod notices;
mod search_views;
mod search_window;
mod space_views;
mod state;
#[cfg(test)]
mod state_tests;
mod tray;
mod watchers;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use prism_core::jobs::JobEngine;
use prism_core::store::Store;
use tauri::{Manager, WebviewWindowBuilder, WindowEvent};

use crate::netprobe::{NetProbe, ProbeTiming};
use crate::notices::Notices;
use crate::state::AppState;

/// Catalog folder: development builds use `%LOCALAPPDATA%\PrismDev\app` (TESTING S4), release
/// builds the application data folder.
fn data_dir(app: &tauri::App) -> PathBuf {
    if cfg!(debug_assertions) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("PrismDev")
            .join("app")
    } else {
        app.path()
            .app_local_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("Prism"))
    }
}

/// Started with Windows (F19-AC3): Prism starts hidden in the notification area.
pub const AUTOSTART_ARG: &str = "--autostart";

/// The main window, from its configuration in `tauri.conf.json` (`create: false`). Created here
/// so that WebView2 keeps its data next to the catalog: by default it would use the folder of
/// the identifier, also in development builds (TESTING S4).
fn create_main_window(app: &tauri::App, data_dir: &Path) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .unwrap_or_default();
    let hidden = std::env::args().any(|a| a == AUTOSTART_ARG);
    WebviewWindowBuilder::from_config(app.handle(), &config)?
        .data_directory(data_dir.join("webview"))
        .visible(!hidden)
        .build()?;
    Ok(())
}

/// Identifier under which Prism's notifications appear. Development builds borrow the one of
/// Windows PowerShell, so nothing is written to the registry; release builds register their
/// own, with the Prism icon saved in the data folder (`prism_win::notify`).
fn notification_app_id(app: &tauri::App, data_dir: &Path) -> String {
    if cfg!(debug_assertions) {
        return prism_win::notify::DEV_APP_ID.to_owned();
    }
    let id = app.config().identifier.clone();
    let icon = data_dir.join("prism.png");
    if !icon.exists()
        && let Err(e) = std::fs::write(&icon, include_bytes!("../icons/128x128.png"))
    {
        tracing::warn!(error = %e, "cannot save the notification icon");
    }
    if let Err(e) = prism_win::notify::register_app_id(&id, "Prism", Some(&icon)) {
        tracing::warn!(error = %e, "notifications may not appear");
    }
    id
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(app);
    let store = Store::open(&dir.join("catalog.db"))?;
    let app_id = notification_app_id(app, &dir);
    let state = Arc::new(AppState {
        notices: Mutex::new(Notices::default()),
        search: Arc::new(prism_core::search::live::LiveIndexes::default()),
        shortcut_error: Mutex::new(None),
        app_id,
        store,
        engine: JobEngine::start(4),
        online: Mutex::new(HashMap::new()),
        elevated: prism_win::process::is_elevated(),
        data_dir: dir,
        dev: cfg!(debug_assertions),
        net: Arc::new(NetProbe::new(
            Arc::new(prism_win::volumes::disk_space),
            ProbeTiming::default(),
        )),
    });
    tracing::info!(elevated = state.elevated, data = %state.data_dir.display(), "Prism started");
    create_main_window(app, &state.data_dir)?;
    search_window::create(app, &state.data_dir)?;
    let shortcut = state.settings().search_shortcut;
    if let Some(error) = search_window::register(app.handle(), &shortcut) {
        tracing::warn!(%error, "search shortcut not registered");
        *state.shortcut_error() = Some(error);
    }
    app.manage(Arc::clone(&state));
    tray::create(app.handle())?;
    monitor::spawn(app.handle(), &state);
    // F19-AC3: keep the task in line with the setting and with the current location of the
    // program (it may have moved or been updated). Off the startup path.
    let s = Arc::clone(&state);
    let _ = std::thread::Builder::new()
        .name("prism-autostart".into())
        .spawn(move || {
            if let Err(e) = s.apply_autostart(s.settings().autostart) {
                tracing::warn!(code = %e.code, message = %e.message, "start with Windows");
            }
        });
    Ok(())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let result = tauri::Builder::default()
        // F19-AC4: starting Prism again brings the running instance to the front.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        // F07-AC1: the global shortcut shows or hides the search window.
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        search_window::toggle(app);
                    }
                })
                .build(),
        )
        .setup(setup)
        .on_window_event(|window, event| {
            // F19-AC1: closing the window hides Prism in the notification area.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_volumes,
            commands::get_jobs,
            commands::index_volume,
            commands::cancel_job,
            commands::set_labels,
            commands::set_ignored,
            commands::open_volume,
            commands::space_folder,
            commands::largest_files,
            commands::type_stats,
            commands::open_entry,
            commands::search,
            commands::hide_search,
            commands::cleanup_scan,
            commands::delete_entries,
            commands::reveal_entry,
            commands::remove_volume,
            commands::get_settings,
            commands::set_settings,
            commands::set_native_texts,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!(error = %e, "Prism stopped with an error");
    }
}
