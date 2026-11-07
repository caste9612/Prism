//! Space analysis from the catalog (F08): the content of a folder ordered by size, a pruned
//! tree for the treemap, the largest files of a subtree and the statistics by file type. Only
//! the catalog is read, so all of it works for offline volumes too (F08-AC4).
//!
//! Folder sizes come from the aggregates (`dir_stats`), where a file with several hard links
//! counts once; a file's own size is its logical size. The type statistics count every name,
//! hard links included.

use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{EntryId, FileTime, VolumeId};
use crate::names;
use crate::store::{self, StoreError, from_db};

/// One child of a folder, for the space views.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceItem {
    pub id: EntryId,
    /// Display name (decoded from the stored form).
    pub name: String,
    pub is_dir: bool,
    /// Logical size; for a folder, the total of its subtree.
    pub size: u64,
    /// Files below (1 for a file).
    pub files: u64,
    pub modified: Option<FileTime>,
}

fn display_name(stored: &str) -> String {
    names::decode(stored).to_string_lossy().into_owned()
}

/// The children of a folder, largest first (F08-AC2); ties by name.
///
/// # Errors
/// SQLite errors.
pub fn folder_items(conn: &Connection, folder: EntryId) -> Result<Vec<SpaceItem>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT e.id, e.name, e.is_dir, \
                CASE WHEN e.is_dir THEN coalesce(d.total_size, 0) ELSE e.size END AS sz, \
                CASE WHEN e.is_dir THEN coalesce(d.file_count, 0) ELSE 1 END, \
                e.modified_at \
         FROM entries e LEFT JOIN dir_stats d ON d.entry_id = e.id \
         WHERE e.parent_id = ?1 ORDER BY sz DESC, e.name",
    )?;
    let rows = stmt.query_map([folder.0], |r| {
        Ok(SpaceItem {
            id: EntryId(r.get(0)?),
            name: display_name(&r.get::<_, String>(1)?),
            is_dir: r.get(2)?,
            size: from_db(r.get(3)?),
            files: from_db(r.get(4)?),
            modified: r.get::<_, Option<i64>>(5)?.map(FileTime),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// A node of the treemap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreemapNode {
    pub id: EntryId,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub children: Vec<TreemapNode>,
    /// Total size of the children left out because too small to be drawn.
    pub other: u64,
}

/// Limits of a treemap, so that it stays fast on folders with millions of entries (F08-AC5).
#[derive(Clone, Copy, Debug)]
pub struct TreemapOptions {
    /// Levels below the folder (1 = its children only).
    pub depth: u32,
    /// Children smaller than this fraction of the folder are grouped as `other`.
    pub min_fraction: f64,
    /// At most this many nodes in all.
    pub max_nodes: usize,
}

impl Default for TreemapOptions {
    /// The depth is only a safety limit: size threshold and node budget decide. Measured on
    /// C: with 1.37 million entries (docs/benchmarks/2026-11-07-t4-space.md).
    fn default() -> Self {
        Self {
            depth: 64,
            min_fraction: 0.000_02,
            max_nodes: 12_000,
        }
    }
}

/// The tree drawn by the treemap of `folder` (F08-AC1): its subtree down to `depth` levels,
/// largest first, without the items too small to be visible.
///
/// # Errors
/// [`StoreError::NotFound`] if `folder` does not exist; SQLite errors.
pub fn treemap(
    conn: &Connection,
    folder: EntryId,
    opts: TreemapOptions,
) -> Result<TreemapNode, StoreError> {
    let entry = store::entry(conn, folder)?
        .ok_or_else(|| StoreError::NotFound(format!("entry {folder}")))?;
    let size = if entry.is_dir {
        store::dir_stats(conn, folder)?.map_or(0, |d| d.total_size)
    } else {
        entry.meta.size
    };
    let root = TreemapNode {
        id: entry.id,
        name: display_name(&entry.name),
        is_dir: entry.is_dir,
        size,
        children: Vec::new(),
        other: 0,
    };
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    #[allow(clippy::cast_sign_loss)]
    let min_size = (((size as f64) * opts.min_fraction).ceil() as u64).max(1);

    // Largest folders first, at any depth: the node budget goes where the space is, so the big
    // files of deep folders (games, virtual disks) are drawn before small shallow items. With a
    // fixed number of levels, deep trees showed only empty folder blocks. Nodes live in a flat
    // list (with the index of their parent and the levels left below them) and are assembled
    // at the end.
    let mut flat: Vec<(TreemapNode, Option<usize>, u32)> = vec![(root, None, opts.depth)];
    let mut queue = std::collections::BinaryHeap::from([(size, std::cmp::Reverse(0usize))]);
    let mut budget = opts.max_nodes;
    while let Some((_, std::cmp::Reverse(i))) = queue.pop() {
        if budget == 0 {
            break; // the folders left are drawn as plain blocks
        }
        let (node, _, levels) = &flat[i];
        if *levels == 0 || !node.is_dir {
            continue;
        }
        let (id, levels) = (node.id, *levels);
        for item in folder_items(conn, id)? {
            if item.size >= min_size && budget > 0 {
                budget -= 1;
                let child = TreemapNode {
                    id: item.id,
                    name: item.name,
                    is_dir: item.is_dir,
                    size: item.size,
                    children: Vec::new(),
                    other: 0,
                };
                if item.is_dir {
                    queue.push((item.size, std::cmp::Reverse(flat.len())));
                }
                flat.push((child, Some(i), levels - 1));
            } else {
                flat[i].0.other += item.size;
            }
        }
    }
    // Children come after their parent, largest first: moving them from the end keeps the
    // tree valid and gives each parent its children in reverse order.
    while flat.len() > 1 {
        if let Some((child, Some(parent), _)) = flat.pop() {
            flat[parent].0.children.push(child);
        }
    }
    // What is left is the root, the only node without a parent.
    let Some((mut root, None, _)) = flat.pop() else {
        return Err(StoreError::Corrupt(format!("treemap of {folder}")));
    };
    reverse_children(&mut root);
    Ok(root)
}

fn reverse_children(node: &mut TreemapNode) {
    node.children.reverse();
    node.children.iter_mut().for_each(reverse_children);
}

/// A large file and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LargeFile {
    pub item: SpaceItem,
    /// Folder path relative to the volume root (display form, `\` separators).
    pub folder: String,
}

/// The `limit` largest files below `folder` (F08-AC2).
///
/// # Errors
/// SQLite errors.
pub fn largest_files(
    conn: &Connection,
    folder: EntryId,
    limit: usize,
) -> Result<Vec<LargeFile>, StoreError> {
    let entry = store::entry(conn, folder)?
        .ok_or_else(|| StoreError::NotFound(format!("entry {folder}")))?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let cols = "f.id, f.name, f.size, f.modified_at, f.parent_id";
    let rows: Vec<(SpaceItem, Option<i64>)> = if entry.parent.is_none() {
        // The whole volume: no need to walk the tree.
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {cols} FROM entries f WHERE f.volume_id = ?1 AND f.is_dir = 0 \
             ORDER BY f.size DESC, f.id LIMIT ?2"
        ))?;
        stmt.query_map(params![entry.volume.0, limit], large_row)?
            .collect::<Result<_, _>>()?
    } else {
        // For each folder of the subtree, its files through the (parent_id, name) index:
        // `CROSS JOIN` fixes the join order and `+is_dir` keeps SQLite from building an
        // automatic index on `is_dir` instead, which made the query scan every file of the
        // volume for each folder (over 2 minutes on C:\Users; 0.6 s with this plan).
        let mut stmt = conn.prepare_cached(&format!(
            "WITH RECURSIVE sub(id) AS ( \
                 SELECT ?1 UNION ALL \
                 SELECT e.id FROM sub CROSS JOIN entries e ON e.parent_id = sub.id \
                 WHERE +e.is_dir = 1) \
             SELECT {cols} FROM sub CROSS JOIN entries f ON f.parent_id = sub.id \
             WHERE +f.is_dir = 0 ORDER BY f.size DESC, f.id LIMIT ?2"
        ))?;
        stmt.query_map(params![folder.0, limit], large_row)?
            .collect::<Result<_, _>>()?
    };
    rows.into_iter()
        .map(|(item, parent)| {
            let folder = match parent {
                Some(p) => store::relative_path(conn, EntryId(p))?
                    .split('\\')
                    .map(display_name)
                    .collect::<Vec<_>>()
                    .join("\\"),
                None => String::new(),
            };
            Ok(LargeFile { item, folder })
        })
        .collect()
}

