//! Guided cleanup (F09): the categories of space that can be recovered, computed from the
//! catalog (kept current by the journal and the folder watchers). Deleting goes through
//! `fileops` (ADR-0012), which checks every item on the disk again first.
//!
//! What is never proposed (F09-AC2): system and program folders ([`DEFAULT_PROTECTED`]),
//! application data (`Users\*\AppData`, except the temporary and cache folders that the
//! dedicated categories target), hidden or system items and anything below them, links and
//! junctions, and folders whose name starts with a dot (`.git`, `.cache`) or a dollar sign
//! (`$SysReset`, `$WINDOWS.~BT`: Windows setup and recovery).

use rusqlite::{Connection, params};

use crate::model::{ATTR_REPARSE_POINT, EntryId, FileTime, VolumeId};
use crate::names;
use crate::store::{self, StoreError, to_db};

/// `FILE_ATTRIBUTE_HIDDEN` and `FILE_ATTRIBUTE_SYSTEM`.
const ATTR_HIDDEN: u32 = 0x2;
const ATTR_SYSTEM: u32 = 0x4;

/// The categories of F09-AC1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    /// Old files in the temporary folders of Windows and of the user.
    Temp,
    /// Folders with no file below them.
    EmptyFolders,
    /// Large files not modified for a long time.
    LargeOld,
    /// Known cache folders of browsers that are not running.
    BrowserCache,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Temp => "temp",
            Self::EmptyFolders => "empty_folders",
            Self::LargeOld => "large_old",
            Self::BrowserCache => "browser_cache",
        }
    }
}

/// An item proposed for cleanup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupItem {
    pub entry: EntryId,
    /// Path relative to the volume root, display form.
    pub path: String,
    pub is_dir: bool,
    /// Logical size (subtree size for a folder).
    pub size: u64,
    pub files: u64,
    pub modified: Option<FileTime>,
    /// The browser a cache belongs to.
    pub owner: Option<String>,
}

/// Folder layout of a browser's caches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheLayout {
    /// Chrome, Edge, Brave: `User Data\<profile>\{Cache, Code Cache, GPUCache}` and the shader
    /// caches of `User Data`.
    Chromium,
    /// Firefox: `Profiles\<profile>\cache2` (in the local application data).
    Firefox,
}

/// Where a browser keeps its caches on a volume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserCaches {
    pub browser: String,
    /// Executable name, to know whether it runs (F09-AC4).
    pub process: String,
    /// `User Data` (Chromium) or `Profiles` (Firefox) folder, relative to the volume root.
    pub folder: String,
    pub layout: CacheLayout,
}

/// The rules of a cleanup scan of one volume.
#[derive(Clone, Debug)]
pub struct CleanupRules {
    /// Temporary folders on this volume, relative to its root.
    pub temp_dirs: Vec<String>,
    /// Temporary files modified after this are left alone (they may be in use).
    pub temp_before: FileTime,
    /// Large and old files: at least this size...
    pub large_min_size: u64,
    /// ...and not modified since this.
    pub large_before: FileTime,
    pub browsers: Vec<BrowserCaches>,
    /// Paths (relative, `*` matches one component) never proposed by the other categories.
    pub protected: Vec<String>,
}

/// Folders that no category touches (relative to any volume root; `*` = any one component).
pub const DEFAULT_PROTECTED: &[&str] = &[
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "Recovery",
    "PerfLogs",
    "Boot",
    "EFI",
    "$Recycle.Bin",
    "System Volume Information",
    "$WinREAgent",
    "WindowsApps",
    // Created empty by Windows updates since April 2025 as a security mitigation; Microsoft
    // asks not to delete it.
    "inetpub",
    "Users\\*\\AppData",
    "Users\\Default",
    "Users\\All Users",
];

