//! Space analysis for the interface (F08): types exchanged with the screen "Space" and their
//! conversion from `prism_core::space`. The analysis itself lives in `prism-core`.

use prism_core::space::{self, LargeFile, SIZE_BANDS, SpaceItem, Totals, TreemapNode, TypeStats};
use serde::Serialize;
use ts_rs::TS;

/// `dir` for a folder, else the category of the file (same rules as the statistics).
fn kind_of(name: &str, is_dir: bool) -> String {
    if is_dir {
        "dir".to_owned()
    } else {
        space::category_of(&space::extension(name))
            .as_str()
            .to_owned()
    }
}

/// A file or folder in the space views.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpaceEntryView {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub is_dir: bool,
    /// `dir`, or the file category (`images`, `videos`, …) for its colour.
    pub kind: String,
    #[ts(type = "number")]
    pub size: u64,
    /// Files below (1 for a file).
    #[ts(type = "number")]
    pub files: u64,
    /// Last modification, milliseconds since 1970.
    #[ts(type = "number | null")]
    pub modified: Option<i64>,
}

impl From<SpaceItem> for SpaceEntryView {
    fn from(i: SpaceItem) -> Self {
        Self {
            id: i.id.0,
            kind: kind_of(&i.name, i.is_dir),
            name: i.name,
            is_dir: i.is_dir,
            size: i.size,
            files: i.files,
            modified: i.modified.map(|t| t.to_unix_millis()),
        }
    }
}

/// A node of the treemap (F08-AC1).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TreemapNodeView {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    /// `dir` for a folder, else the file category (`images`, `videos`, …) for its colour.
    pub kind: String,
    #[ts(type = "number")]
    pub size: u64,
    /// Size of the children too small to be drawn.
    #[ts(type = "number")]
    pub other: u64,
    pub children: Vec<TreemapNodeView>,
}

impl From<TreemapNode> for TreemapNodeView {
    fn from(n: TreemapNode) -> Self {
        Self {
            id: n.id.0,
            kind: kind_of(&n.name, n.is_dir),
            name: n.name,
            size: n.size,
            other: n.other,
            children: n.children.into_iter().map(Self::from).collect(),
        }
    }
}

/// One step of the breadcrumb (the root of the volume has an empty name).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CrumbView {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
}

/// A folder in the space screen: where it is, its content and its treemap.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpaceFolderView {
    #[ts(type = "number")]
    pub volume_id: i64,
    #[ts(type = "number")]
    pub folder_id: i64,
    pub breadcrumb: Vec<CrumbView>,
    /// Children largest first, at most [`MAX_ITEMS`].
    pub items: Vec<SpaceEntryView>,
    /// Children left out of `items`.
    #[ts(type = "number")]
    pub more_items: u64,
    pub treemap: TreemapNodeView,
}

/// Children of a folder sent to the interface (the treemap shows the rest).
pub const MAX_ITEMS: usize = 1000;

/// A large file and its folder (F08-AC2).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LargeFileView {
    pub entry: SpaceEntryView,
    /// Folder relative to the volume root, `\` separators.
    pub folder: String,
}

impl From<LargeFile> for LargeFileView {
    fn from(f: LargeFile) -> Self {
        Self {
            entry: f.item.into(),
            folder: f.folder,
        }
    }
}

/// A number of files and their size.
#[derive(Clone, Copy, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TotalsView {
    #[ts(type = "number")]
    pub files: u64,
    #[ts(type = "number")]
    pub bytes: u64,
}

impl From<Totals> for TotalsView {
    fn from(t: Totals) -> Self {
        Self {
            files: t.files,
            bytes: t.bytes,
        }
    }
}

/// Totals of one category, extension or size band.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StatRowView {
    /// Category key (`images`, …), extension (empty: none) or lower bound of the size band.
    pub key: String,
    /// The file category of a category or extension row (its colour); none for size bands.
    pub category: Option<String>,
    pub totals: TotalsView,
}

/// Statistics by type (F08-AC3).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TypeStatsView {
    pub total: TotalsView,
    pub categories: Vec<StatRowView>,
    pub extensions: Vec<StatRowView>,
    pub other_extensions: TotalsView,
    pub size_bands: Vec<StatRowView>,
}

impl From<TypeStats> for TypeStatsView {
    fn from(s: TypeStats) -> Self {
        let row = |key: String, category: Option<&str>, totals: Totals| StatRowView {
            key,
            category: category.map(str::to_owned),
            totals: totals.into(),
        };
        Self {
            total: s.total.into(),
            categories: s
                .categories
                .into_iter()
                .map(|(c, t)| row(c.as_str().to_owned(), Some(c.as_str()), t))
                .collect(),
            extensions: s
                .extensions
                .into_iter()
                .map(|(e, t)| {
                    let category = space::category_of(&e).as_str();
                    row(e, Some(category), t)
                })
                .collect(),
            other_extensions: s.other_extensions.into(),
            size_bands: SIZE_BANDS
                .iter()
                .zip(s.size_bands)
                .map(|(from, t)| row(from.to_string(), None, t))
                .collect(),
        }
    }
}