fn large_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(SpaceItem, Option<i64>)> {
    Ok((
        SpaceItem {
            id: EntryId(r.get(0)?),
            name: display_name(&r.get::<_, String>(1)?),
            is_dir: false,
            size: from_db(r.get(2)?),
            files: 1,
            modified: r.get::<_, Option<i64>>(3)?.map(FileTime),
        },
        r.get(4)?,
    ))
}

// ---------------------------------------------------------------------------------------------
// Statistics by type (F08-AC3)

/// Broad kinds of files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileCategory {
    Images,
    Videos,
    Audio,
    Documents,
    Archives,
    DiskImages,
    Code,
    Programs,
    Other,
}

impl FileCategory {
    pub const ALL: [Self; 9] = [
        Self::Images,
        Self::Videos,
        Self::Audio,
        Self::Documents,
        Self::Archives,
        Self::DiskImages,
        Self::Code,
        Self::Programs,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Images => "images",
            Self::Videos => "videos",
            Self::Audio => "audio",
            Self::Documents => "documents",
            Self::Archives => "archives",
            Self::DiskImages => "disk_images",
            Self::Code => "code",
            Self::Programs => "programs",
            Self::Other => "other",
        }
    }
}

/// The category of a file extension (lowercase, without the dot).
pub fn category_of(ext: &str) -> FileCategory {
    use FileCategory as C;
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "heic" | "heif"
        | "avif" | "svg" | "ico" | "psd" | "raw" | "cr2" | "cr3" | "nef" | "arw" | "dng"
        | "orf" | "rw2" | "raf" | "pef" | "srw" => C::Images,
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "m4v" | "mpg" | "mpeg" | "webm" | "flv" | "3gp"
        | "mts" | "m2ts" | "ts" | "vob" => C::Videos, // `.ts`: video transport streams weigh far more than TypeScript
        "mp3" | "flac" | "wav" | "aac" | "m4a" | "ogg" | "opus" | "wma" | "aiff" | "alac"
        | "mid" | "midi" => C::Audio,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
        | "rtf" | "txt" | "md" | "csv" | "epub" | "pages" | "numbers" | "key" | "one" | "msg"
        | "eml" => C::Documents,
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "cab" | "lz"
        | "lzma" => C::Archives,
        "iso" | "img" | "vhd" | "vhdx" | "vmdk" | "vdi" | "qcow2" | "wim" | "esd" | "dmg" => {
            C::DiskImages
        }
        "rs" | "c" | "h" | "cpp" | "hpp" | "cs" | "java" | "kt" | "py" | "js" | "mjs" | "tsx"
        | "jsx" | "go" | "rb" | "php" | "swift" | "html" | "css" | "scss" | "json" | "xml"
        | "yaml" | "yml" | "toml" | "sql" | "sh" | "ps1" | "bat" | "svelte" | "vue" => C::Code,
        "exe" | "dll" | "msi" | "msix" | "appx" | "sys" | "com" | "scr" | "jar" | "apk" => {
            C::Programs
        }
        _ => C::Other,
    }
}

