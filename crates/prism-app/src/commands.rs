//! Tauri commands: thin wrappers over `prism-core`, taking catalog ids, never free paths
//! (ADR-0003, NF5). Errors reach the interface as a code plus a technical message.

use std::sync::Arc;

use prism_core::jobs::JobId;
use prism_core::model::{EntryId, VolumeId, VolumeLabels, VolumeStatus};
use prism_core::store;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::backup_views::{
    CompareSummaryView, CopyPreviewView, CopyResultView, ReportFilterInput, ReportFolderView,
    SideInput, VerifySummaryView,
};
use crate::cleanup_views::{CleanupResultView, CleanupView};
use crate::diagnostics_state::DiagnosticsView;
use crate::dto::{ApiError, AppInfo, JobView, LabelsInput, NativeTexts, Settings, VolumeView};
use crate::duplicates_views::{DupFilterInput, DuplicatesView, RuleInput};
use crate::health_state::DiskHealthView;
use crate::identity::AmbiguousView;
use crate::nas_state::{NasHealthView, NasServerView};
use crate::search_views::SearchResultView;
use crate::shares::SharePathView;
use crate::shell_requests::LocatedView;
use crate::similar_views::SimilarView;
use crate::space_views::{LargeFileView, SpaceFolderView, TypeStatsView};
use crate::state::{AppState, IndexKind, access_path, job_view};
use crate::updates::{UpdateState, UpdateView};
use crate::{search_window, tray};

type R<T> = Result<T, ApiError>;

fn emit_volumes(app: &AppHandle) {
    let _ = app.emit("volumes-changed", ());
}

#[tauri::command]
pub fn app_info(state: State<'_, Arc<AppState>>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        elevated: state.elevated,
        data_dir: state.data_dir.display().to_string(),
        dev: state.dev,
        shortcut_error: state.shortcut_error().clone(),
    }
}

#[tauri::command]
pub fn get_volumes(state: State<'_, Arc<AppState>>) -> R<Vec<VolumeView>> {
    state.volume_views()
}

#[tauri::command]
pub fn get_jobs(state: State<'_, Arc<AppState>>) -> Vec<JobView> {
    state.engine.jobs().iter().map(job_view).collect()
}

/// Indexes a volume (F02-AC1 "Index"), or updates it when already indexed.
#[tauri::command]
pub fn index_volume(app: AppHandle, state: State<'_, Arc<AppState>>, id: i64) -> R<u64> {
    let volume = VolumeId(id);
    let status = state
        .store
        .read(|c| store::volume(c, volume))?
        .map(|v| v.status)
        .ok_or_else(|| ApiError::new("volume_not_found", id))?;
    let kind = if status == VolumeStatus::Indexed {
        IndexKind::Update
    } else {
        IndexKind::Index
    };
    let job = state.submit_index(volume, kind)?;
    emit_volumes(&app);
    Ok(job.0)
}

#[tauri::command]
pub fn cancel_job(state: State<'_, Arc<AppState>>, id: u64) -> bool {
    state.engine.cancel(JobId(id))
}

/// F03: name, physical location, notes, colour.
#[tauri::command]
pub fn set_labels(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: i64,
    labels: LabelsInput,
) -> R<()> {
    let clean = |s: Option<String>| s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let labels = VolumeLabels {
        name: clean(labels.name),
        location: clean(labels.location),
        notes: clean(labels.notes),
        color: clean(labels.color),
    };
    state
        .store
        .write(move |w| store::set_volume_labels(w, VolumeId(id), &labels))?;
    emit_volumes(&app);
    Ok(())
}

/// Checks the new network path of a share of the catalog (REVIEW D59).
#[tauri::command]
pub async fn check_share_path(
    state: State<'_, Arc<AppState>>,
    id: i64,
    path: String,
) -> R<SharePathView> {
    let s = Arc::clone(&state);
    blocking(move || s.check_share_path(VolumeId(id), &path)).await
}

/// Gives a share of the catalog its new network path, after the user confirmed it (REVIEW D59).
#[tauri::command]
pub async fn move_share(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: i64,
    path: String,
) -> R<()> {
    let s = Arc::clone(&state);
    blocking(move || s.move_share(VolumeId(id), &path)).await?;
    emit_volumes(&app);
    Ok(())
}

