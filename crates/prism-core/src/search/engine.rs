//! The search engine (F07): runs a parsed [`Query`] on the in-memory indexes of the volumes,
//! online and offline (F07-AC4), and ranks what it finds.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::model::{EntryId, VolumeId};
use crate::names;
use crate::space::{self, FileCategory};

use super::index::{NO_PARENT, VolumeIndex};
use super::query::{Filter, Kind, Query};

/// What the engine needs to know about a volume besides its index.
#[derive(Clone, Debug)]
pub struct VolumeMeta {
    pub id: VolumeId,
    /// Names a `disk:` filter may use: user name, Windows label, mount (`D:\`).
    pub names: Vec<String>,
    pub online: bool,
}

/// Limits of a search.
#[derive(Clone, Copy, Debug)]
pub struct SearchOptions {
    /// Results returned, best first.
    pub limit: usize,
    /// Matches counted at most (the interface shows "more than N").
    pub count_cap: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 200,
            count_cap: 10_000,
        }
    }
}

/// One result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub volume: VolumeId,
    pub entry: EntryId,
    /// Lower is better (see [`score`]).
    pub score: u32,
}

/// The results of a search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchOutcome {
    pub hits: Vec<SearchHit>,
    /// Matches counted (all of them unless `capped`).
    pub total: usize,
    pub capped: bool,
}

/// A query that cannot run (F07-AC6).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SearchError {
    #[error("no disk is called `{0}`")]
    UnknownDisk(String),
}

/// How well a folded name matches the first word: exact, prefix, word start, elsewhere.
fn score(name: &[u8], term: &[u8]) -> u32 {
    if name == term {
        return 0;
    }
    if name.starts_with(term) {
        return 1;
    }
    let word_start = memchr::memmem::find_iter(name, term)
        .any(|p| p > 0 && matches!(name[p - 1], b' ' | b'_' | b'-' | b'.' | b'(' | b'['));
    if word_start { 2 } else { 3 }
}

fn extension_of(folded: &[u8]) -> String {
    std::str::from_utf8(folded).map_or_else(|_| String::new(), space::extension)
}