/// Extension of a stored file name: lowercase, without the dot, empty if none (names like
/// `.gitignore` have no extension).
pub fn extension(stored_name: &str) -> String {
    match stored_name.rfind('.') {
        Some(i) if i > 0 && i + 1 < stored_name.len() => {
            let ext = &stored_name[i + 1..];
            if ext.len() <= 16 && !ext.contains(' ') {
                ext.to_lowercase()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

/// Size bands of the statistics, as their lower bounds in bytes (decimal units).
pub const SIZE_BANDS: [u64; 6] = [0, 1_000, 100_000, 1_000_000, 100_000_000, 1_000_000_000];

fn band_of(size: u64) -> usize {
    SIZE_BANDS.iter().rposition(|&b| size >= b).unwrap_or(0)
}

/// A number of files and their total size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub files: u64,
    pub bytes: u64,
}

impl Totals {
    fn add(&mut self, size: u64) {
        self.files += 1;
        self.bytes += size;
    }
}

/// Distribution of the files by category, extension and size band.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeStats {
    pub total: Totals,
    /// Every category, in [`FileCategory::ALL`] order.
    pub categories: Vec<(FileCategory, Totals)>,
    /// The extensions with most bytes, largest first (empty string = no extension).
    pub extensions: Vec<(String, Totals)>,
    /// Totals of the extensions left out of `extensions`.
    pub other_extensions: Totals,
    /// One per [`SIZE_BANDS`] entry.
    pub size_bands: Vec<Totals>,
}

/// What the statistics cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsScope {
    Volume(VolumeId),
    Catalog,
}

/// Statistics by type (F08-AC3), for one volume or the whole catalog, with the `top`
/// extensions by size listed one by one.
///
/// # Errors
/// SQLite errors.
pub fn type_stats(
    conn: &Connection,
    scope: StatsScope,
    top: usize,
) -> Result<TypeStats, StoreError> {
    let mut by_ext: std::collections::HashMap<String, Totals> = std::collections::HashMap::new();
    let mut bands = vec![Totals::default(); SIZE_BANDS.len()];
    let mut total = Totals::default();
    let mut visit = |name: String, size: i64| {
        let size = from_db(size);
        total.add(size);
        bands[band_of(size)].add(size);
        by_ext.entry(extension(&name)).or_default().add(size);
    };
    match scope {
        StatsScope::Volume(v) => {
            let mut stmt = conn.prepare_cached(
                "SELECT name, size FROM entries WHERE volume_id = ?1 AND is_dir = 0",
            )?;
            let mut rows = stmt.query([v.0])?;
            while let Some(r) = rows.next()? {
                visit(r.get(0)?, r.get(1)?);
            }
        }
        StatsScope::Catalog => {
            let mut stmt =
                conn.prepare_cached("SELECT name, size FROM entries WHERE is_dir = 0")?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                visit(r.get(0)?, r.get(1)?);
            }
        }
    }
    let mut categories: Vec<(FileCategory, Totals)> = FileCategory::ALL
        .iter()
        .map(|c| (*c, Totals::default()))
        .collect();
    for (ext, t) in &by_ext {
        let cat = category_of(ext);
        if let Some((_, ct)) = categories.iter_mut().find(|(c, _)| *c == cat) {
            ct.files += t.files;
            ct.bytes += t.bytes;
        }
    }
    let mut extensions: Vec<(String, Totals)> = by_ext.into_iter().collect();
    extensions.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then_with(|| a.0.cmp(&b.0)));
    let mut other_extensions = Totals::default();
    for (_, t) in extensions.iter().skip(top) {
        other_extensions.files += t.files;
        other_extensions.bytes += t.bytes;
    }
    extensions.truncate(top);
    Ok(TypeStats {
        total,
        categories,
        extensions,
        other_extensions,
        size_bands: bands,
    })
}