/// F02-AC4: "Always ignore" (and its undo from the settings).
#[tauri::command]
pub fn set_ignored(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: i64,
    ignored: bool,
) -> R<()> {
    let volume = VolumeId(id);
    let current = state
        .store
        .read(|c| store::volume(c, volume))?
        .ok_or_else(|| ApiError::new("volume_not_found", id))?;
    let indexed = current.last_indexed_at.is_some();
    let status = match (ignored, indexed) {
        (true, _) => VolumeStatus::Ignored,
        (false, true) => VolumeStatus::ReindexNeeded,
        (false, false) => VolumeStatus::New,
    };
    state
        .store
        .write(move |w| store::set_volume_status(w, volume, status))?;
    emit_volumes(&app);
    Ok(())
}

/// Opens the root of a connected volume in Explorer, with the user's normal rights (ADR-0003).
#[tauri::command]
pub fn open_volume(state: State<'_, Arc<AppState>>, id: i64) -> R<()> {
    let detected = state
        .online()
        .get(&VolumeId(id))
        .cloned()
        .ok_or_else(|| ApiError::new("volume_offline", id))?;
    let root = access_path(&detected).ok_or_else(|| ApiError::new("volume_offline", id))?;
    prism_win::shell::open(&root).map_err(|e| ApiError::new("open_failed", e))
}

/// Runs a catalog read away from the main thread and the async runtime: the space queries can
/// take a few hundred milliseconds on large volumes (NF1).
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> R<T> + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::new("unknown", e))?
}

/// A folder of a volume (its root if `folder_id` is missing) with its treemap (F08-AC1/AC2).
#[tauri::command]
pub async fn space_folder(
    state: State<'_, Arc<AppState>>,
    volume_id: i64,
    folder_id: Option<i64>,
) -> R<SpaceFolderView> {
    let s = Arc::clone(&state);
    blocking(move || s.space_folder(VolumeId(volume_id), folder_id.map(EntryId))).await
}

/// The largest files below a folder (F08-AC2).
#[tauri::command]
pub async fn largest_files(
    state: State<'_, Arc<AppState>>,
    volume_id: i64,
    folder_id: Option<i64>,
    limit: usize,
) -> R<Vec<LargeFileView>> {
    let s = Arc::clone(&state);
    blocking(move || s.largest_files(VolumeId(volume_id), folder_id.map(EntryId), limit)).await
}

/// Statistics by type of a volume, or of the whole catalog without `volume_id` (F08-AC3).
#[tauri::command]
pub async fn type_stats(
    state: State<'_, Arc<AppState>>,
    volume_id: Option<i64>,
) -> R<TypeStatsView> {
    let s = Arc::clone(&state);
    blocking(move || s.type_stats(volume_id.map(VolumeId))).await
}

/// The most recent file operations of the actions log (F16-AC6).
#[tauri::command]
pub async fn recent_actions(
    state: State<'_, Arc<AppState>>,
    limit: usize,
) -> R<Vec<crate::dto::ActionView>> {
    let s = Arc::clone(&state);
    blocking(move || s.recent_actions(limit)).await
}

/// Opens a file or folder of the catalog with the user's normal rights (ADR-0003).
#[tauri::command]
pub fn open_entry(state: State<'_, Arc<AppState>>, volume_id: i64, entry_id: i64) -> R<()> {
    let path = state.entry_path(VolumeId(volume_id), EntryId(entry_id))?;
    prism_win::shell::open(&path).map_err(|e| ApiError::new("open_failed", e))
}

/// Shows a file or folder of the catalog selected in Explorer (ADR-0003).
#[tauri::command]
pub fn reveal_entry(state: State<'_, Arc<AppState>>, volume_id: i64, entry_id: i64) -> R<()> {
    let path = state.entry_path(VolumeId(volume_id), EntryId(entry_id))?;
    prism_win::shell::reveal(&path).map_err(|e| ApiError::new("open_failed", e))
}

/// Searches the catalog of every disk, online and offline (F07). `utc_offset_min` is the
/// user's time zone, for the dates typed in the filters.
#[tauri::command]
pub async fn search(
    state: State<'_, Arc<AppState>>,
    text: String,
    utc_offset_min: i32,
) -> R<SearchResultView> {
    let s = Arc::clone(&state);
    blocking(move || s.search(&text, utc_offset_min)).await
}

/// Hides the search window (Esc, or after opening a result).
#[tauri::command]
pub fn hide_search(app: AppHandle) {
    if let Some(w) = app.get_webview_window(search_window::LABEL) {
        let _ = w.hide();
    }
}

