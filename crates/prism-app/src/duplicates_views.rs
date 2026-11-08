//! What the duplicates screen receives and sends (F10, F16): groups with their copies on every
//! disk, the copies the selection rules propose, and the rules themselves.

use std::collections::HashMap;

use prism_core::duplicates::{DupFilter, DupGroup, DupResult, Rule};
use prism_core::model::{Volume, VolumeId, VolumeKind};
use prism_core::space::{self, FileCategory};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::search_views::disk_name;

/// Groups sent at most (largest recoverable space first); the totals count all of them.
pub const MAX_GROUPS: usize = 300;

/// The filters of the screen (F10-AC5).
#[derive(Clone, Debug, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DupFilterInput {
    #[ts(type = "number")]
    pub min_size: u64,
    /// A file category (`images`, `videos`...), or none for all.
    pub category: Option<String>,
    #[ts(type = "number | null")]
    pub volume_id: Option<i64>,
    /// Text the path of a copy must contain.
    pub folder: Option<String>,
}

impl DupFilterInput {
    pub fn to_filter(&self) -> DupFilter {
        DupFilter {
            min_size: self.min_size,
            category: self
                .category
                .as_deref()
                .and_then(|c| FileCategory::ALL.into_iter().find(|k| k.as_str() == c)),
            volume: self.volume_id.map(VolumeId),
            folder: self
                .folder
                .as_deref()
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .map(str::to_owned),
        }
    }
}

/// A selection rule (F10-AC3): `keepOnVolume` (with `volumeId`), `keepOldest`, `keepNewest`,
/// `keepShortestPath`, `neverIn` (with `text`).
#[derive(Clone, Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RuleInput {
    pub kind: String,
    #[ts(type = "number | null")]
    pub volume_id: Option<i64>,
    pub text: Option<String>,
}

/// The rules in the order given; unknown or incomplete ones are left out.
pub fn rules(input: &[RuleInput]) -> Vec<Rule> {
    input
        .iter()
        .filter_map(|r| match r.kind.as_str() {
            "keepOnVolume" => r.volume_id.map(|v| Rule::KeepOnVolume(VolumeId(v))),
            "keepOldest" => Some(Rule::KeepOldest),
            "keepNewest" => Some(Rule::KeepNewest),
            "keepShortestPath" => Some(Rule::KeepShortestPath),
            "neverIn" => r
                .text
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(|t| Rule::NeverIn(t.to_owned())),
            _ => None,
        })
        .collect()
}

/// One copy of a content.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DupFileView {
    #[ts(type = "number")]
    pub entry_id: i64,
    #[ts(type = "number")]
    pub volume_id: i64,
    /// Name of the disk and where it is kept (F10-AC1).
    pub disk: String,
    pub location: Option<String>,
    /// Path relative to the volume root, name included.
    pub path: String,
    /// Full path through the current mount, or the last one known.
    pub full_path: String,
    #[ts(type = "number | null")]
    pub modified: Option<i64>,
    pub online: bool,
    /// No Recycle Bin on the disk: deleting is permanent (F16-AC2).
    pub permanent: bool,
}

/// Files with the same content.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DupGroupView {
    /// Stable key of the group in the list (size and start of the hash).
    pub key: String,
    #[ts(type = "number")]
    pub size: u64,
    /// Space freed by keeping one copy (F10-AC2).
    #[ts(type = "number")]
    pub recoverable: u64,
    /// File category of the content.
    pub kind: String,
    pub files: Vec<DupFileView>,
    /// The copies the rules propose to delete (never all, never offline ones).
    #[ts(type = "number[]")]
    pub selected: Vec<i64>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DuplicatesView {
    pub groups: Vec<DupGroupView>,
    #[ts(type = "number")]
    pub total_groups: u64,
    #[ts(type = "number")]
    pub total_recoverable: u64,
    /// Files still without a content hash on the indexed disks: the list may grow (F06-AC5).
    #[ts(type = "number")]
    pub pending_files: u64,
}

