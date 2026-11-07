//! Guided cleanup for the interface (F09): types exchanged with the screen "Cleanup", the rules
//! of this user and machine, and the scan of the connected volumes. The rules and the deletion
//! live in `prism-core` (`cleanup`, `fileops`).

use std::path::Path;

use prism_core::cleanup::{
    self, BrowserCaches, CacheLayout, Category, CleanupItem, CleanupRules, DEFAULT_PROTECTED,
};
use prism_core::fileops::DeleteReport;
use prism_core::model::{FileTime, unix_millis_now};
use serde::Serialize;
use ts_rs::TS;

/// Items of a group sent to the interface; the totals always count all of them.
pub const MAX_ITEMS: usize = 2000;

/// One item proposed for cleanup.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CleanupItemView {
    #[ts(type = "number")]
    pub id: i64,
    /// Relative to the volume root.
    pub path: String,
    pub is_dir: bool,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub files: u64,
    #[ts(type = "number | null")]
    pub modified: Option<i64>,
}

impl From<CleanupItem> for CleanupItemView {
    fn from(i: CleanupItem) -> Self {
        Self {
            id: i.entry.0,
            path: i.path,
            is_dir: i.is_dir,
            size: i.size,
            files: i.files,
            modified: i.modified.map(|t| t.to_unix_millis()),
        }
    }
}

/// One category on one volume (F09-AC1).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CleanupGroupView {
    /// `temp`, `empty_folders`, `large_old` or `browser_cache`.
    pub category: String,
    #[ts(type = "number")]
    pub volume_id: i64,
    /// The browser of a cache group.
    pub owner: Option<String>,
    /// Set when the group cannot be cleaned now: the browser that is running (F09-AC4).
    pub blocked_by: Option<String>,
    /// No Recycle Bin on this volume: deleting is permanent and needs a confirmation
    /// (F16-AC2).
    pub permanent: bool,
    pub items: Vec<CleanupItemView>,
    #[ts(type = "number")]
    pub count: u64,
    #[ts(type = "number")]
    pub total: u64,
}

/// What the Recycle Bin of a volume holds (shown for information, F09-AC1).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecycleBinView {
    #[ts(type = "number")]
    pub volume_id: i64,
    #[ts(type = "number")]
    pub items: u64,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// The result of a cleanup scan.
#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CleanupView {
    pub groups: Vec<CleanupGroupView>,
    pub recycle_bins: Vec<RecycleBinView>,
}

/// What a deletion did (F16-AC6 has the details in the actions log).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CleanupResultView {
    pub done: u32,
    pub skipped: u32,
    pub failed: u32,
    #[ts(type = "number")]
    pub freed: u64,
}

impl From<&DeleteReport> for CleanupResultView {
    fn from(r: &DeleteReport) -> Self {
        let n = |k: &str| u32::try_from(r.count(k)).unwrap_or(u32::MAX);
        Self {
            done: n("done"),
            skipped: n("skipped_changed") + n("skipped_missing"),
            failed: n("failed"),
            freed: r.freed(),
        }
    }
}

/// A group of items, capped for the interface.
pub fn group(
    category: Category,
    volume_id: i64,
    items: Vec<CleanupItem>,
    owner: Option<String>,
    blocked_by: Option<String>,
    permanent: bool,
) -> Option<CleanupGroupView> {
    if items.is_empty() {
        return None;
    }
    let count = items.len() as u64;
    let total = cleanup::total(&items);
    Some(CleanupGroupView {
        category: category.as_str().to_owned(),
        volume_id,
        owner,
        blocked_by,
        permanent,
        items: items.into_iter().take(MAX_ITEMS).map(Into::into).collect(),
        count,
        total,
    })
}

const DAY: i64 = 86_400_000;

/// The cleanup rules for the volume reached at `mount`, from the user's environment: where
/// the temporary folders and the browsers' data are on this machine.
pub fn rules_for(mount: &Path) -> CleanupRules {
    let mount = mount.to_string_lossy();
    let env = |var: &str| std::env::var(var).ok();
    let under = |abs: Option<String>| abs.and_then(|a| cleanup::relative_under(&mount, &a));
    let local = env("LOCALAPPDATA");
    let browser = |name: &str, process: &str, parts: &[&str], layout| {
        let folder = local.as_ref().map(|l| {
            let mut p = std::path::PathBuf::from(l);
            p.extend(parts);
            p.to_string_lossy().into_owned()
        });
        under(folder).map(|folder| BrowserCaches {
            browser: name.to_owned(),
            process: process.to_owned(),
            folder,
            layout,
        })
    };
    let system_temp =
        env("SystemRoot").map(|r| Path::new(&r).join("Temp").to_string_lossy().into_owned());
    let now = unix_millis_now();
    CleanupRules {
        temp_dirs: [under(env("TEMP")), under(system_temp)]
            .into_iter()
            .flatten()
            .filter(|d| !d.is_empty())
            .collect(),
        temp_before: FileTime::from_unix_millis(now - DAY),
        large_min_size: 500_000_000,
        large_before: FileTime::from_unix_millis(now - 365 * DAY),
        browsers: [
            browser(
                "Chrome",
                "chrome.exe",
                &["Google", "Chrome", "User Data"],
                CacheLayout::Chromium,
            ),
            browser(
                "Edge",
                "msedge.exe",
                &["Microsoft", "Edge", "User Data"],
                CacheLayout::Chromium,
            ),
            browser(
                "Brave",
                "brave.exe",
                &["BraveSoftware", "Brave-Browser", "User Data"],
                CacheLayout::Chromium,
            ),
            browser(
                "Firefox",
                "firefox.exe",
                &["Mozilla", "Firefox", "Profiles"],
                CacheLayout::Firefox,
            ),
        ]
        .into_iter()
        .flatten()
        .collect(),
        protected: DEFAULT_PROTECTED.iter().map(|s| (*s).to_owned()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The rules of the system volume point at this user's temporary folders and browser data,
    // relative to the volume root; another volume gets no temporary folder.
    #[test]
    fn rules_from_the_environment() {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let rules = rules_for(Path::new(&format!("{system}\\")));
        assert!(
            rules
                .temp_dirs
                .iter()
                .any(|d| d.eq_ignore_ascii_case(r"Windows\Temp")),
            "{:?}",
            rules.temp_dirs
        );
        assert!(rules.temp_dirs.iter().all(|d| !d.contains(':')));
        assert!(rules.browsers.iter().all(|b| b.folder.starts_with("Users")));
        assert!(rules.protected.iter().any(|p| p == "Windows"));
        let other = rules_for(Path::new(r"\\nas\share"));
        assert!(other.temp_dirs.is_empty() && other.browsers.is_empty());
    }
}