/// What the guided cleanup proposes on the connected disks (F09).
#[tauri::command]
pub async fn cleanup_scan(state: State<'_, Arc<AppState>>) -> R<CleanupView> {
    let s = Arc::clone(&state);
    blocking(move || s.cleanup_scan()).await
}

/// Deletes the chosen entries of a volume through the safe deletion path (ADR-0012, F16).
#[tauri::command]
pub async fn delete_entries(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    volume_id: i64,
    entry_ids: Vec<i64>,
    empty_folders: bool,
    permanent: bool,
) -> R<CleanupResultView> {
    let s = Arc::clone(&state);
    let entries: Vec<EntryId> = entry_ids.into_iter().map(EntryId).collect();
    let result =
        blocking(move || s.delete_entries(VolumeId(volume_id), &entries, empty_folders, permanent))
            .await;
    emit_volumes(&app);
    result
}

/// Groups of identical files on every disk, with the copies the rules propose (F10).
#[tauri::command]
pub async fn duplicate_groups(
    state: State<'_, Arc<AppState>>,
    filter: DupFilterInput,
    rules: Vec<RuleInput>,
) -> R<DuplicatesView> {
    let s = Arc::clone(&state);
    blocking(move || s.duplicates(&filter, &rules)).await
}

/// Deletes copies of duplicates on the connected disks (F16-AC3/AC4).
#[tauri::command]
pub async fn delete_duplicates(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    entry_ids: Vec<i64>,
    allow_offline_only: bool,
    permanent: bool,
) -> R<CleanupResultView> {
    let s = Arc::clone(&state);
    let entries: Vec<EntryId> = entry_ids.into_iter().map(EntryId).collect();
    let result =
        blocking(move || s.delete_duplicates(&entries, allow_offline_only, permanent)).await;
    emit_volumes(&app);
    result
}

/// Which files of a folder have no copy in the destinations (F13), also with disks offline.
#[tauri::command]
pub async fn backup_verify(
    state: State<'_, Arc<AppState>>,
    source: SideInput,
    destinations: Vec<SideInput>,
) -> R<VerifySummaryView> {
    let s = Arc::clone(&state);
    blocking(move || s.backup_verify(&source, &destinations)).await
}

/// Two folders side by side, by path or by content (F14).
#[tauri::command]
pub async fn backup_compare(
    state: State<'_, Arc<AppState>>,
    a: SideInput,
    b: SideInput,
    by_content: bool,
) -> R<CompareSummaryView> {
    let s = Arc::clone(&state);
    blocking(move || s.backup_compare(&a, &b, by_content)).await
}

/// One level of the result tree of a check or a comparison (F13-AC4, F14-AC3).
#[tauri::command]
pub async fn backup_folder(
    state: State<'_, Arc<AppState>>,
    report_id: u64,
    folder: String,
    filter: ReportFilterInput,
) -> R<ReportFolderView> {
    let s = Arc::clone(&state);
    blocking(move || s.backup_folder(report_id, &folder, &filter)).await
}

/// Files, size and free space before copying the missing files (F15-AC1/AC2).
#[tauri::command]
pub async fn copy_preview(
    state: State<'_, Arc<AppState>>,
    report_id: u64,
    dest: SideInput,
) -> R<CopyPreviewView> {
    let s = Arc::clone(&state);
    blocking(move || s.copy_preview(report_id, &dest)).await
}

/// Starts copying the missing files of a check (F15); returns the job.
#[tauri::command]
pub fn copy_start(
    state: State<'_, Arc<AppState>>,
    report_id: u64,
    dest: SideInput,
    rename_on_conflict: bool,
) -> R<u64> {
    Ok(state.copy_start(report_id, &dest, rename_on_conflict)?.0)
}

/// The outcome of a finished copy job (F15-AC5).
#[tauri::command]
pub fn copy_result(state: State<'_, Arc<AppState>>, job_id: u64) -> Option<CopyResultView> {
    state.copy_result(job_id)
}

/// Groups of similar images on every disk (F11), optionally with a copy on one volume.
#[tauri::command]
pub async fn similar_images(
    state: State<'_, Arc<AppState>>,
    max_distance: u32,
    volume_id: Option<i64>,
) -> R<SimilarView> {
    let s = Arc::clone(&state);
    blocking(move || s.similar_images(max_distance, volume_id.map(VolumeId))).await
}