/// Runs `query` on the indexes; volumes without an index are skipped.
///
/// # Errors
/// [`SearchError::UnknownDisk`] when a `disk:` filter matches no volume.
pub fn search(
    indexes: &[Arc<VolumeIndex>],
    volumes: &[VolumeMeta],
    query: &Query,
    opts: SearchOptions,
) -> Result<SearchOutcome, SearchError> {
    // Which volumes take part.
    let mut allowed: HashSet<VolumeId> = volumes.iter().map(|v| v.id).collect();
    let mut size = (0u64, u64::MAX);
    let mut after = i64::MIN;
    let mut before = i64::MAX;
    let mut exts: Option<HashSet<String>> = None;
    let mut cats: Option<HashSet<FileCategory>> = None;
    let mut kind: Option<Kind> = None;
    let mut paths: Vec<Vec<u8>> = Vec::new();
    let mut regexes = Vec::new();
    for f in &query.filters {
        match f {
            Filter::Disk(name) => {
                let wanted = names::fold(name.trim_end_matches('\\'));
                let found: HashSet<VolumeId> = volumes
                    .iter()
                    .filter(|v| {
                        v.names
                            .iter()
                            .any(|n| names::fold(n.trim_end_matches('\\')) == wanted)
                    })
                    .map(|v| v.id)
                    .collect();
                if found.is_empty() {
                    return Err(SearchError::UnknownDisk(name.clone()));
                }
                allowed.retain(|v| found.contains(v));
            }
            Filter::Online(on) => {
                allowed.retain(|id| volumes.iter().any(|v| v.id == *id && v.online == *on));
            }
            Filter::Size { min, max } => {
                size.0 = size.0.max(min.unwrap_or(0));
                size.1 = size.1.min(max.unwrap_or(u64::MAX));
            }
            Filter::Modified {
                after: a,
                before: b,
            } => {
                if let Some(a) = a {
                    after = after.max(a.0);
                }
                if let Some(b) = b {
                    before = before.min(b.0);
                }
            }
            Filter::Ext(list) => {
                let set: HashSet<String> = list.iter().cloned().collect();
                exts = Some(match exts {
                    Some(prev) => prev.intersection(&set).cloned().collect(),
                    None => set,
                });
            }
            Filter::Type(list) => {
                let set: HashSet<FileCategory> = list.iter().copied().collect();
                cats = Some(match cats {
                    Some(prev) => prev.intersection(&set).copied().collect(),
                    None => set,
                });
            }
            Filter::Is(k) => kind = Some(*k),
            Filter::Path(p) => paths.push(p.as_bytes().to_vec()),
            Filter::Regex(r) => {
                // Checked by the parser; names in the index are upper-cased.
                if let Ok(re) = regex::RegexBuilder::new(r)
                    .case_insensitive(true)
                    .size_limit(1 << 20)
                    .build()
                {
                    regexes.push(re);
                }
            }
        }
    }
    let file_filters = size != (0, u64::MAX) || exts.is_some() || cats.is_some();

    // The longest word finds the candidates; the others are checked on each.
    let mut terms: Vec<&[u8]> = query.terms.iter().map(|t| t.as_bytes()).collect();
    terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
    let first_term = query.terms.first().map(String::as_bytes);

    let mut out = SearchOutcome::default();
    let keep = opts.limit.saturating_mul(10).max(2000);
    let mut found: Vec<SearchHit> = Vec::new();
    for ix in indexes.iter().filter(|ix| allowed.contains(&ix.volume)) {
        let mut path_memo: HashMap<u32, bool> = HashMap::new();
        let mut accept = |i: u32| -> bool {
            let name = ix.folded_name(i);
            if ix.parent(i) == NO_PARENT {
                return false; // the volume root
            }
            if !terms
                .iter()
                .skip(1)
                .all(|t| memchr::memmem::find(name, t).is_some())
            {
                return false;
            }
            let is_dir = ix.is_dir(i);
            match kind {
                Some(Kind::Dir) if !is_dir => return false,
                Some(Kind::File) if is_dir => return false,
                _ => {}
            }
            if is_dir && file_filters {
                return false;
            }
            let s = ix.size(i);
            if !is_dir && (s < size.0 || s > size.1) {
                return false;
            }
            let m = ix.modified(i);
            if (after != i64::MIN || before != i64::MAX) && (m == 0 || m < after || m >= before) {
                return false;
            }
            if exts.is_some() || cats.is_some() {
                let ext = extension_of(name).to_lowercase();
                if exts.as_ref().is_some_and(|e| !e.contains(&ext)) {
                    return false;
                }
                if cats
                    .as_ref()
                    .is_some_and(|c| !c.contains(&space::category_of(&ext)))
                {
                    return false;
                }
            }
            if !regexes.is_empty() {
                let text = String::from_utf8_lossy(name);
                if !regexes.iter().all(|re| re.is_match(&text)) {
                    return false;
                }
            }
            for p in &paths {
                let parent = ix.parent(i);
                let inside = *path_memo
                    .entry(parent)
                    .or_insert_with(|| folder_path(ix, parent).windows(p.len()).any(|w| w == p));
                if !inside {
                    return false;
                }
            }
            true
        };
        let mut visit = |i: u32| -> bool {
            if accept(i) {
                out.total += 1;
                if found.len() < keep {
                    let name = ix.folded_name(i);
                    found.push(SearchHit {
                        volume: ix.volume,
                        entry: ix.id(i),
                        score: first_term.map_or(3, |t| score(name, t)) * 1000
                            + u32::try_from(name.len().min(999)).unwrap_or(999),
                    });
                }
                if out.total >= opts.count_cap {
                    out.capped = true;
                    return false;
                }
            }
            true
        };
        match terms.first() {
            Some(t) => ix.for_each_match(t, &mut visit),
            None => {
                for i in 0..u32::try_from(ix.len()).unwrap_or(u32::MAX) {
                    if !visit(i) {
                        break;
                    }
                }
            }
        }
        if out.capped {
            break;
        }
    }
    found.sort_by_key(|h| (h.score, h.volume.0, h.entry.0));
    found.truncate(opts.limit);
    out.hits = found;
    Ok(out)
}

