//! In-memory name index for the search (ADR-0010, option A, "like Everything"): the case-folded
//! names of a volume in one byte buffer, scanned for substrings with SIMD (`memchr::memmem`),
//! plus the few fields the filters need (parent, kind, size, modification time).
//!
//! The index is built from the catalog and holds no information the catalog does not have; it
//! is rebuilt (or updated) when the catalog changes.

use memchr::memmem;
use rusqlite::Connection;

use crate::model::{EntryId, VolumeId};
use crate::names;
use crate::store::{StoreError, from_db};

/// Parent index of the volume root.
pub const NO_PARENT: u32 = u32::MAX;

/// Separator between names in the buffer: never part of a name.
const SEP: u8 = 0;

const FLAG_DIR: u8 = 1;

/// The searchable names of one volume.
#[derive(Debug)]
pub struct VolumeIndex {
    pub volume: VolumeId,
    /// Case-folded UTF-8 names, each followed by [`SEP`].
    names: Vec<u8>,
    /// Start of each name in `names`.
    starts: Vec<u32>,
    ids: Vec<i64>,
    parents: Vec<u32>,
    sizes: Vec<u64>,
    modified: Vec<i64>,
    flags: Vec<u8>,
}

/// One entry of the index, by position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub volume: VolumeId,
    pub index: u32,
}

impl VolumeIndex {
    /// Builds the index of a volume from the catalog.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn build(conn: &Connection, volume: VolumeId) -> Result<Self, StoreError> {
        let mut stmt = conn.prepare(
            "SELECT id, parent_id, name, is_dir, size, coalesce(modified_at, 0) \
             FROM entries WHERE volume_id = ?1 ORDER BY id",
        )?;
        let mut rows = stmt.query([volume.0])?;
        let mut ix = Self {
            volume,
            names: Vec::new(),
            starts: Vec::new(),
            ids: Vec::new(),
            parents: Vec::new(),
            sizes: Vec::new(),
            modified: Vec::new(),
            flags: Vec::new(),
        };
        let mut parent_ids: Vec<Option<i64>> = Vec::new();
        while let Some(r) = rows.next()? {
            let id: i64 = r.get(0)?;
            let parent: Option<i64> = r.get(1)?;
            let stored: String = r.get(2)?;
            let display = names::decode(&stored);
            let folded = names::fold(&display.to_string_lossy());
            ix.starts
                .push(u32::try_from(ix.names.len()).unwrap_or(u32::MAX));
            ix.names.extend_from_slice(folded.as_bytes());
            ix.names.push(SEP);
            ix.ids.push(id);
            parent_ids.push(parent);
            ix.sizes.push(from_db(r.get(4)?));
            ix.modified.push(r.get(5)?);
            ix.flags
                .push(if r.get::<_, bool>(3)? { FLAG_DIR } else { 0 });
        }
        // Parents by position: ids are sorted, so a binary search finds them.
        ix.parents = parent_ids
            .into_iter()
            .map(|p| {
                p.and_then(|p| ix.ids.binary_search(&p).ok())
                    .and_then(|i| u32::try_from(i).ok())
                    .unwrap_or(NO_PARENT)
            })
            .collect();
        // The vectors grew by doubling: give back the unused capacity (about a third).
        ix.names.shrink_to_fit();
        ix.starts.shrink_to_fit();
        ix.ids.shrink_to_fit();
        ix.sizes.shrink_to_fit();
        ix.modified.shrink_to_fit();
        ix.flags.shrink_to_fit();
        Ok(ix)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Approximate memory used, in bytes.
    pub fn memory(&self) -> usize {
        self.names.capacity()
            + self.starts.capacity() * 4
            + self.ids.capacity() * 8
            + self.parents.capacity() * 4
            + self.sizes.capacity() * 8
            + self.modified.capacity() * 8
            + self.flags.capacity()
    }

    pub fn id(&self, i: u32) -> EntryId {
        EntryId(self.ids[i as usize])
    }

    pub fn parent(&self, i: u32) -> u32 {
        self.parents[i as usize]
    }

    pub fn is_dir(&self, i: u32) -> bool {
        self.flags[i as usize] & FLAG_DIR != 0
    }

    pub fn size(&self, i: u32) -> u64 {
        self.sizes[i as usize]
    }

    /// Modification time (FILETIME), 0 if unknown.
    pub fn modified(&self, i: u32) -> i64 {
        self.modified[i as usize]
    }

    /// The folded name of an entry.
    pub fn folded_name(&self, i: u32) -> &[u8] {
        let start = self.starts[i as usize] as usize;
        let end = self
            .starts
            .get(i as usize + 1)
            .map_or(self.names.len(), |&s| s as usize)
            - 1;
        &self.names[start..end]
    }

    /// Positions of the entries whose folded name contains `needle` (already folded), in index
    /// order, at most `limit`. The volume root (empty name) never matches.
    pub fn find(&self, needle: &[u8], limit: usize) -> Vec<u32> {
        let mut out = Vec::new();
        if needle.is_empty() {
            return out;
        }
        let finder = memmem::Finder::new(needle);
        let mut last = u32::MAX;
        for pos in finder.find_iter(&self.names) {
            let i = self.starts.partition_point(|&s| s as usize <= pos) - 1;
            let i = u32::try_from(i).unwrap_or(u32::MAX);
            if i != last {
                out.push(i);
                last = i;
                if out.len() >= limit {
                    break;
                }
            }
        }
        out
    }

