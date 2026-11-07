//! Content hashes computed in the background (F06, ADR-0011), in three passes that put the
//! files that may have duplicates first (F06-AC4):
//!
//! 1. a **quick** hash (BLAKE3 of the size, the first and the last 64 KiB) of every file whose
//!    size is shared with another file of the catalog;
//! 2. the **full** hash (BLAKE3 of all the content) of the files whose size and quick hash are
//!    shared: the candidate duplicates;
//! 3. the full hash of all the other files (for the backup checks of T7), smallest first.
//!
//! Every hash is saved with the size and modification time read from the disk right after the
//! file was read; if they no longer match the catalog the file changed meanwhile and the hash is
//! dropped (it is computed again after the catalog update, F06-AC3). The caller runs this on a
//! thread in background mode (low CPU and I/O priority, F06-AC2).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cancel::CancelToken;
use crate::model::{EntryId, VolumeId, unix_millis_now};
use crate::names;
use crate::scan::disk_meta;
use crate::store::{self, Digest, HashRecord, HashState, Store, StoreError};

/// Bytes read at each end of a file for the quick hash.
pub const QUICK_CHUNK: usize = 64 * 1024;

const READ_BUFFER: usize = 1024 * 1024;
const SAVE_EVERY: usize = 256;

/// The quick hash of a file of `size` bytes.
///
/// # Errors
/// I/O errors.
pub fn quick_hash(path: &Path, size: u64) -> std::io::Result<Digest> {
    let mut f = File::open(path)?;
    let mut h = blake3::Hasher::new();
    h.update(&size.to_le_bytes());
    let mut buf = vec![0u8; QUICK_CHUNK];
    let n = read_full(&mut f, &mut buf)?;
    h.update(&buf[..n]);
    let chunk = QUICK_CHUNK as u64;
    if size > chunk {
        f.seek(SeekFrom::Start(size.saturating_sub(chunk).max(chunk)))?;
        let n = read_full(&mut f, &mut buf)?;
        h.update(&buf[..n]);
    }
    Ok(*h.finalize().as_bytes())
}

/// The full hash of a file; stops with `Interrupted` when `cancel` is set.
///
/// # Errors
/// I/O errors; `Interrupted` if cancelled.
pub fn full_hash(path: &Path, cancel: &CancelToken, read: &AtomicU64) -> std::io::Result<Digest> {
    let mut f = File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; READ_BUFFER];
    loop {
        if cancel.is_cancelled() {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        read.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(*h.finalize().as_bytes())
}

fn read_full(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        let n = f.read(&mut buf[total..])?;
        if n == 0 {
            break;
        }
        total += n;
    }
    Ok(total)
}

/// What a hashing run did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HashReport {
    pub quick: u64,
    pub full: u64,
    /// Files that changed while being read (hashed again later).
    pub changed: u64,
    /// Files that could not be read (in use, gone, no access).
    pub unreadable: u64,
    /// Stopped early (cancelled or paused).
    pub interrupted: bool,
}

/// Progress of a run, in bytes to read.
#[derive(Debug, Default)]
pub struct HashProgress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

/// Errors that stop a run.
#[derive(Debug, thiserror::Error)]
pub enum HashError {
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// The order of the work (F06-AC4): positions in `states` for each pass.
fn plan(states: &[HashState], shared_sizes: &std::collections::HashSet<u64>) -> Vec<usize> {
    let mut quick: Vec<usize> = (0..states.len())
        .filter(|&i| states[i].quick.is_none() && shared_sizes.contains(&states[i].size))
        .collect();
    quick.sort_by_key(|&i| std::cmp::Reverse(states[i].size));
    quick
}

/// Folder paths of a volume (relative, stored names), resolved once per folder.
struct Folders<'a> {
    conn: &'a rusqlite::Connection,
    cache: HashMap<EntryId, PathBuf>,
}

impl Folders<'_> {
    fn path(&mut self, root: &Path, folder: EntryId) -> Result<PathBuf, StoreError> {
        if let Some(p) = self.cache.get(&folder) {
            return Ok(p.clone());
        }
        let mut path = root.to_path_buf();
        for part in store::relative_path(self.conn, folder)?
            .split('\\')
            .filter(|p| !p.is_empty())
        {
            path.push(names::decode(part));
        }
        self.cache.insert(folder, path.clone());
        Ok(path)
    }
}