fn file_view(
    f: &prism_core::duplicates::DupFile,
    volumes: &HashMap<VolumeId, (Volume, Option<String>)>,
) -> DupFileView {
    let (disk, location, mount, permanent) = volumes.get(&f.volume).map_or_else(
        || (format!("#{}", f.volume.0), None, String::new(), true),
        |(v, mount)| {
            (
                disk_name(v),
                v.labels.location.clone().filter(|l| !l.trim().is_empty()),
                mount
                    .clone()
                    .or_else(|| v.info.unc_path.clone())
                    .or_else(|| v.info.last_mount.clone())
                    .unwrap_or_default(),
                v.info.kind != VolumeKind::Fixed,
            )
        },
    );
    let full_path = if mount.is_empty() {
        f.path.clone()
    } else {
        format!("{}\\{}", mount.trim_end_matches('\\'), f.path)
    };
    DupFileView {
        entry_id: f.entry.0,
        volume_id: f.volume.0,
        disk,
        location,
        path: f.path.clone(),
        full_path,
        modified: f.modified.map(|t| t.to_unix_millis()),
        online: f.online,
        permanent,
    }
}

fn group_view(
    g: &DupGroup,
    rules: &[Rule],
    volumes: &HashMap<VolumeId, (Volume, Option<String>)>,
) -> DupGroupView {
    let name = g
        .files
        .first()
        .map(|f| f.path.rsplit('\\').next().unwrap_or_default().to_owned())
        .unwrap_or_default();
    DupGroupView {
        key: format!(
            "{}-{}",
            g.size,
            g.hash
                .iter()
                .take(8)
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ),
        size: g.size,
        recoverable: g.recoverable(),
        kind: space::category_of(&space::extension(&name))
            .as_str()
            .to_owned(),
        files: g.files.iter().map(|f| file_view(f, volumes)).collect(),
        selected: prism_core::duplicates::select(g, rules)
            .into_iter()
            .map(|e| e.0)
            .collect(),
    }
}

/// The view of a search for duplicates. `volumes` maps every volume to its current mount when
/// connected.
pub fn view(
    result: &DupResult,
    rules: &[Rule],
    volumes: &HashMap<VolumeId, (Volume, Option<String>)>,
    pending_files: u64,
) -> DuplicatesView {
    DuplicatesView {
        groups: result
            .groups
            .iter()
            .map(|g| group_view(g, rules, volumes))
            .collect(),
        total_groups: result.total_groups,
        total_recoverable: result.total_recoverable,
        pending_files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_and_filters_from_the_interface() {
        let input = [
            RuleInput {
                kind: "neverIn".into(),
                volume_id: None,
                text: Some("  Backup ".into()),
            },
            RuleInput {
                kind: "keepOnVolume".into(),
                volume_id: Some(3),
                text: None,
            },
            RuleInput {
                kind: "keepOnVolume".into(),
                volume_id: None,
                text: None,
            },
            RuleInput {
                kind: "neverIn".into(),
                volume_id: None,
                text: Some("   ".into()),
            },
            RuleInput {
                kind: "whatever".into(),
                volume_id: None,
                text: None,
            },
            RuleInput {
                kind: "keepOldest".into(),
                volume_id: None,
                text: None,
            },
        ];
        assert_eq!(
            rules(&input),
            [
                Rule::NeverIn("Backup".into()),
                Rule::KeepOnVolume(VolumeId(3)),
                Rule::KeepOldest
            ]
        );
        let f = DupFilterInput {
            min_size: 10,
            category: Some("videos".into()),
            volume_id: Some(2),
            folder: Some("  ".into()),
        }
        .to_filter();
        assert_eq!(f.category, Some(FileCategory::Videos));
        assert_eq!(f.volume, Some(VolumeId(2)));
        assert_eq!(f.folder, None);
        assert_eq!(
            DupFilterInput {
                category: Some("nope".into()),
                ..DupFilterInput::default()
            }
            .to_filter()
            .category,
            None
        );
    }
}
