//! Types exchanged with the interface. TypeScript definitions are generated from these by
//! `ts-rs` into `ui/src/lib/bindings/` when the tests run (`cargo test -p prism-app`), so the
//! two sides never drift apart (ADR-0014).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A volume of the catalog as shown on the catalog screen (F01).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VolumeView {
    #[ts(type = "number")]
    pub id: i64,
    /// User name, or Windows label, or mount.
    pub name: String,
    pub user_name: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub color: Option<String>,
    pub os_label: Option<String>,
    /// `internal`, `external`, `stick`, `network` or `folder` (development volumes).
    pub kind: String,
    pub fs_type: Option<String>,
    /// Current mount when online, last known one otherwise.
    pub mount: Option<String>,
    pub online: bool,
    /// `new`, `indexing`, `indexed`, `ignored`, `reindex_needed`.
    pub status: String,
    #[ts(type = "number | null")]
    pub capacity: Option<u64>,
    #[ts(type = "number | null")]
    pub free_space: Option<u64>,
    /// Logical size of the indexed files.
    #[ts(type = "number")]
    pub indexed_size: u64,
    #[ts(type = "number")]
    pub file_count: u64,
    #[ts(type = "number")]
    pub folder_count: u64,
    #[ts(type = "number | null")]
    pub last_indexed_at: Option<i64>,
    #[ts(type = "number | null")]
    pub last_seen_at: Option<i64>,
    /// The running or queued job of this volume, if any (F01-AC3).
    pub job: Option<JobView>,
    /// Content hashes of the volume, once counted (F06-AC5).
    pub hashes: Option<HashesView>,
}

/// How far the content hashes of a volume are (F06-AC5).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HashesView {
    /// Files with content.
    #[ts(type = "number")]
    pub total: u64,
    /// Of which with a valid hash.
    #[ts(type = "number")]
    pub hashed: u64,
    /// A hash job runs on the volume now.
    pub running: bool,
}

/// A job of the engine (ADR-0009).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobView {
    #[ts(type = "number")]
    pub id: u64,
    /// `index`, `update`.
    pub kind: String,
    #[ts(type = "number | null")]
    pub volume_id: Option<i64>,
    /// `queued`, `running`, `done`, `failed`, `cancelled`.
    pub state: String,
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number")]
    pub total: u64,
    /// `scanning`, `writing`...
    pub phase: String,
    pub error: Option<String>,
}

/// Texts shown by Windows outside the window (tray menu, notifications), in the language of
/// the interface, which sends them (ARCHITECTURE §7). `{name}` and `{count}` are filled in by
/// the backend.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NativeTexts {
    pub open: String,
    pub search: String,
    pub quit: String,
    pub new_disk_title: String,
    pub new_disks_title: String,
    pub new_disk_body: String,
    #[serde(default)]
    pub pause_hashing: String,
    #[serde(default)]
    pub resume_hashing: String,
    /// Tray status line (F19-AC2): no job, or `{count}` jobs.
    #[serde(default)]
    pub jobs_idle: String,
    #[serde(default)]
    pub jobs_running: String,
}

/// User labels of a volume (F03).
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LabelsInput {
    pub name: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub color: Option<String>,
}

/// User settings (F21).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Settings {
    /// `system`, `en`, `it`.
    pub language: String,
    /// `system`, `light`, `dark`.
    pub theme: String,
    /// Start with Windows (F19-AC3).
    pub autostart: bool,
    /// Global shortcut of the search window (F07-AC1), e.g. `Ctrl+Space`.
    #[serde(default = "default_search_shortcut")]
    pub search_shortcut: String,
    /// Background hashing paused by the user, from the tray or here (F06-AC5, F19-AC2).
    #[serde(default)]
    pub hashing_paused: bool,
    /// Pause hashing while the PC runs on battery (F21-AC3); off by default.
    #[serde(default)]
    pub pause_hashing_on_battery: bool,
    /// Volumes whose content is never analysed (F21-AC3).
    #[serde(default)]
    #[ts(type = "number[]")]
    pub hashing_excluded: Vec<i64>,
}

pub fn default_search_shortcut() -> String {
    "Ctrl+Space".to_owned()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "system".into(),
            theme: "system".into(),
            autostart: true,
            search_shortcut: default_search_shortcut(),
            hashing_paused: false,
            pause_hashing_on_battery: false,
            hashing_excluded: Vec::new(),
        }
    }
}

/// About the running application.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    /// Running with administrator rights (MFT and journal available).
    pub elevated: bool,
    pub data_dir: String,
    pub dev: bool,
    /// Why the search shortcut could not be registered (F07-AC1), if it could not.
    pub shortcut_error: Option<String>,
}

/// Errors as the interface receives them: a stable code (translated by the UI) and a
/// technical message for the logs (F20-AC3).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_string(),
        }
    }
}

impl From<prism_core::store::StoreError> for ApiError {
    fn from(e: prism_core::store::StoreError) -> Self {
        match e {
            // An id that is not (or no longer) in the catalog, e.g. a folder removed by an
            // update while its treemap was open.
            prism_core::store::StoreError::NotFound(_) => Self::new("not_found", e),
            _ => Self::new("store", e),
        }
    }
}