/// The folder an entry id refers to, checked to belong to `volume` (the interface passes ids,
/// never paths: ADR-0003).
///
/// # Errors
/// [`StoreError::NotFound`] if the entry is missing or on another volume.
pub fn folder_of_volume(
    conn: &Connection,
    volume: VolumeId,
    folder: Option<EntryId>,
) -> Result<EntryId, StoreError> {
    match folder {
        None => store::root_entry(conn, volume)?
            .map(|e| e.id)
            .ok_or_else(|| StoreError::NotFound(format!("volume {volume} is not indexed"))),
        Some(id) => {
            let found: Option<(i64, bool)> = conn
                .prepare_cached("SELECT volume_id, is_dir FROM entries WHERE id = ?1")?
                .query_row([id.0], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            match found {
                Some((v, true)) if v == volume.0 => Ok(id),
                _ => Err(StoreError::NotFound(format!(
                    "folder {id} of volume {volume}"
                ))),
            }
        }
    }
}

/// The folders from the volume root down to `folder`, for the breadcrumb (F08-AC1).
///
/// # Errors
/// [`StoreError::NotFound`] if the entry does not exist.
pub fn breadcrumb(
    conn: &Connection,
    folder: EntryId,
) -> Result<Vec<(EntryId, String)>, StoreError> {
    let mut chain = store::ancestors_and_self(conn, folder)?;
    chain.reverse();
    chain
        .into_iter()
        .map(|id| {
            let e = store::entry(conn, id)?
                .ok_or_else(|| StoreError::NotFound(format!("entry {id}")))?;
            Ok((id, display_name(&e.name)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntryMeta, VolumeInfo, VolumeStatus};
    use crate::store::{Store, WriteOp};

    /// A catalog from `(path, size)` pairs; paths ending in `\` are folders.
    fn catalog(items: &[(&str, u64)]) -> (tempfile::TempDir, Store, VolumeId) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("catalog.db")).unwrap();
        let info = VolumeInfo::dev_folder(r"T:\");
        let volume = store
            .write(move |w| store::insert_volume(w, &info, VolumeStatus::Indexed))
            .unwrap();
        let ids = store.allocate_entry_ids(u32::try_from(items.len() + 1).unwrap());
        let mut next = ids.0;
        let root = EntryId(next);
        next += 1;
        let mut ops = vec![WriteOp::Insert {
            id: root,
            volume,
            parent: None,
            name: String::new(),
            is_dir: true,
            meta: EntryMeta::default(),
        }];
        let mut dirs: std::collections::HashMap<String, EntryId> =
            std::collections::HashMap::from([(String::new(), root)]);
        for (path, size) in items {
            let is_dir = path.ends_with('\\');
            let trimmed = path.trim_end_matches('\\');
            let (parent, name) = trimmed.rsplit_once('\\').unwrap_or(("", trimmed));
            let id = EntryId(next);
            next += 1;
            ops.push(WriteOp::Insert {
                id,
                volume,
                parent: Some(dirs[parent]),
                name: name.to_owned(),
                is_dir,
                meta: EntryMeta {
                    size: *size,
                    ..EntryMeta::default()
                },
            });
            if is_dir {
                dirs.insert(trimmed.to_owned(), id);
            }
        }
        ops.push(WriteOp::RecomputeDirStats {
            dirs: dirs.values().copied().collect(),
        });
        let session = store.session();
        session.submit(volume, ops).unwrap();
        session.flush().unwrap();
        (dir, store, volume)
    }

    fn sample() -> (tempfile::TempDir, Store, VolumeId) {
        catalog(&[
            (r"Video\", 0),
            (r"Video\film.mkv", 7_000),
            (r"Video\clip.mp4", 1_000),
            (r"Foto\", 0),
            (r"Foto\2024\", 0),
            (r"Foto\2024\a.jpg", 900),
            (r"Foto\2024\b.CR2", 600),
            (r"Foto\c.png", 300),
            (r"notes.txt", 150),
            (r"tiny.bin", 1),
            (r".gitignore", 49),
        ])
    }

    #[test]
    fn f08_ac2_folder_items_largest_first() {
        let (_d, store, volume) = sample();
        let items = store
            .read(|c| {
                let root = folder_of_volume(c, volume, None)?;
                folder_items(c, root)
            })
            .unwrap();
        let view: Vec<(&str, u64, bool, u64)> = items
            .iter()
            .map(|i| (i.name.as_str(), i.size, i.is_dir, i.files))
            .collect();
        assert_eq!(
            view,
            [
                ("Video", 8_000, true, 2),
                ("Foto", 1_800, true, 3),
                ("notes.txt", 150, false, 1),
                (".gitignore", 49, false, 1),
                ("tiny.bin", 1, false, 1),
            ]
        );
    }

    #[test]
    fn f08_ac1_treemap_is_pruned_and_bounded() {
        let (_d, store, volume) = sample();
        let tree = store
            .read(|c| {
                let root = folder_of_volume(c, volume, None)?;
                treemap(
                    c,
                    root,
                    TreemapOptions {
                        depth: 3,
                        min_fraction: 0.01, // 1% of 10_000 = 100 bytes
                        max_nodes: 100,
                    },
                )
            })
            .unwrap();
        assert_eq!(tree.size, 10_000);
        let names: Vec<&str> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Video", "Foto", "notes.txt"]);
        assert_eq!(tree.other, 50, ".gitignore and tiny.bin grouped");
        let foto = &tree.children[1];
        assert_eq!(foto.children[0].name, "2024");
        assert_eq!(foto.children[0].children.len(), 2);
        // Every level adds up: drawn children plus `other` equal the folder size.
        fn check(n: &TreemapNode) {
            if n.is_dir && !n.children.is_empty() {
                let sum: u64 = n.children.iter().map(|c| c.size).sum::<u64>() + n.other;
                assert_eq!(sum, n.size, "{}", n.name);
                n.children.iter().for_each(check);
            }
        }
        check(&tree);

        // Depth 1 and a node budget of 2.
        let small = store
            .read(|c| {
                let root = folder_of_volume(c, volume, None)?;
                treemap(
                    c,
                    root,
                    TreemapOptions {
                        depth: 1,
                        min_fraction: 0.0,
                        max_nodes: 2,
                    },
                )
            })
            .unwrap();
        assert_eq!(small.children.len(), 2);
        assert!(small.children.iter().all(|c| c.children.is_empty()));
        assert_eq!(small.other, 200);
    }

    // The node budget goes to the largest folders first, whatever their depth: the big file
    // two levels down is drawn, the small folder's child is grouped.
    #[test]
    fn treemap_budget_goes_to_the_largest_folders() {
        let (_d, store, volume) = catalog(&[
            (r"Big\", 0),
            (r"Big\Sub\", 0),
            (r"Big\Sub\deep.bin", 5_000),
            (r"Big\x.bin", 3_000),
            (r"Small\", 0),
            (r"Small\s.bin", 1_000),
        ]);
        let tree = store
            .read(|c| {
                let root = folder_of_volume(c, volume, None)?;
                treemap(
                    c,
                    root,
                    TreemapOptions {
                        depth: 5,
                        min_fraction: 0.0,
                        max_nodes: 5,
                    },
                )
            })
            .unwrap();
        let big = &tree.children[0];
        let small = &tree.children[1];
        assert_eq!((big.name.as_str(), small.name.as_str()), ("Big", "Small"));
        let sub = &big.children[0];
        assert_eq!(sub.name, "Sub");
        assert_eq!(sub.children.len(), 1);
        assert_eq!(sub.children[0].name, "deep.bin");
        // Budget spent: Small is not expanded and is drawn as a plain block.
        assert!(small.children.is_empty());
        assert_eq!(small.other, 0);
        assert_eq!(small.size, 1_000);
    }

    #[test]
    fn f08_ac2_largest_files_of_volume_and_subtree() {
        let (_d, store, volume) = sample();
        let (all, foto) = store
            .read(|c| {
                let root = folder_of_volume(c, volume, None)?;
                let foto = folder_items(c, root)?
                    .into_iter()
                    .find(|i| i.name == "Foto")
                    .unwrap()
                    .id;
                Ok((largest_files(c, root, 3)?, largest_files(c, foto, 10)?))
            })
            .unwrap();
        let view = |v: &[LargeFile]| -> Vec<(String, String)> {
            v.iter()
                .map(|f| (f.item.name.clone(), f.folder.clone()))
                .collect()
        };
        assert_eq!(
            view(&all),
            [
                ("film.mkv".into(), "Video".into()),
                ("clip.mp4".into(), "Video".into()),
                ("a.jpg".into(), r"Foto\2024".into()),
            ]
        );
        assert_eq!(
            view(&foto),
            [
                ("a.jpg".into(), r"Foto\2024".into()),
                ("b.CR2".into(), r"Foto\2024".into()),
                ("c.png".into(), "Foto".into()),
            ]
        );
    }

    #[test]
    fn f08_ac3_type_statistics() {
        let (_d, store, volume) = sample();
        let stats = store
            .read(|c| type_stats(c, StatsScope::Volume(volume), 3))
            .unwrap();
        assert_eq!(
            stats.total,
            Totals {
                files: 8,
                bytes: 10_000
            }
        );
        let cat = |c: FileCategory| stats.categories.iter().find(|(k, _)| *k == c).unwrap().1;
        assert_eq!(
            cat(FileCategory::Videos),
            Totals {
                files: 2,
                bytes: 8_000
            }
        );
        assert_eq!(
            cat(FileCategory::Images),
            Totals {
                files: 3,
                bytes: 1_800
            }
        );
        assert_eq!(
            cat(FileCategory::Documents),
            Totals {
                files: 1,
                bytes: 150
            }
        );
        assert_eq!(
            cat(FileCategory::Other),
            Totals {
                files: 2,
                bytes: 50
            }
        );
        let exts: Vec<&str> = stats.extensions.iter().map(|(e, _)| e.as_str()).collect();
        assert_eq!(exts, ["mkv", "mp4", "jpg"]);
        assert_eq!(
            stats.other_extensions,
            Totals {
                files: 5,
                bytes: 1_100
            }
        );
        // Bands: <1 KB: 300, 150, 1, 49, 900, 600; 1-100 KB: 7000, 1000.
        assert_eq!(
            stats.size_bands[0],
            Totals {
                files: 6,
                bytes: 2_000
            }
        );
        assert_eq!(
            stats.size_bands[1],
            Totals {
                files: 2,
                bytes: 8_000
            }
        );
        assert_eq!(
            stats.size_bands.iter().map(|t| t.files).sum::<u64>(),
            stats.total.files
        );
        let catalog = store
            .read(|c| type_stats(c, StatsScope::Catalog, 3))
            .unwrap();
        assert_eq!(catalog.total, stats.total);
    }

    #[test]
    fn extensions_and_bands() {
        assert_eq!(extension("Photo.JPG"), "jpg");
        assert_eq!(extension("archive.tar.gz"), "gz");
        assert_eq!(extension(".gitignore"), "");
        assert_eq!(extension("README"), "");
        assert_eq!(extension("trailing."), "");
        assert_eq!(extension("v1.0 final"), "");
        assert_eq!(band_of(0), 0);
        assert_eq!(band_of(999), 0);
        assert_eq!(band_of(1_000), 1);
        assert_eq!(band_of(5_000_000_000), 5);
        assert_eq!(category_of("cr2"), FileCategory::Images);
        assert_eq!(category_of("vhdx"), FileCategory::DiskImages);
    }

    #[test]
    fn folder_ids_are_checked_against_the_volume() {
        let (_d, store, volume) = sample();
        store
            .read(|c| {
                let root = folder_of_volume(c, volume, None)?;
                let items = folder_items(c, root)?;
                let file = items.iter().find(|i| !i.is_dir).unwrap().id;
                assert!(folder_of_volume(c, volume, Some(file)).is_err(), "a file");
                assert!(folder_of_volume(c, VolumeId(999), Some(root)).is_err());
                let video = items[0].id;
                assert_eq!(folder_of_volume(c, volume, Some(video))?, video);
                let crumbs = breadcrumb(c, video)?;
                assert_eq!(crumbs, [(root, String::new()), (video, "Video".into())]);
                Ok(())
            })
            .unwrap();
    }
}