/// The protected paths as components.
fn patterns(rules: &CleanupRules) -> Vec<Vec<String>> {
    rules
        .protected
        .iter()
        .map(|p| {
            p.split(['\\', '/'])
                .filter(|c| !c.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

fn matches_prefix(path: &[String], pattern: &[String]) -> bool {
    path.len() >= pattern.len()
        && pattern
            .iter()
            .zip(path)
            .all(|(p, c)| p == "*" || names::eq_ignore_case(p, c))
}

/// Display components from the root down to `id`, with the attributes of each.
fn chain(conn: &Connection, id: EntryId) -> Result<Vec<(String, u32)>, StoreError> {
    let mut stmt =
        conn.prepare_cached("SELECT parent_id, name, attrs FROM entries WHERE id = ?1")?;
    let mut out = Vec::new();
    let mut current = Some(id);
    while let Some(cur) = current {
        let (parent, name, attrs): (Option<i64>, String, i64) = stmt
            .query_row([cur.0], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(StoreError::from)?;
        if parent.is_some() {
            let display = names::decode(&name).to_string_lossy().into_owned();
            out.push((display, u32::try_from(attrs).unwrap_or(0)));
        }
        current = parent.map(EntryId);
        if out.len() > 10_000 {
            return Err(StoreError::Corrupt(format!("cycle above entry {id}")));
        }
    }
    out.reverse();
    Ok(out)
}

/// Folders that hold an installed program: an application lives there, wherever it was
/// installed (games in `D:\Programs`, Steam libraries, portable tools), and its empty folders
/// and big data files are not the user's to clean (F09-AC2). A folder counts as one when it
/// directly contains a `.dll`: installed applications nearly always do, while the folders
/// where users keep their files (Downloads with a setup `.exe`) rarely do.
struct ProgramFolders {
    memo: std::collections::HashMap<EntryId, bool>,
}

impl ProgramFolders {
    fn new() -> Self {
        Self {
            memo: std::collections::HashMap::new(),
        }
    }

    fn holds_program(&mut self, conn: &Connection, folder: EntryId) -> Result<bool, StoreError> {
        if let Some(&known) = self.memo.get(&folder) {
            return Ok(known);
        }
        let found: bool = conn
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM entries WHERE parent_id = ?1 AND is_dir = 0 \
                 AND name LIKE '%.dll')",
            )?
            .query_row([folder.0], |r| r.get(0))?;
        self.memo.insert(folder, found);
        Ok(found)
    }

    /// Whether `id` lies inside a program folder (the volume root does not count).
    fn contains(&mut self, conn: &Connection, id: EntryId) -> Result<bool, StoreError> {
        let chain = store::ancestors_and_self(conn, id)?;
        // chain: id, parent, ..., root; skip the root.
        for folder in chain.iter().take(chain.len().saturating_sub(1)) {
            if self.holds_program(conn, *folder)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Whether an item may be proposed by the categories that scan the whole volume.
fn allowed(chain: &[(String, u32)], protected: &[Vec<String>]) -> bool {
    let names: Vec<String> = chain.iter().map(|(n, _)| n.clone()).collect();
    // A user profile folder itself (`Users\<name>`), even empty: Windows may still list it.
    let profile_root = names.len() == 2 && names::eq_ignore_case(&names[0], "Users");
    !profile_root
        && !protected.iter().any(|p| matches_prefix(&names, p))
        && chain.iter().all(|(name, attrs)| {
            attrs & (ATTR_HIDDEN | ATTR_SYSTEM | ATTR_REPARSE_POINT) == 0
                && !name.starts_with('.')
                && !name.starts_with('$')
        })
}

fn item(
    conn: &Connection,
    id: EntryId,
    path: &[(String, u32)],
    owner: Option<&str>,
) -> Result<CleanupItem, StoreError> {
    let e = store::entry(conn, id)?.ok_or_else(|| StoreError::NotFound(format!("entry {id}")))?;
    let (size, files) = if e.is_dir {
        store::dir_stats(conn, id)?.map_or((0, 0), |d| (d.total_size, d.file_count))
    } else {
        (e.meta.size, 1)
    };
    Ok(CleanupItem {
        entry: id,
        path: path
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join("\\"),
        is_dir: e.is_dir,
        size,
        files,
        modified: e.meta.modified,
        owner: owner.map(str::to_owned),
    })
}

/// Files of a subtree (the folder itself excluded) modified before `before`.
fn files_below(
    conn: &Connection,
    folder: EntryId,
    before: FileTime,
) -> Result<Vec<EntryId>, StoreError> {
    // Same plan as `space::largest_files`: fixed join order through the parent index.
    let mut stmt = conn.prepare_cached(
        "WITH RECURSIVE sub(id) AS ( \
             SELECT ?1 UNION ALL \
             SELECT e.id FROM sub CROSS JOIN entries e ON e.parent_id = sub.id \
             WHERE +e.is_dir = 1) \
         SELECT f.id FROM sub CROSS JOIN entries f ON f.parent_id = sub.id \
         WHERE +f.is_dir = 0 AND f.modified_at < ?2 ORDER BY f.size DESC, f.id",
    )?;
    let rows = stmt.query_map(params![folder.0, before.0], |r| Ok(EntryId(r.get(0)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Temporary files older than `rules.temp_before` (F09-AC1). Files in use are left alone by
/// the deletion itself (the shell refuses them).
///
/// # Errors
/// SQLite errors.
pub fn temp_files(
    conn: &Connection,
    volume: VolumeId,
    rules: &CleanupRules,
) -> Result<Vec<CleanupItem>, StoreError> {
    let mut out = Vec::new();
    for dir in &rules.temp_dirs {
        let Some(folder) = store::resolve_relative_path(conn, volume, dir)? else {
            continue;
        };
        for id in files_below(conn, folder.id, rules.temp_before)? {
            let path = chain(conn, id)?;
            out.push(item(conn, id, &path, None)?);
        }
    }
    Ok(out)
}

/// The top-most folders without any file below them (F09-AC1): removing one removes its empty
/// subfolders too.
///
/// # Errors
/// SQLite errors.
pub fn empty_folders(
    conn: &Connection,
    volume: VolumeId,
    rules: &CleanupRules,
) -> Result<Vec<CleanupItem>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT e.id FROM entries e \
         JOIN dir_stats d ON d.entry_id = e.id \
         JOIN entries p ON p.id = e.parent_id \
         LEFT JOIN dir_stats pd ON pd.entry_id = p.id \
         WHERE e.volume_id = ?1 AND e.is_dir = 1 AND d.file_count = 0 \
           AND (p.parent_id IS NULL OR coalesce(pd.file_count, 0) > 0) \
         ORDER BY e.id",
    )?;
    let ids: Vec<EntryId> = stmt
        .query_map([volume.0], |r| Ok(EntryId(r.get(0)?)))?
        .collect::<Result<_, _>>()?;
    let protected = patterns(rules);
    let mut programs = ProgramFolders::new();
    let mut out = Vec::new();
    for id in ids {
        let path = chain(conn, id)?;
        if allowed(&path, &protected) && !programs.contains(conn, id)? {
            out.push(item(conn, id, &path, None)?);
        }
    }
    Ok(out)
}

/// Files of at least `rules.large_min_size` not modified since `rules.large_before`, largest
/// first (F09-AC1). Temporary and cache folders are left to their own categories.
///
/// # Errors
/// SQLite errors.
pub fn large_old_files(
    conn: &Connection,
    volume: VolumeId,
    rules: &CleanupRules,
) -> Result<Vec<CleanupItem>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id FROM entries WHERE volume_id = ?1 AND is_dir = 0 AND size >= ?2 \
         AND modified_at < ?3 ORDER BY size DESC, id",
    )?;
    let ids: Vec<EntryId> = stmt
        .query_map(
            params![volume.0, to_db(rules.large_min_size), rules.large_before.0],
            |r| Ok(EntryId(r.get(0)?)),
        )?
        .collect::<Result<_, _>>()?;
    let mut protected = patterns(rules);
    protected.extend(
        rules
            .temp_dirs
            .iter()
            .map(|d| d.split('\\').map(str::to_owned).collect()),
    );
    let mut programs = ProgramFolders::new();
    let mut out = Vec::new();
    for id in ids {
        let path = chain(conn, id)?;
        if allowed(&path, &protected) && !programs.contains(conn, id)? {
            out.push(item(conn, id, &path, None)?);
        }
    }
    Ok(out)
}

/// Cache folders of the given browsers (F09-AC1): only known cache folders, never profiles,
/// cookies, passwords or history. Whether the browser runs is checked by the caller
/// (F09-AC4).
///
/// # Errors
/// SQLite errors.
pub fn browser_caches(
    conn: &Connection,
    volume: VolumeId,
    browser: &BrowserCaches,
) -> Result<Vec<CleanupItem>, StoreError> {
    let Some(root) = store::resolve_relative_path(conn, volume, &browser.folder)? else {
        return Ok(Vec::new());
    };
    let mut found: Vec<EntryId> = Vec::new();
    let pick = |parent: EntryId, wanted: &[&str], found: &mut Vec<EntryId>| {
        for name in wanted {
            if let Some(c) = store::child_by_name(conn, parent, name)?
                && c.is_dir
            {
                found.push(c.id);
            }
        }
        Ok::<(), StoreError>(())
    };
    match browser.layout {
        CacheLayout::Chromium => {
            pick(
                root.id,
                &["ShaderCache", "GrShaderCache", "GraphiteDawnCache"],
                &mut found,
            )?;
            for profile in store::children(conn, root.id)?.iter().filter(|c| c.is_dir) {
                pick(profile.id, &["Cache", "Code Cache", "GPUCache"], &mut found)?;
            }
        }
        CacheLayout::Firefox => {
            for profile in store::children(conn, root.id)?.iter().filter(|c| c.is_dir) {
                pick(profile.id, &["cache2"], &mut found)?;
            }
        }
    }
    let mut out = Vec::new();
    for id in found {
        let path = chain(conn, id)?;
        let it = item(conn, id, &path, Some(&browser.browser))?;
        if it.size > 0 {
            out.push(it);
        }
    }
    out.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    Ok(out)
}

/// The path of `abs` relative to `root` (case-insensitive, `\` separators), if it is below it.
#[must_use]
pub fn relative_under(root: &str, abs: &str) -> Option<String> {
    let root = root.replace('/', "\\");
    let abs = abs.replace('/', "\\");
    let root = root.trim_end_matches('\\');
    let head = abs.get(..root.len())?;
    if !names::eq_ignore_case(head, root) {
        return None;
    }
    let rest = abs.get(root.len()..)?;
    if !rest.is_empty() && !rest.starts_with('\\') {
        return None;
    }
    Some(rest.trim_matches('\\').to_owned())
}

/// Total size of items.
#[must_use]
pub fn total(items: &[CleanupItem]) -> u64 {
    items.iter().map(|i| i.size).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntryMeta, VolumeInfo, VolumeStatus};
    use crate::store::{Store, WriteOp};

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_794_000_000_000; // 2026-11-06

    /// A catalog from `(path, size, age in days, attrs)`; paths ending in `\` are folders.
    fn catalog(items: &[(&str, u64, i64, u32)]) -> (tempfile::TempDir, Store, VolumeId) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("catalog.db")).unwrap();
        let info = VolumeInfo::dev_folder(r"C:\");
        let volume = store
            .write(move |w| store::insert_volume(w, &info, VolumeStatus::Indexed))
            .unwrap();
        let first = store.allocate_entry_ids(u32::try_from(items.len() + 1).unwrap());
        let mut next = first.0;
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
        let mut dirs = std::collections::HashMap::from([(String::new(), root)]);
        for (path, size, age, attrs) in items {
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
                    modified: Some(FileTime::from_unix_millis(NOW - age * DAY)),
                    attrs: *attrs,
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

    const GB: u64 = 1_000_000_000;

    fn rules() -> CleanupRules {
        CleanupRules {
            temp_dirs: vec![
                r"Users\me\AppData\Local\Temp".into(),
                r"Windows\Temp".into(),
            ],
            temp_before: FileTime::from_unix_millis(NOW - DAY),
            large_min_size: GB / 2,
            large_before: FileTime::from_unix_millis(NOW - 365 * DAY),
            browsers: Vec::new(),
            protected: DEFAULT_PROTECTED.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn sample() -> (tempfile::TempDir, Store, VolumeId) {
        catalog(&[
            (r"Users\", 0, 0, 0),
            (r"Users\me\", 0, 0, 0),
            (r"Users\me\AppData\", 0, 0, ATTR_HIDDEN),
            (r"Users\me\AppData\Local\", 0, 0, 0),
            (r"Users\me\AppData\Local\Temp\", 0, 0, 0),
            (r"Users\me\AppData\Local\Temp\old.tmp", 500, 10, 0),
            (r"Users\me\AppData\Local\Temp\fresh.tmp", 900, 0, 0),
            (r"Users\me\AppData\Local\Temp\sub\", 0, 0, 0),
            (r"Users\me\AppData\Local\Temp\sub\deep.log", 300, 3, 0),
            (r"Users\me\AppData\Local\Temp\empty\", 0, 0, 0),
            (r"Users\me\AppData\Local\Google\", 0, 0, 0),
            (r"Users\me\AppData\Local\Google\Chrome\", 0, 0, 0),
            (r"Users\me\AppData\Local\Google\Chrome\User Data\", 0, 0, 0),
            (
                r"Users\me\AppData\Local\Google\Chrome\User Data\Default\",
                0,
                0,
                0,
            ),
            (
                r"Users\me\AppData\Local\Google\Chrome\User Data\Default\Cache\",
                0,
                0,
                0,
            ),
            (
                r"Users\me\AppData\Local\Google\Chrome\User Data\Default\Cache\data_1",
                7000,
                1,
                0,
            ),
            (
                r"Users\me\AppData\Local\Google\Chrome\User Data\Default\Cookies",
                50,
                1,
                0,
            ),
            (
                r"Users\me\AppData\Local\Google\Chrome\User Data\Default\GPUCache\",
                0,
                0,
                0,
            ),
            (
                r"Users\me\AppData\Local\Google\Chrome\User Data\Default\GPUCache\x",
                10,
                1,
                0,
            ),
            (
                r"Users\me\AppData\Local\Google\Chrome\User Data\ShaderCache\",
                0,
                0,
                0,
            ),
            (
                r"Users\me\AppData\Local\Google\Chrome\User Data\ShaderCache\s",
                20,
                1,
                0,
            ),
            (r"Users\me\AppData\Local\Docker\", 0, 0, 0),
            (r"Users\me\AppData\Local\Docker\disk.vhdx", 6 * GB, 400, 0),
            (r"Users\me\AppData\Local\EmptyAppDir\", 0, 0, 0),
            (r"Users\me\Videos\", 0, 0, 0),
            (r"Users\me\Videos\holiday-2019.mkv", 2 * GB, 800, 0),
            (r"Users\me\Videos\recent.mkv", 3 * GB, 30, 0),
            (r"Users\me\Videos\small-old.mkv", GB / 10, 900, 0),
            (r"Users\me\Projects\", 0, 0, 0),
            (r"Users\me\Projects\a\", 0, 0, 0),
            (r"Users\me\Projects\a\empty1\", 0, 0, 0),
            (r"Users\me\Projects\a\empty1\empty2\", 0, 0, 0),
            (r"Users\me\Projects\a\main.rs", 10, 5, 0),
            (r"Users\me\Projects\a\.git\", 0, 0, ATTR_HIDDEN),
            (r"Users\me\Projects\a\.git\refs\", 0, 0, 0),
            (r"Users\me\Projects\a\.cache\", 0, 0, 0),
            (r"Users\me\Projects\link\", 0, 0, ATTR_REPARSE_POINT),
            (r"Windows\", 0, 0, 0),
            (r"Windows\Temp\", 0, 0, 0),
            (r"Windows\Temp\setup.log", 100, 30, 0),
            (r"Windows\EmptyInWindows\", 0, 0, 0),
            (r"Windows\big-old.dat", 4 * GB, 999, 0),
            (r"hiberfil.sys", 8 * GB, 999, ATTR_HIDDEN | ATTR_SYSTEM),
            (r"inetpub\", 0, 0, 0),
            (r"Users\TEMP\", 0, 0, 0),
            (r"$SysReset\", 0, 0, 0),
            (r"Old Backups\", 0, 0, 0),
            (r"Old Backups\image.iso", GB, 700, 0),
            // A program installed outside Program Files (D24/D25): its empty folders and old
            // data files are never proposed.
            (r"Games\", 0, 0, 0),
            (r"Games\Steam\", 0, 0, 0),
            (r"Games\Steam\steam.exe", 4_000_000, 300, 0),
            (r"Games\Steam\steamclient.dll", 9_000_000, 300, 0),
            (r"Games\Steam\steamapps\", 0, 0, 0),
            (r"Games\Steam\steamapps\downloading\", 0, 0, 0),
            (r"Games\Steam\steamapps\common\", 0, 0, 0),
            (r"Games\Steam\steamapps\common\Game\", 0, 0, 0),
            (
                r"Games\Steam\steamapps\common\Game\data.pak",
                3 * GB,
                800,
                0,
            ),
            (r"Downloads\", 0, 0, 0),
            (r"Downloads\setup.exe", 50_000_000, 900, 0),
            (r"Downloads\old-dvd.iso", 4 * GB, 900, 0),
            (r"Downloads\empty-unzip\", 0, 0, 0),
        ])
    }

    fn paths(items: &[CleanupItem]) -> Vec<&str> {
        items.iter().map(|i| i.path.as_str()).collect()
    }

    #[test]
    fn f09_temp_files_old_enough() {
        let (_d, store, volume) = sample();
        let items = store.read(|c| temp_files(c, volume, &rules())).unwrap();
        assert_eq!(
            paths(&items),
            [
                r"Users\me\AppData\Local\Temp\old.tmp",
                r"Users\me\AppData\Local\Temp\sub\deep.log",
                r"Windows\Temp\setup.log",
            ]
        );
        assert_eq!(total(&items), 900);
    }

    #[test]
    fn f09_ac2_empty_folders_outside_protected_areas() {
        let (_d, store, volume) = sample();
        let items = store.read(|c| empty_folders(c, volume, &rules())).unwrap();
        // Top-most only (empty1 holds empty2); nothing in AppData, Windows, hidden or dot
        // folders, links, program folders (Steam); Downloads with a setup .exe is not one.
        assert_eq!(
            paths(&items),
            [r"Users\me\Projects\a\empty1", r"Downloads\empty-unzip"]
        );
        assert!(items[0].is_dir);
    }

    #[test]
    fn f09_ac2_large_old_files_outside_protected_areas() {
        let (_d, store, volume) = sample();
        let items = store
            .read(|c| large_old_files(c, volume, &rules()))
            .unwrap();
        assert_eq!(
            paths(&items),
            [
                r"Downloads\old-dvd.iso",
                r"Users\me\Videos\holiday-2019.mkv",
                r"Old Backups\image.iso"
            ]
        );
        assert_eq!(items[1].size, 2 * GB);
        // The 3 GB game data inside the Steam folder is not proposed.
        assert!(!paths(&items).iter().any(|p| p.ends_with("data.pak")));
    }

    #[test]
    fn f09_browser_caches_only_known_cache_folders() {
        let (_d, store, volume) = sample();
        let chrome = BrowserCaches {
            browser: "Chrome".into(),
            process: "chrome.exe".into(),
            folder: r"Users\me\AppData\Local\Google\Chrome\User Data".into(),
            layout: CacheLayout::Chromium,
        };
        let items = store.read(|c| browser_caches(c, volume, &chrome)).unwrap();
        assert_eq!(
            paths(&items),
            [
                r"Users\me\AppData\Local\Google\Chrome\User Data\Default\Cache",
                r"Users\me\AppData\Local\Google\Chrome\User Data\ShaderCache",
                r"Users\me\AppData\Local\Google\Chrome\User Data\Default\GPUCache",
            ]
        );
        assert!(
            items
                .iter()
                .all(|i| i.is_dir && i.owner.as_deref() == Some("Chrome"))
        );
        assert!(!paths(&items).iter().any(|p| p.ends_with("Cookies")));
        let missing = BrowserCaches {
            folder: r"Users\me\AppData\Local\Mozilla\Firefox\Profiles".into(),
            layout: CacheLayout::Firefox,
            ..chrome
        };
        assert!(
            store
                .read(|c| browser_caches(c, volume, &missing))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative_under(r"C:\", r"c:\Users\me\AppData\Local\Temp").as_deref(),
            Some(r"Users\me\AppData\Local\Temp")
        );
        assert_eq!(relative_under(r"C:\", r"C:\").as_deref(), Some(""));
        assert_eq!(relative_under(r"D:\", r"C:\Windows"), None);
        assert_eq!(relative_under(r"C:\Users", r"C:\Users2\x"), None);
    }
}
