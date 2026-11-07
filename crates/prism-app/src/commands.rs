//! Tauri commands: thin wrappers over `prism-core`, taking catalog ids, never free paths
//! (ADR-0003, NF5). Errors reach the interface as a code plus a technical message.

use std::sync::Arc;

use prism_core::jobs::JobId;
use prism_core::model::{EntryId, VolumeId, VolumeLabels, VolumeStatus};
use prism_core::store;
use tauri::{AppHandle, Emitter, State};

use crate::cleanup_views::{CleanupResultView, CleanupView};
use crate::dto::{ApiError, AppInfo, JobView, LabelsInput, NativeTexts, Settings, VolumeView};
use crate::space_views::{LargeFileView, SpaceFolderView, TypeStatsView};
use crate::state::{AppState, IndexKind, SETTINGS_KEY, access_path, job_view};
use crate::tray;

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

/// F01-AC5: removes the catalog data of a volume; files on the disk are never touched.
#[tauri::command]
pub fn remove_volume(app: AppHandle, state: State<'_, Arc<AppState>>, id: i64) -> R<()> {
    let volume = VolumeId(id);
    if !state.engine.active_for(volume).is_empty() {
        return Err(ApiError::new("volume_busy", id));
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
pub fn set_settings(state: State<'_, Arc<AppState>>, settings: Settings) -> R<()> {
    let ok = matches!(settings.language.as_str(), "system" | "en" | "it")
        && matches!(settings.theme.as_str(), "system" | "light" | "dark");
    if !ok {
        return Err(ApiError::new("invalid_settings", format!("{settings:?}")));
    }
    let before = state.settings();
    let json =
        serde_json::to_string(&settings).map_err(|e| ApiError::new("invalid_settings", e))?;
    state
        .store
        .write(move |w| store::set_setting(w, SETTINGS_KEY, &json))?;
    if before.autostart != settings.autostart {
        state.apply_autostart(settings.autostart)?;
    }
    Ok(())
}

/// The tray menu and the notifications use texts sent by the interface, in the language it
/// shows (ARCHITECTURE §7).
#[tauri::command]
pub fn set_native_texts(app: AppHandle, state: State<'_, Arc<AppState>>, texts: NativeTexts) {
    tray::set_labels(&app, &texts.open, &texts.quit);
    state.notices().set_texts(texts);
}