/// Hashes the files of a volume reached at `root` (see the module documentation). `pause`
/// is asked between files (on battery, user pause): when it says so the run stops cleanly.
///
/// # Errors
/// [`HashError`] if the catalog cannot be read or written; problems with single files are
/// counted in the report.
pub fn hash_volume(
    store: &Store,
    volume: VolumeId,
    root: &Path,
    cancel: &CancelToken,
    pause: &dyn Fn() -> bool,
    progress: &HashProgress,
) -> Result<HashReport, HashError> {
    let mut report = HashReport::default();
    let (states, shared) =
        store.read(|c| Ok((store::hash_states(c, volume)?, store::shared_sizes(c)?)))?;
    let mut states = states;

    // Pass 1: quick hashes of the files that share their size.
    let work = plan(&states, &shared);
    let quick_bytes: u64 = work.len() as u64 * (2 * QUICK_CHUNK as u64);
    progress.total.store(quick_bytes, Ordering::Relaxed);
    let mut pending: Vec<HashRecord> = Vec::new();
    let stop = |report: &mut HashReport| {
        let s = cancel.is_cancelled() || pause();
        if s {
            report.interrupted = true;
        }
        s
    };
    store.read(|conn| {
        let mut folders = Folders {
            conn,
            cache: HashMap::new(),
        };
        for i in &work {
            if stop(&mut report) {
                break;
            }
            let s = &states[*i];
            let Some(parent) = s.parent else { continue };
            let path = folders.path(root, parent)?.join(names::decode(&s.name));
            match quick_hash(&path, s.size) {
                Ok(q) => {
                    if unchanged(&path, s) {
                        report.quick += 1;
                        pending.push(HashRecord {
                            entry: s.entry,
                            size: s.size,
                            modified: s.modified,
                            quick: Some(q),
                            full: s.full,
                        });
                    } else {
                        report.changed += 1;
                    }
                }
                Err(_) => report.unreadable += 1,
            }
            progress
                .done
                .fetch_add(2 * QUICK_CHUNK as u64, Ordering::Relaxed);
        }
        Ok(())
    })?;
    for r in &pending {
        if let Some(s) = states.iter_mut().find(|s| s.entry == r.entry) {
            s.quick = r.quick;
        }
    }
    save(store, &mut pending)?;
    if report.interrupted {
        return Ok(report);
    }

    // Passes 2 and 3: full hashes, candidate duplicates first, then the rest smallest first.
    let shared_quick = store.read(store::shared_quick)?;
    let mut full_work: Vec<usize> = (0..states.len())
        .filter(|&i| states[i].full.is_none())
        .collect();
    full_work.sort_by_key(|&i| {
        let s = &states[i];
        let candidate = s.quick.is_some_and(|q| shared_quick.contains(&(s.size, q)));
        (
            !candidate,
            if candidate { u64::MAX - s.size } else { s.size },
        )
    });
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(
        full_work.iter().map(|&i| states[i].size).sum(),
        Ordering::Relaxed,
    );
    store.read(|conn| {
        let mut folders = Folders {
            conn,
            cache: HashMap::new(),
        };
        for &i in &full_work {
            if stop(&mut report) {
                break;
            }
            let s = &states[i];
            let Some(parent) = s.parent else { continue };
            let path = folders.path(root, parent)?.join(names::decode(&s.name));
            match full_hash(&path, cancel, &progress.done) {
                Ok(f) => {
                    if unchanged(&path, s) {
                        report.full += 1;
                        pending.push(HashRecord {
                            entry: s.entry,
                            size: s.size,
                            modified: s.modified,
                            quick: s.quick,
                            full: Some(f),
                        });
                    } else {
                        report.changed += 1;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    report.interrupted = true;
                    break;
                }
                Err(_) => report.unreadable += 1,
            }
            if pending.len() >= SAVE_EVERY {
                save(store, &mut pending).map_err(|e| match e {
                    HashError::Store(s) => s,
                })?;
            }
        }
        Ok(())
    })?;
    save(store, &mut pending)?;
    Ok(report)
}

/// Whether the file still has the size and modification time the catalog knows.
fn unchanged(path: &Path, s: &HashState) -> bool {
    disk_meta(path)
        .is_ok_and(|(is_dir, meta)| !is_dir && meta.size == s.size && meta.modified == s.modified)
}

fn save(store: &Store, pending: &mut Vec<HashRecord>) -> Result<(), HashError> {
    if pending.is_empty() {
        return Ok(());
    }
    let batch = std::mem::take(pending);
    let at = unix_millis_now();
    store.write(move |w| store::set_hashes(w, &batch, at))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileTime;

    fn state(id: i64, size: u64) -> HashState {
        HashState {
            entry: EntryId(id),
            parent: Some(EntryId(0)),
            name: format!("f{id}"),
            size,
            modified: Some(FileTime(1)),
            quick: None,
            full: None,
        }
    }

    // F06-AC4: files that share their size come first, largest first; the others wait for the
    // full pass.
    #[test]
    fn f06_ac4_files_that_may_be_duplicates_first() {
        let states = [
            state(1, 10),
            state(2, 500),
            state(3, 10),
            state(4, 500),
            state(5, 7),
        ];
        let shared = std::collections::HashSet::from([10, 500]);
        let order: Vec<i64> = plan(&states, &shared)
            .into_iter()
            .map(|i| states[i].entry.0)
            .collect();
        assert_eq!(order, [2, 4, 1, 3]);
    }

    #[test]
    fn quick_hash_reads_both_ends() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let c = dir.path().join("c");
        let mut data = vec![1u8; 300_000];
        std::fs::write(&a, &data).unwrap();
        data[150_000] = 2; // the middle: same quick hash, different full hash
        std::fs::write(&b, &data).unwrap();
        data[299_999] = 3; // the end: different quick hash
        std::fs::write(&c, &data).unwrap();
        let size = 300_000;
        assert_eq!(quick_hash(&a, size).unwrap(), quick_hash(&b, size).unwrap());
        assert_ne!(quick_hash(&a, size).unwrap(), quick_hash(&c, size).unwrap());
        let read = AtomicU64::new(0);
        let cancel = CancelToken::new();
        assert_ne!(
            full_hash(&a, &cancel, &read).unwrap(),
            full_hash(&b, &cancel, &read).unwrap()
        );
        assert_eq!(read.load(Ordering::Relaxed), 600_000);
        // A small file is read once.
        let small = dir.path().join("s");
        std::fs::write(&small, b"hello").unwrap();
        assert_eq!(quick_hash(&small, 5).unwrap().len(), 32);
    }
}
