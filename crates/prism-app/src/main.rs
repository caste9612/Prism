//! Prism desktop application (Tauri 2). Only wiring: commands, events, windows, tray; the
//! domain logic lives in `prism-core` and `prism-win` (ARCHITECTURE §2, ADR-0014).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup_state;
mod backup_views;
mod cleanup_views;
mod commands;
mod diagnostics_state;
mod dto;
mod duplicates_views;
mod hash_jobs;
mod health_state;
mod identity;
mod monitor;
mod netprobe;
mod notices;
mod search_views;
mod search_window;
mod shell_requests;
mod similar_state;
mod similar_views;
mod space_views;
mod state;
#[cfg(test)]
mod state_tests;
mod tray;
mod updates;
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
        hashing: hash_jobs::HashingState::default(),
        backup: backup_state::BackupState::default(),
        thumbnails: similar_views::Thumbnails::default(),
        identities: identity::Identities::default(),
        health: health_state::HealthState::default(),
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
    state.apply_hashing(&state.settings());
    create_main_window(app, &state.data_dir)?;
    search_window::create(app, &state.data_dir)?;
    let shortcut = state.settings().search_shortcut;
    if let Some(error) = search_window::register(app.handle(), &shortcut) {
        tracing::warn!(%error, "search shortcut not registered");
        *state.shortcut_error() = Some(error);
    }
    app.manage(Arc::clone(&state));
    tray::create(app.handle(), state.settings().hashing_paused)?;
    monitor::spawn(app.handle(), &state);
    // F22-AC3: updates, when this build is configured for them.
    let update_state = Arc::new(updates::UpdateState::default());
    app.manage(Arc::clone(&update_state));
    updates::spawn(app.handle(), update_state);
    shell_requests::spawn(app.handle(), &state);
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

/// Where the log files go (F20-AC1): `logs` in the data folder. Computed before Tauri starts,
/// the same way as [`data_dir`] (the local data folder of the identifier in release builds).
pub fn logs_dir() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let data = if cfg!(debug_assertions) {
        local.join("PrismDev").join("app")
    } else {
        local.join(IDENTIFIER)
    };
    data.join("logs")
}

/// The identifier of `tauri.conf.json`, which names the data folder of release builds.
const IDENTIFIER: &str = "com.prism.catalog";

/// Logs to the console and to a file a day, kept 14 days (F20-AC1). The guard flushes the file
/// when dropped, at exit.
fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };
    let console = tracing_subscriber::fmt::layer().with_filter(filter());
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("prism")
        .filename_suffix("log")
        .max_log_files(14)
        .build(logs_dir())
        .ok()
        .map(tracing_appender::non_blocking);
    match file {
        Some((writer, guard)) => {
            let layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer)
                .with_filter(filter());
            tracing_subscriber::registry()
                .with(console)
                .with(layer)
                .init();
            Some(guard)
        }
        None => {
            tracing_subscriber::registry().with(console).init();
            None
        }
    }
}

fn main() {
    let mut logs = init_logging();
    let app = tauri::Builder::default()
        // F19-AC4: starting Prism again brings the running instance to the front.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        // F22-AC3: signed updates (checked only by builds with a key and an address).
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            commands::recent_actions,
            commands::open_entry,
            commands::search,
            commands::hide_search,
            commands::cleanup_scan,
            commands::delete_entries,
            commands::duplicate_groups,
            commands::delete_duplicates,
            commands::backup_verify,
            commands::backup_compare,
            commands::backup_folder,
            commands::copy_preview,
            commands::copy_start,
            commands::copy_result,
            commands::similar_images,
            commands::image_thumbnail,
            commands::similar_videos,
            commands::open_logs,
            commands::reset_catalog,
            commands::ambiguous_volumes,
            commands::disk_health,
            commands::locate_path,
            commands::update_status,
            commands::check_updates,
            commands::install_update,
            commands::resolve_ambiguous,
            commands::diagnostics_preview,
            commands::export_diagnostics,
            commands::video_thumbnail,
            commands::reveal_entry,
            commands::remove_volume,
            commands::get_settings,
            commands::set_settings,
            commands::set_native_texts,
        ])
        .build(tauri::generate_context!());
    match app {
        // Tauri ends the process at the end of the event loop without dropping anything: the
        // catalog and the log file are closed here.
        Ok(app) => app.run(move |handle, event| {
            if let tauri::RunEvent::Exit = event {
                close_catalog(handle);
                drop(logs.take());
            }
        }),
        Err(e) => tracing::error!(error = %e, "Prism stopped with an error"),
    }
}

/// Records a clean exit of the catalog (NF4) on "Quit" and before an update is installed: in
/// both cases the process ends without dropping the managed state, and the next start would
/// otherwise check the whole catalog.
pub fn close_catalog(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        let clean = state.store.close(std::time::Duration::from_secs(5));
        tracing::info!(clean, "catalog closed");
    }
}