/// Groups of similar videos on every disk (F12), optionally with a copy on one volume.
#[tauri::command]
pub async fn similar_videos(
    state: State<'_, Arc<AppState>>,
    max_distance: u32,
    volume_id: Option<i64>,
) -> R<SimilarView> {
    let s = Arc::clone(&state);
    blocking(move || s.similar_videos(max_distance, volume_id.map(VolumeId))).await
}

/// A JPEG thumbnail of the middle frame of a connected video, as raw bytes (F12).
#[tauri::command]
pub async fn video_thumbnail(
    state: State<'_, Arc<AppState>>,
    volume_id: i64,
    entry_id: i64,
) -> R<tauri::ipc::Response> {
    let s = Arc::clone(&state);
    let jpeg = blocking(move || s.video_thumbnail(VolumeId(volume_id), EntryId(entry_id))).await?;
    Ok(tauri::ipc::Response::new(jpeg))
}

/// A JPEG thumbnail of a connected image, as raw bytes (F11-AC3).
#[tauri::command]
pub async fn image_thumbnail(
    state: State<'_, Arc<AppState>>,
    volume_id: i64,
    entry_id: i64,
) -> R<tauri::ipc::Response> {
    let s = Arc::clone(&state);
    let jpeg = blocking(move || s.image_thumbnail(VolumeId(volume_id), EntryId(entry_id))).await?;
    Ok(tauri::ipc::Response::new(jpeg))
}

/// What the settings show about updates (F22-AC3).
#[tauri::command]
pub fn update_status(updates: State<'_, Arc<UpdateState>>) -> UpdateView {
    updates.snapshot()
}

/// Checks for an update now.
#[tauri::command]
pub async fn check_updates(app: AppHandle, updates: State<'_, Arc<UpdateState>>) -> R<UpdateView> {
    crate::updates::check_now(&app, &updates).await?;
    Ok(updates.snapshot())
}

/// Installs the downloaded update after the user confirmed it; Prism restarts.
#[tauri::command]
pub fn install_update(app: AppHandle, updates: State<'_, Arc<UpdateState>>) -> R<()> {
    crate::updates::install(&app, &updates)
}

/// The catalog folder of a path on a connected disk, for the Explorer menu (F18-AC1).
#[tauri::command]
pub async fn locate_path(state: State<'_, Arc<AppState>>, path: String) -> R<LocatedView> {
    let s = Arc::clone(&state);
    blocking(move || s.locate_path(&path)).await
}

/// The health of the disk of a connected volume, with its history (F17).
#[tauri::command]
pub async fn disk_health(
    state: State<'_, Arc<AppState>>,
    volume_id: i64,
) -> R<Option<DiskHealthView>> {
    let s = Arc::clone(&state);
    blocking(move || s.disk_health(VolumeId(volume_id))).await
}

/// Adds a network folder to the catalog by its path, after checking that it answers (F02-AC5).
#[tauri::command]
pub async fn add_share(app: AppHandle, state: State<'_, Arc<AppState>>, path: String) -> R<i64> {
    let s = Arc::clone(&state);
    let id = blocking(move || s.add_share(&path)).await?;
    emit_volumes(&app);
    Ok(id.0)
}

/// The NAS of the catalog and whether Prism has their login (REVIEW D66).
#[tauri::command]
pub async fn nas_servers(state: State<'_, Arc<AppState>>) -> R<Vec<NasServerView>> {
    let s = Arc::clone(&state);
    blocking(move || s.nas_servers()).await
}

/// Reads a NAS with the login the user typed and, when it works, saves the login in the Windows
/// Credential Manager (REVIEW D66).
#[tauri::command]
pub async fn nas_connect(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    server: String,
    user: String,
    password: String,
) -> R<()> {
    let s = Arc::clone(&state);
    blocking(move || s.nas_connect(&server, &user, &password)).await?;
    emit_volumes(&app);
    Ok(())
}

/// Forgets the login of a NAS and what was read from it.
#[tauri::command]
pub async fn nas_disconnect(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    server: String,
) -> R<()> {
    let s = Arc::clone(&state);
    blocking(move || s.nas_disconnect(&server)).await?;
    emit_volumes(&app);
    Ok(())
}

/// The array and the disks of the NAS behind a network share of the catalog.
#[tauri::command]
pub async fn nas_health(
    state: State<'_, Arc<AppState>>,
    volume_id: i64,
) -> R<Option<NasHealthView>> {
    let s = Arc::clone(&state);
    blocking(move || s.nas_health(VolumeId(volume_id))).await
}