/// The folded path of a folder from the volume root (`\` between names).
fn folder_path(ix: &VolumeIndex, mut i: u32) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    while i != NO_PARENT && ix.parent(i) != NO_PARENT {
        parts.push(ix.folded_name(i));
        i = ix.parent(i);
    }
    parts.reverse();
    parts.join(&b'\\')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntryMeta, FileTime, VolumeInfo, VolumeStatus};
    use crate::search::query::{Clock, parse};
    use crate::store::{self, Store, WriteOp};

    const NOW: i64 = 1_794_000_000_000;
    const DAY: i64 = 86_400_000;

    /// `(path, size, age in days)`; paths ending in `\` are folders.
    type Items = &'static [(&'static str, u64, i64)];

    /// Two volumes, one online and one offline.
    fn catalog() -> (tempfile::TempDir, Vec<Arc<VolumeIndex>>, Vec<VolumeMeta>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("catalog.db")).unwrap();
        let mut indexes = Vec::new();
        let mut metas = Vec::new();
        let disks: [(&str, bool, Items); 2] = [
            (
                "Windows",
                true,
                &[
                    (r"Users\", 0, 0),
                    (r"Users\me\", 0, 0),
                    (r"Users\me\Documenti\", 0, 0),
                    (r"Users\me\Documenti\Report 2024.pdf", 2_000_000, 10),
                    (r"Users\me\Documenti\report-old.docx", 50_000, 700),
                    (r"Users\me\Pictures\", 0, 0),
                    (r"Users\me\Pictures\IMG_0001.JPG", 3_000_000, 40),
                    (r"Users\me\Pictures\passport.png", 900_000, 5),
                    (r"Reports\", 0, 0),
                    (r"report", 10, 1),
                ],
            ),
            (
                "Backup Foto",
                false,
                &[
                    (r"2019\", 0, 0),
                    (r"2019\IMG_0001.JPG", 3_000_000, 2000),
                    (r"2019\film.mkv", 4_000_000_000, 2000),
                ],
            ),
        ];
        for (name, online, items) in disks {
            let info = VolumeInfo::dev_folder(&format!("{name}\\"));
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
            let mut dirs = HashMap::from([(String::new(), root)]);
            for (path, size, age) in items {
                let is_dir = path.ends_with('\\');
                let trimmed = path.trim_end_matches('\\');
                let (parent, leaf) = trimmed.rsplit_once('\\').unwrap_or(("", trimmed));
                let id = EntryId(next);
                next += 1;
                ops.push(WriteOp::Insert {
                    id,
                    volume,
                    parent: Some(dirs[parent]),
                    name: leaf.to_owned(),
                    is_dir,
                    meta: EntryMeta {
                        size: *size,
                        modified: Some(FileTime::from_unix_millis(NOW - age * DAY)),
                        ..EntryMeta::default()
                    },
                });
                if is_dir {
                    dirs.insert(trimmed.to_owned(), id);
                }
            }
            let session = store.session();
            session.submit(volume, ops).unwrap();
            session.flush().unwrap();
            indexes.push(Arc::new(
                store.read(|c| VolumeIndex::build(c, volume)).unwrap(),
            ));
            metas.push(VolumeMeta {
                id: volume,
                names: vec![name.to_owned(), format!("{}:\\", &name[..1])],
                online,
            });
        }
        (dir, indexes, metas)
    }

    fn run(text: &str) -> Result<Vec<String>, SearchError> {
        let (_d, indexes, metas) = catalog();
        let q = parse(
            text,
            Clock {
                now_ms: NOW,
                utc_offset_min: 0,
            },
        )
        .unwrap();
        let outcome = search(&indexes, &metas, &q, SearchOptions::default())?;
        Ok(outcome
            .hits
            .iter()
            .map(|h| {
                let ix = indexes.iter().find(|ix| ix.volume == h.volume).unwrap();
                let i = (0..u32::try_from(ix.len()).unwrap())
                    .find(|&i| ix.id(i) == h.entry)
                    .unwrap();
                String::from_utf8_lossy(ix.folded_name(i)).to_lowercase()
            })
            .collect())
    }

    // F07-AC2 and ranking: exact name first, then names starting with the word, then word
    // starts, then anywhere; shorter names first, then catalog order.
    #[test]
    fn f07_ac2_substring_and_ranking() {
        assert_eq!(
            run("report").unwrap(),
            ["report", "reports", "report 2024.pdf", "report-old.docx"]
        );
        // "port": word start ("report-old" has none, "passport" neither) ranks like anywhere;
        // the exact-or-prefix matches are absent, so shorter names come first.
        assert_eq!(
            run("port").unwrap(),
            [
                "report",
                "reports",
                "passport.png",
                "report 2024.pdf",
                "report-old.docx"
            ]
        );
        assert_eq!(run("REPORT 2024").unwrap(), ["report 2024.pdf"]);
    }

    // F07-AC4: offline volumes are searched too, and `online:` filters them.
    #[test]
    fn f07_ac4_offline_volumes_are_included() {
        assert_eq!(run("img_0001").unwrap().len(), 2);
        assert_eq!(run("img_0001 online:no").unwrap().len(), 1);
        assert_eq!(run("img_0001 disk:\"backup foto\"").unwrap().len(), 1);
        assert_eq!(run("img_0001 w:").unwrap().len(), 1);
    }

    #[test]
    fn f07_ac5_filters() {
        assert_eq!(run("size:>1GB").unwrap(), ["film.mkv"]);
        // Without words, every match ranks alike: shorter names, then catalog order.
        assert_eq!(
            run("type:image").unwrap(),
            ["img_0001.jpg", "passport.png", "img_0001.jpg"]
        );
        assert_eq!(
            run("ext:pdf,docx").unwrap(),
            ["report 2024.pdf", "report-old.docx"]
        );
        assert_eq!(
            run("report modified:<30d is:file").unwrap(),
            ["report", "report 2024.pdf"]
        );
        assert_eq!(run("report modified:>1y").unwrap(), ["report-old.docx"]);
        assert_eq!(
            run("is:dir path:users").unwrap(),
            ["me", "pictures", "documenti"]
        );
        assert_eq!(
            run("path:documenti").unwrap(),
            ["report 2024.pdf", "report-old.docx"]
        );
        assert_eq!(
            run("regex:^img_\\d+\\.jpg$ online:yes").unwrap(),
            ["img_0001.jpg"]
        );
        assert!(run("film type:document").unwrap().is_empty());
    }

    // F07-AC6: a disk that does not exist is an error, not an empty result.
    #[test]
    fn f07_ac6_unknown_disk() {
        assert_eq!(
            run("report disk:Archivio"),
            Err(SearchError::UnknownDisk("Archivio".into()))
        );
    }

    #[test]
    fn counting_is_capped() {
        let (_d, indexes, metas) = catalog();
        let q = parse(
            "e",
            Clock {
                now_ms: NOW,
                utc_offset_min: 0,
            },
        )
        .unwrap();
        let all = search(&indexes, &metas, &q, SearchOptions::default()).unwrap();
        assert!(!all.capped);
        let capped = search(
            &indexes,
            &metas,
            &q,
            SearchOptions {
                limit: 2,
                count_cap: 3,
            },
        )
        .unwrap();
        assert!(capped.capped);
        assert_eq!(capped.total, 3);
        assert_eq!(capped.hits.len(), 2);
    }
}
