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

/// Progress of a run, in bytes to read, with the phase (`hashQuick`, `hashFull`): the progress
/// of the job that runs it.
pub type HashProgress = crate::jobs::JobProgress;

/// Errors that stop a run.
#[derive(Debug, thiserror::Error)]
pub enum HashError {
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Folder paths of a volume (relative, stored names), resolved once per folder. The work is
/// ordered by size, not by folder: the cache is emptied when it grows, so that a volume with
/// many folders does not keep them all in memory (NF3).
struct Folders<'a> {
    conn: &'a rusqlite::Connection,
    cache: HashMap<EntryId, PathBuf>,
}

/// Folders kept by [`Folders`] at most.
const FOLDER_CACHE: usize = 4096;

impl<'a> Folders<'a> {
    fn new(conn: &'a rusqlite::Connection) -> Self {
        Self {
            conn,
            cache: HashMap::new(),
        }
    }

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
        if self.cache.len() >= FOLDER_CACHE {
            self.cache.clear();
        }
        self.cache.insert(folder, path.clone());
        Ok(path)
    }

    /// Where the file is now, if the catalog still has it.
    fn file(&mut self, root: &Path, s: &HashState) -> Result<Option<PathBuf>, StoreError> {
        let Some(parent) = s.parent else {
            return Ok(None);
        };
        Ok(Some(self.path(root, parent)?.join(names::decode(&s.name))))
    }
}

/// The order of the full pass: candidate duplicates first (same size and quick hash as another
/// file), largest first; then the other files, smallest first. Ties in id order.
fn full_order(candidate: bool, size: u64, entry: EntryId) -> (bool, u64, EntryId) {
    (
        !candidate,
        if candidate { u64::MAX - size } else { size },
        entry,
    )
}

/// Hashes the files of a volume reached at `root` (see the module documentation). `pause`
/// is asked between files (on battery, user pause): when it says so the run stops cleanly.
///
/// Only the ids of the files still to do are kept in memory; each file is read from the
/// catalog when its turn comes (NF3).
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
    let stop = |report: &mut HashReport| {
        let s = cancel.is_cancelled() || pause();
        if s {
            report.interrupted = true;
        }
        s
    };
    let flush = |pending: &mut Vec<HashRecord>| -> Result<(), StoreError> {
        if pending.len() >= SAVE_EVERY {
            save(store, pending).map_err(|e| match e {
                HashError::Store(s) => s,
            })?;
        }
        Ok(())
    };
    let mut pending: Vec<HashRecord> = Vec::new();

    // Pass 1: quick hashes of the files that share their size (F06-AC4).
    let work = store.read(|c| store::quick_work(c, volume))?;
    progress.set_phase("hashQuick");
    progress.total.store(
        work.len() as u64 * (2 * QUICK_CHUNK as u64),
        Ordering::Relaxed,
    );
    store.read(|conn| {
        let mut folders = Folders::new(conn);
        for &entry in &work {
            if stop(&mut report) {
                break;
            }
            if let Some(s) = store::hash_state(conn, entry)?
                && s.quick.is_none()
                && let Some(path) = folders.file(root, &s)?
            {
                match quick_hash(&path, s.size) {
                    Ok(q) if unchanged(&path, &s) => {
                        report.quick += 1;
                        pending.push(HashRecord {
                            entry: s.entry,
                            size: s.size,
                            modified: s.modified,
                            quick: Some(q),
                            full: s.full,
                        });
                    }
                    Ok(_) => report.changed += 1,
                    Err(_) => report.unreadable += 1,
                }
            }
            progress
                .done
                .fetch_add(2 * QUICK_CHUNK as u64, Ordering::Relaxed);
            flush(&mut pending)?;
        }
        Ok(())
    })?;
    drop(work);
    save(store, &mut pending)?;
    if report.interrupted {
        return Ok(report);
    }

    // Passes 2 and 3: full hashes, candidate duplicates first, then the rest smallest first.
    let mut full_work: Vec<(bool, u64, EntryId)> = Vec::new();
    let mut total = 0u64;
    store.read(|c| {
        let shared_quick = store::shared_quick(c)?;
        store::full_work(c, volume, |entry, size, quick| {
            let candidate = quick.is_some_and(|q| shared_quick.contains(&(size, q)));
            total = total.saturating_add(size);
            full_work.push(full_order(candidate, size, entry));
        })
    })?;
    full_work.sort_unstable();
    progress.set_phase("hashFull");
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(total, Ordering::Relaxed);
    store.read(|conn| {
        let mut folders = Folders::new(conn);
        for &(_, _, entry) in &full_work {
            if stop(&mut report) {
                break;
            }
            let Some(s) = store::hash_state(conn, entry)? else {
                continue;
            };
            if s.full.is_some() {
                continue;
            }
            let Some(path) = folders.file(root, &s)? else {
                continue;
            };
            match full_hash(&path, cancel, &progress.done) {
                Ok(f) => {
                    if unchanged(&path, &s) {
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
            flush(&mut pending)?;
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
