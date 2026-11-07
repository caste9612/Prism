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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "system".into(),
            theme: "system".into(),
            autostart: true,
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
        Self::new("store", e)
    }
}