/// Connected disks Prism cannot tell apart, waiting for the user's choice (F02-AC3).
#[tauri::command]
pub fn ambiguous_volumes(state: State<'_, Arc<AppState>>) -> R<Vec<AmbiguousView>> {
    state.ambiguous_volumes()
}

/// The user says which catalog disk an ambiguous disk is, or that it is a new one (`None`).
#[tauri::command]
pub fn resolve_ambiguous(
    state: State<'_, Arc<AppState>>,
    key: String,
    volume_id: Option<i64>,
) -> R<()> {
    state.resolve_ambiguous(&key, volume_id.map(VolumeId))
}

/// Empties the catalog after the user confirmed it (F21-AC4); returns the disks removed.
#[tauri::command]
pub async fn reset_catalog(app: AppHandle, state: State<'_, Arc<AppState>>) -> R<usize> {
    let s = Arc::clone(&state);
    let removed = blocking(move || s.reset_catalog()).await;
    emit_volumes(&app);
    let _ = app.emit("settings-changed", ());
    removed
}

/// Opens the folder of the log files in Explorer (F20-AC2).
#[tauri::command]
pub fn open_logs(state: State<'_, Arc<AppState>>) -> R<()> {
    let dir = state.logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| ApiError::new("open_failed", e))?;
    prism_win::shell::open(&dir).map_err(|e| ApiError::new("open_failed", e))
}

/// What the diagnostic package will contain, before exporting it (F20-AC2).
#[tauri::command]
pub async fn diagnostics_preview(state: State<'_, Arc<AppState>>) -> R<DiagnosticsView> {
    let s = Arc::clone(&state);
    blocking(move || s.diagnostics_preview()).await
}

/// Writes the diagnostic package and shows it in Explorer; returns its path (F20-AC2).
#[tauri::command]
pub async fn export_diagnostics(state: State<'_, Arc<AppState>>) -> R<String> {
    let s = Arc::clone(&state);
    let path = blocking(move || s.export_diagnostics()).await?;
    let _ = prism_win::shell::reveal(&path);
    Ok(path.to_string_lossy().into_owned())
}

/// F01-AC5: removes the catalog data of a volume; files on the disk are never touched.
#[tauri::command]
pub fn remove_volume(app: AppHandle, state: State<'_, Arc<AppState>>, id: i64) -> R<()> {
    let volume = VolumeId(id);
    if !state.foreground_jobs(volume).is_empty() {
        return Err(ApiError::new("volume_busy", id));
    }
    // Hashes in progress stop; what they would still save is dropped with the entries.
    for job in state.engine.active_for(volume) {
        state.engine.cancel(job.id);
    }
    state
        .store
        .write(move |w| store::remove_volume(w, volume))?;
    state.online().remove(&volume);
    emit_volumes(&app);
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: State<'_, Arc<AppState>>, settings: Settings) -> R<()> {
    let ok = matches!(settings.language.as_str(), "system" | "en" | "it")
        && matches!(settings.theme.as_str(), "system" | "light" | "dark");
    if !ok {
        return Err(ApiError::new("invalid_settings", format!("{settings:?}")));
    }
    let before = state.settings();
    // F07-AC1: a new shortcut must be registrable; otherwise the previous one stays.
    if before.search_shortcut != settings.search_shortcut || state.shortcut_error().is_some() {
        if let Some(error) = search_window::register(&app, &settings.search_shortcut) {
            let previous = search_window::register(&app, &before.search_shortcut);
            *state.shortcut_error() = previous;
            return Err(ApiError::new("shortcut_unavailable", error));
        }
        *state.shortcut_error() = None;
    }
    state.save_settings(&settings)?;
    state.apply_hashing(&settings);
    tray::set_hashing_paused(&app, settings.hashing_paused);
    if before.autostart != settings.autostart {
        state.apply_autostart(settings.autostart)?;
    }
    Ok(())
}

/// The tray menu and the notifications use texts sent by the interface, in the language it
/// shows (ARCHITECTURE §7).
#[tauri::command]
pub fn set_native_texts(app: AppHandle, state: State<'_, Arc<AppState>>, texts: NativeTexts) {
    tray::set_labels(&app, &texts, state.settings().hashing_paused);
    state.notices().set_texts(texts);
}
