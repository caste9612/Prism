//! What the backup screen receives and sends (F13–F15): the folders to check, the summaries of
//! a check or a comparison, one level of the result tree at a time, the copy preview and its
//! outcome.

use prism_core::backup::copy::{CopyOutcome, CopyReport};
use prism_core::backup::{FolderView, Status, Tally};
use prism_core::names;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Files listed at most in one level of a result tree (the folders carry the totals).
pub const MAX_FILES: usize = 500;

/// A folder of a volume: its path relative to the volume root (`\`, empty for the whole disk).
#[derive(Clone, Debug, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SideInput {
    #[ts(type = "number")]
    pub volume_id: i64,
    pub folder: String,
}

impl SideInput {
    /// The path components, without empty ones.
    pub fn components(&self) -> Vec<String> {
        self.folder
            .split(['\\', '/'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TallyView {
    #[ts(type = "number")]
    pub files: u64,
    #[ts(type = "number")]
    pub bytes: u64,
}

impl From<Tally> for TallyView {
    fn from(t: Tally) -> Self {
        Self {
            files: t.files,
            bytes: t.bytes,
        }
    }
}

/// A backup check (F13): how many source files have a copy, verified how (F13-AC3).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VerifySummaryView {
    #[ts(type = "number")]
    pub report_id: u64,
    pub total: TallyView,
    pub by_content: TallyView,
    pub by_metadata: TallyView,
    pub missing: TallyView,
}

/// A comparison (F14): totals by status, in the order `onlyA`, `onlyB`, `different`, `same`.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CompareSummaryView {
    #[ts(type = "number")]
    pub report_id: u64,
    pub tallies: Vec<TallyView>,
}

/// Which items of a report to show.
#[derive(Clone, Debug, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReportFilterInput {
    /// Text the path must contain (any case).
    pub text: Option<String>,
    #[ts(type = "number")]
    pub min_size: u64,
    /// Statuses to keep in a comparison (`onlyA`...); all when empty.
    pub statuses: Vec<String>,
}

/// A subfolder in a level of a result tree, with totals by class (one class for a check, the
/// four statuses for a comparison).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReportFolderNodeView {
    /// Stored name: what to send back to open it.
    pub key: String,
    pub name: String,
    pub tallies: Vec<TallyView>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReportFileView {
    /// Relative path (stored names).
    pub rel: String,
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    /// `missing` for a check; `onlyA`, `onlyB`, `different`, `same` for a comparison.
    pub status: String,
    /// Path in B of a file moved or renamed (comparison by content).
    pub other: Option<String>,
    /// `content` or `metadata`: how identical or different was decided.
    pub evidence: Option<String>,
}

/// One level of a result tree (F13-AC4, F14-AC3).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReportFolderView {
    pub folders: Vec<ReportFolderNodeView>,
    pub files: Vec<ReportFileView>,
    /// Files of this level not listed (over [`MAX_FILES`]).
    pub more_files: u32,
    pub tallies: Vec<TallyView>,
}

/// Display form of a path in stored names.
pub fn display(rel: &str) -> String {
    rel.split('\\')
        .map(|p| names::decode(p).to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\\")
}

/// The view of a level; `file` builds the view of the item at a position of the filtered list.
pub fn folder_view(view: FolderView, file: impl Fn(usize) -> ReportFileView) -> ReportFolderView {
    let more = view.files.len().saturating_sub(MAX_FILES);
    ReportFolderView {
        folders: view
            .folders
            .into_iter()
            .map(|f| ReportFolderNodeView {
                name: display(&f.name),
                key: f.name,
                tallies: f.tallies.into_iter().map(Into::into).collect(),
            })
            .collect(),
        files: view.files.into_iter().take(MAX_FILES).map(file).collect(),
        more_files: u32::try_from(more).unwrap_or(u32::MAX),
        tallies: view.tallies.into_iter().map(Into::into).collect(),
    }
}

pub fn status_of(name: &str) -> Option<Status> {
    Status::ALL.into_iter().find(|s| s.as_str() == name)
}

/// What a copy will do (F15-AC2).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CopyPreviewView {
    #[ts(type = "number")]
    pub files: u64,
    #[ts(type = "number")]
    pub bytes: u64,
    /// Free space of the destination disk, when Windows tells it.
    #[ts(type = "number | null")]
    pub free_space: Option<u64>,
    pub enough_space: bool,
    /// Where the copies go: the folder structure of the source is recreated below it.
    pub dest_path: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CopyFailureView {
    pub path: String,
    pub error: String,
}

/// The outcome of a copy (F15-AC5).
#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CopyResultView {
    pub copied: u32,
    pub renamed: u32,
    /// Already at the destination with the same content (a resumed copy).
    pub already_there: u32,
    pub skipped: u32,
    pub failed: Vec<CopyFailureView>,
    #[ts(type = "number")]
    pub bytes: u64,
    /// Cancelled before the end: copying again resumes.
    pub interrupted: bool,
}

impl From<&CopyReport> for CopyResultView {
    fn from(r: &CopyReport) -> Self {
        let n = |k: &str| u32::try_from(r.count(k)).unwrap_or(u32::MAX);
        Self {
            copied: n("copied"),
            renamed: n("renamed"),
            already_there: n("already_there"),
            skipped: n("skipped_conflict"),
            failed: r
                .results
                .iter()
                .filter_map(|x| match &x.outcome {
                    CopyOutcome::Failed(e) => Some(CopyFailureView {
                        path: display(&x.rel),
                        error: e.clone(),
                    }),
                    _ => None,
                })
                .collect(),
            bytes: r.copied_bytes(),
            interrupted: r.interrupted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sides_and_statuses_from_the_interface() {
        let s = SideInput {
            volume_id: 1,
            folder: r" Foto \2019/ \".into(),
        };
        assert_eq!(s.components(), ["Foto", "2019"]);
        assert!(SideInput::default().components().is_empty());
        assert_eq!(status_of("onlyB"), Some(Status::OnlyB));
        assert_eq!(status_of("x"), None);
    }
}
