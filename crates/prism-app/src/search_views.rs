//! Search for the interface (F07): types exchanged with the search views and the details of
//! the results. The language, the index and the engine live in `prism-core::search`.

use std::collections::HashMap;

use prism_core::model::{EntryId, Volume, VolumeId};
use prism_core::names;
use prism_core::search::engine::SearchHit;
use prism_core::search::query::QueryError;
use prism_core::space;
use prism_core::store::{self, StoreError};
use rusqlite::Connection;
use serde::Serialize;
use ts_rs::TS;

/// One result.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchHitView {
    #[ts(type = "number")]
    pub volume_id: i64,
    #[ts(type = "number")]
    pub entry_id: i64,
    pub name: String,
    /// Folder relative to the volume root (`\` separators, empty at the root).
    pub folder: String,
    /// Full path through the mount the volume had last (for copying, F07-AC7).
    pub full_path: String,
    pub is_dir: bool,
    /// `dir` or the file category.
    pub kind: String,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number | null")]
    pub modified: Option<i64>,
    pub online: bool,
    /// Name of the disk (user name, label or mount) and its physical location (F07-AC4).
    pub disk: String,
    pub location: Option<String>,
}

/// A mistake in the search text, for the interface to show where it is (F07-AC6).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueryErrorView {
    pub code: String,
    pub value: String,
    /// Character positions (not bytes) in the text.
    pub start: u32,
    pub end: u32,
}

impl QueryErrorView {
    pub fn new(code: &str, value: &str, text: &str, start: usize, end: usize) -> Self {
        let chars = |byte: usize| {
            u32::try_from(
                text.get(..byte.min(text.len()))
                    .map_or(0, |s| s.chars().count()),
            )
            .unwrap_or(0)
        };
        Self {
            code: code.to_owned(),
            value: value.to_owned(),
            start: chars(start),
            end: chars(end),
        }
    }

    pub fn from_query(e: &QueryError, text: &str) -> Self {
        Self::new(e.code, &e.value, text, e.start, e.end)
    }
}

/// The answer to a search.
#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchResultView {
    pub hits: Vec<SearchHitView>,
    #[ts(type = "number")]
    pub total: u64,
    pub capped: bool,
    pub error: Option<QueryErrorView>,
    /// Time spent, for the status line.
    pub ms: f64,
}

/// The name shown for a volume: user name, Windows label, mount.
pub fn disk_name(v: &Volume) -> String {
    [v.labels.name.as_deref(), v.info.os_label.as_deref()]
        .into_iter()
        .flatten()
        .find(|n| !n.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| v.info.last_mount.clone())
        .unwrap_or_else(|| format!("#{}", v.id.0))
}

/// The details of the hits, read from the catalog; folder paths are resolved once per folder.
///
/// # Errors
/// SQLite errors.
pub fn hit_views(
    conn: &Connection,
    hits: &[SearchHit],
    volumes: &HashMap<VolumeId, (Volume, bool)>,
) -> Result<Vec<SearchHitView>, StoreError> {
    let mut folders: HashMap<EntryId, String> = HashMap::new();
    let mut out = Vec::with_capacity(hits.len());
    for h in hits {
        let Some(e) = store::entry(conn, h.entry)? else {
            continue; // removed since the index was built
        };
        let Some((volume, online)) = volumes.get(&h.volume) else {
            continue;
        };
        let folder = match e.parent {
            Some(p) => {
                if let Some(f) = folders.get(&p) {
                    f.clone()
                } else {
                    let f = store::relative_path(conn, p)?
                        .split('\\')
                        .filter(|c| !c.is_empty())
                        .map(|c| names::decode(c).to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("\\");
                    folders.insert(p, f.clone());
                    f
                }
            }
            None => String::new(),
        };
        let name = names::decode(&e.name).to_string_lossy().into_owned();
        let mount = volume
            .info
            .unc_path
            .clone()
            .or_else(|| volume.info.last_mount.clone())
            .unwrap_or_default();
        let mut full = mount.trim_end_matches('\\').to_owned();
        for part in [folder.as_str(), name.as_str()] {
            if !part.is_empty() {
                full.push('\\');
                full.push_str(part);
            }
        }
        out.push(SearchHitView {
            volume_id: h.volume.0,
            entry_id: h.entry.0,
            kind: if e.is_dir {
                "dir".to_owned()
            } else {
                space::category_of(&space::extension(&name))
                    .as_str()
                    .to_owned()
            },
            size: if e.is_dir {
                store::dir_stats(conn, e.id)?.map_or(0, |d| d.total_size)
            } else {
                e.meta.size
            },
            modified: e.meta.modified.map(|t| t.to_unix_millis()),
            is_dir: e.is_dir,
            online: *online,
            disk: disk_name(volume),
            location: volume
                .labels
                .location
                .clone()
                .filter(|l| !l.trim().is_empty()),
            folder,
            full_path: full,
            name,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_positions_are_in_characters() {
        // "città colour:red": the filter starts at byte 7 but character 6.
        let text = "città colour:red";
        let e = QueryErrorView::new("unknown_filter", "colour", text, 7, 13);
        assert_eq!((e.start, e.end), (6, 12));
    }
}
