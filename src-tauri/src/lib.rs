//! Prism - High-performance disk analyzer and deduplicator
//!
//! This library provides the core functionality for scanning filesystems,
//! detecting duplicates, and analyzing disk usage.

pub mod commands;
pub mod database;
pub mod duplicates;
pub mod error;
pub mod scanner;
pub mod search;
pub mod utils;

pub use error::{PrismError, PrismResult};

use database::Database;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{image::Image, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
use tokio::sync::Mutex;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Show or create the quick search window
fn show_search_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("search") {
        info!("Showing existing search window");
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        info!("Creating new search window");
        // Create a new search window
        match WebviewWindowBuilder::new(app, "search", WebviewUrl::App("/search".into()))
            .title("Prism Quick Search")
            .inner_size(700.0, 500.0)
            .min_inner_size(500.0, 300.0)
            .center()
            .decorations(false)
            .transparent(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(true)
            .build()
        {
            Ok(window) => {
                info!("Search window created successfully");
                let _ = window.set_focus();
            }
            Err(e) => {
                info!("Failed to create search window: {}", e);
            }
        }
    }
}

/// Close the quick search window
#[tauri::command]
fn close_search_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("search") {
        info!("Closing search window");
        let _ = window.close();
    }
}

/// Open the quick search window (callable from frontend)
#[tauri::command]
fn open_search_window(app: tauri::AppHandle) {
    show_search_window(&app);
}

/// Get the log directory path
#[tauri::command]
fn get_log_dir(state: tauri::State<'_, AppState>) -> String {
    state.log_dir.to_string_lossy().to_string()
}

/// Export logs to a specified path (copies all log files)
#[tauri::command]
fn export_logs(state: tauri::State<'_, AppState>, output_path: String) -> Result<usize, String> {
    use std::fs;

    let log_dir = &state.log_dir;
    let output_dir = std::path::Path::new(&output_path);

    // Create output directory if needed
    fs::create_dir_all(output_dir).map_err(|e| format!("Failed to create output directory: {}", e))?;

    let mut copied = 0;

    // Copy all log files (files containing ".log" in name, e.g., prism.log.2026-01-06)
    if let Ok(entries) = fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Match files like "prism.log", "prism.log.2026-01-06", etc.
            if path.is_file() && file_name.contains(".log") {
                let dest = output_dir.join(file_name);
                if fs::copy(&path, &dest).is_ok() {
                    copied += 1;
                }
            }
        }
    }

    info!("Exported {} log files to {:?}", copied, output_path);
    Ok(copied)
}

/// Application state shared across all commands
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
    pub db_path: std::path::PathBuf,
    /// Path to log files directory
    pub log_dir: std::path::PathBuf,
    /// Flag to indicate if a scan is currently running
    pub is_scanning: Arc<AtomicBool>,
}

/// Initialize the Tauri application
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Set up log directory early (before tracing init)
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Prism")
        .join("logs");
    let _ = std::fs::create_dir_all(&log_dir);

    // Initialize tracing with file appender
    let file_appender = tracing_appender::rolling::daily(&log_dir, "prism.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "prism=debug,tauri=info".into()),
        )
        .with(tracing_subscriber::fmt::layer()) // Console output
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).with_ansi(false)) // File output
        .init();

    // Keep _guard alive for the entire app lifetime
    let log_dir_clone = log_dir.clone();

    info!("Starting Prism application");
    info!("Log directory: {:?}", log_dir);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            info!("Setting up application");

            // Get the app data directory for the database
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            // Create the directory if it doesn't exist
            std::fs::create_dir_all(&app_data_dir)?;

            // Store log_dir for later use
            let log_dir = log_dir_clone.clone();

            let db_path = app_data_dir.join("prism.db");
            info!("Database path: {:?}", db_path);

            // Initialize database
            let db = Database::new(&db_path)?;
            db.initialize_schema()?;

            info!("Database initialized successfully");

            // Store the database in app state
            app.manage(AppState {
                db: Arc::new(Mutex::new(db)),
                db_path: db_path.clone(),
                log_dir,
                is_scanning: Arc::new(AtomicBool::new(false)),
            });

            // Set up system tray
            let search = MenuItem::with_id(app, "search", "Quick Search (Ctrl+Space)", true, None::<&str>)?;
            let show = MenuItem::with_id(app, "show", "Show Prism", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&search, &show, &quit])?;

            // Load tray icon from bundled resources
            let icon = Image::from_path("icons/32x32.png")
                .unwrap_or_else(|_| Image::from_bytes(include_bytes!("../icons/32x32.png")).unwrap());

            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .tooltip("Prism - Disk Analyzer (Ctrl+Space to search)")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "search" => {
                        show_search_window(app);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        info!("Quit requested from tray menu");
                        // Try to optimize database but don't block exit if it fails
                        if let Some(state) = app.try_state::<AppState>() {
                            // Try non-blocking lock first
                            if let Ok(db) = state.db.try_lock() {
                                info!("Flushing database before exit...");
                                if let Err(e) = db.optimize() {
                                    info!("Database optimize on exit failed: {}", e);
                                }
                                drop(db);
                                info!("Database flushed successfully");
                            } else {
                                info!("Database locked, skipping optimization on exit");
                            }
                        }
                        // Force exit
                        std::process::exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            info!("System tray initialized");

            // Register global shortcut (Ctrl+Space)
            let shortcut = Shortcut::new(Some(Modifiers::CONTROL), Code::Space);
            let app_handle = app.handle().clone();

            if let Err(e) = app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, _event| {
                show_search_window(&app_handle);
            }) {
                info!("Failed to register global shortcut: {}", e);
            } else {
                info!("Global shortcut Ctrl+Space registered");
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Prevent the window from closing, hide it instead
                api.prevent_close();
                let _ = window.hide();
                info!("Window hidden to tray");
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Stats
            commands::stats::get_stats,
            commands::stats::get_recent_scans,
            // Scan
            commands::scan::start_scan,
            commands::scan::auto_scan_drives,
            commands::scan::start_incremental_scan,
            commands::scan::start_smart_scan,
            // Search
            commands::search::search_files,
            commands::search::search_files_advanced,
            commands::search::quick_search,
            // Duplicates
            commands::duplicates::find_duplicates,
            commands::duplicates::delete_duplicate,
            commands::duplicates::delete_duplicates_batch,
            commands::duplicates::find_similar_images,
            // Analytics
            commands::analytics::get_size_distribution,
            commands::analytics::get_extension_distribution,
            commands::analytics::get_folder_sizes,
            commands::analytics::get_file_type_distribution,
            commands::analytics::get_folder_contents,
            // Treemap (pre-computed folder sizes)
            commands::analytics::get_treemap_data,
            commands::analytics::get_folder_children,
            commands::analytics::rebuild_folder_sizes,
            // Tree view
            commands::tree::get_directory_tree,
            commands::tree::get_tree_children,
            // Drives
            commands::drives::get_available_drives,
            commands::drives::get_drive_stats,
            commands::drives::get_drives_status,
            commands::drives::remove_offline_drive,
            commands::drives::detect_drive_overlaps,
            commands::drives::check_pre_scan_overlap,
            // Export
            commands::export::export_to_json,
            commands::export::export_to_csv,
            // Utils
            commands::utils::clear_database,
            commands::utils::reset_app,
            commands::utils::open_in_explorer,
            // Verification
            commands::verify::verify_cross_disk,
            commands::verify::quick_backup_check,
            close_search_window,
            open_search_window,
            get_log_dir,
            export_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