    /// Like [`VolumeIndex::find`], on `threads` slices of the buffer in parallel.
    pub fn find_parallel(&self, needle: &[u8], limit: usize, threads: usize) -> Vec<u32> {
        let n = self.starts.len();
        if threads <= 1 || n < 200_000 || needle.is_empty() {
            return self.find(needle, limit);
        }
        let chunk = n.div_ceil(threads);
        let finder = memmem::Finder::new(needle);
        let parts: Vec<Vec<u32>> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads)
                .map(|t| {
                    let finder = &finder;
                    s.spawn(move || {
                        let first = t * chunk;
                        let last = ((t + 1) * chunk).min(n);
                        if first >= last {
                            return Vec::new();
                        }
                        let lo = self.starts[first] as usize;
                        let hi = self
                            .starts
                            .get(last)
                            .map_or(self.names.len(), |&x| x as usize);
                        let mut out = Vec::new();
                        let mut prev = u32::MAX;
                        for pos in finder.find_iter(&self.names[lo..hi]) {
                            let abs = lo + pos;
                            let i = self.starts.partition_point(|&s| s as usize <= abs) - 1;
                            let i = u32::try_from(i).unwrap_or(u32::MAX);
                            if i != prev {
                                out.push(i);
                                prev = i;
                                if out.len() >= limit {
                                    break;
                                }
                            }
                        }
                        out
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_default())
                .collect()
        });
        let mut out: Vec<u32> = parts.into_iter().flatten().collect();
        out.truncate(limit);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntryMeta, VolumeInfo, VolumeStatus};
    use crate::store::{self, Store, WriteOp};

    fn sample() -> (tempfile::TempDir, Store, VolumeId) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("catalog.db")).unwrap();
        let info = VolumeInfo::dev_folder(r"T:\");
        let volume = store
            .write(move |w| store::insert_volume(w, &info, VolumeStatus::Indexed))
            .unwrap();
        let first = store.allocate_entry_ids(6);
        let id = |k: i64| EntryId(first.0 + k);
        let file = |size| EntryMeta {
            size,
            ..EntryMeta::default()
        };
        let ops = vec![
            WriteOp::Insert {
                id: id(0),
                volume,
                parent: None,
                name: String::new(),
                is_dir: true,
                meta: EntryMeta::default(),
            },
            WriteOp::Insert {
                id: id(1),
                volume,
                parent: Some(id(0)),
                name: "Reports".into(),
                is_dir: true,
                meta: EntryMeta::default(),
            },
            WriteOp::Insert {
                id: id(2),
                volume,
                parent: Some(id(1)),
                name: "Report 2024.PDF".into(),
                is_dir: false,
                meta: file(10),
            },
            WriteOp::Insert {
                id: id(3),
                volume,
                parent: Some(id(1)),
                name: "passport.jpg".into(),
                is_dir: false,
                meta: file(20),
            },
            WriteOp::Insert {
                id: id(4),
                volume,
                parent: Some(id(0)),
                name: "Città.txt".into(),
                is_dir: false,
                meta: file(30),
            },
            WriteOp::Insert {
                id: id(5),
                volume,
                parent: Some(id(0)),
                name: "portportport".into(),
                is_dir: false,
                meta: file(40),
            },
        ];
        let session = store.session();
        session.submit(volume, ops).unwrap();
        session.flush().unwrap();
        (dir, store, volume)
    }

    fn names_of(ix: &VolumeIndex, hits: &[u32]) -> Vec<String> {
        hits.iter()
            .map(|&i| String::from_utf8_lossy(ix.folded_name(i)).into_owned())
            .collect()
    }

    // F07-AC2: substring, without case: "port" finds "report" and "passport".
    #[test]
    fn f07_ac2_substring_without_case() {
        let (_d, store, volume) = sample();
        let ix = store.read(|c| VolumeIndex::build(c, volume)).unwrap();
        assert_eq!(ix.len(), 6);
        let needle = names::fold("PORT");
        let hits = ix.find(needle.as_bytes(), 100);
        assert_eq!(
            names_of(&ix, &hits),
            names_of(&ix, &ix.find(names::fold("port").as_bytes(), 100))
        );
        assert_eq!(hits.len(), 4, "{:?}", names_of(&ix, &hits)); // reports, report 2024, passport, portportport (once)
        assert!(ix.find(names::fold("città").as_bytes(), 10).len() == 1);
        assert!(ix.find(names::fold("CITTÀ").as_bytes(), 10).len() == 1);
        assert!(ix.find(b"zzz", 10).is_empty());
        assert!(ix.find(b"", 10).is_empty());
        assert_eq!(ix.find(names::fold("port").as_bytes(), 2).len(), 2, "limit");
        // Parents and fields.
        let pdf = hits[1];
        assert_eq!(ix.parent(pdf), hits[0]);
        assert!(ix.is_dir(hits[0]) && !ix.is_dir(pdf));
        assert_eq!(ix.size(pdf), 10);
        assert_eq!(ix.parent(ix.parent(pdf)), 0);
        assert_eq!(ix.parent(0), NO_PARENT);
        assert_eq!(
            ix.find_parallel(names::fold("port").as_bytes(), 100, 4),
            hits
        );
    }
}
