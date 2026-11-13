//! Content hashes computed in the background (F06, ADR-0011), in three passes that put the
//! files that may have duplicates first (F06-AC4):
//!
//! 1. a **quick** hash (BLAKE3 of the size, the first and the last 64 KiB) of every file whose
//!    size is shared with another file of the catalog;
//! 2. the **full** hash (BLAKE3 of all the content) of the files whose size and quick hash are
//!    shared: the candidate duplicates;
//! 3. the full hash of all the other files (for the backup checks of T7), smallest first.
//!
//! Every hash is saved with the size and modification time read from the open file right after
//! it was read; if they no longer match the catalog the file changed meanwhile and the hash is
//! dropped (it is computed again after the catalog update, F06-AC3). Files modified in the last
//! minutes are left for a later run: they are probably still being written (seen in a
//! diagnostic package: the same 13 files, rewritten all the time, hashed again every 3 minutes).
//!
//! Reading (REVIEW L31; seen on a file server: 2.5 million files took 21 hours, one at a time,
//! the time going in waiting for the server for each file more than in reading it):
//!
//! - each file is opened once, and its size and time are read from the open file;
//! - files up to [`SMALL_FILE`] are read whole by the first pass, which keeps their full hash
//!   too, so the full pass does not read them again;
//! - images read whole (up to [`INLINE_PRINT_MAX`] in the full pass) are printed from the same
//!   bytes (F11), so the print pass does not read them again;
//! - [`HashOptions::readers`] files are read at the same time (several on network shares and
//!   SSDs, one on hard disks) and [`HashOptions::decoders`] images are decoded at the same time.
//!
//! The caller runs this on a thread in background mode (low CPU and I/O priority, F06-AC2); the
//! other threads enter it through [`HashOptions::init`].

use std::cell::Cell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::cancel::CancelToken;
use crate::media::{self, ImagePrint};
use crate::model::{EntryId, FileTime, VolumeId, unix_millis_now};
use crate::names;
use crate::scan::open_meta;
use crate::store::{self, Digest, HashRecord, HashState, PrintRecord, Store, StoreError};

/// Bytes read at each end of a file for the quick hash.
pub const QUICK_CHUNK: usize = 64 * 1024;
/// Files up to this size are read whole by the quick pass, which then has their full hash too:
/// on a network share, asking for a file costs more than reading a megabyte of it (REVIEW L31).
pub const SMALL_FILE: u64 = 1024 * 1024;
/// Images up to this size are printed from the bytes read for their full hash; larger ones are
/// left to the print pass, which reads them again (memory: one such image per reading thread).
pub const INLINE_PRINT_MAX: u64 = 32 * 1024 * 1024;

const READ_BUFFER: usize = 1024 * 1024;
const SAVE_EVERY: usize = 256;
/// How often a long round writes how far it got to the log (a round on a large file server
/// lasts a day, REVIEW L29).
const PROGRESS_LOG: Duration = Duration::from_secs(10 * 60);
/// Passes with at least this many files say in the log when they start.
const LOG_PASS_FROM: usize = 1000;

/// The quick hash of a file of `size` bytes.
///
/// # Errors
/// I/O errors.
pub fn quick_hash(path: &Path, size: u64) -> std::io::Result<Digest> {
    quick_from(&mut File::open(path)?, size).map(|(d, _)| d)
}

/// The quick hash of an open file of `size` bytes and the bytes read for it.
fn quick_from(f: &mut File, size: u64) -> std::io::Result<(Digest, u64)> {
    let mut h = blake3::Hasher::new();
    h.update(&size.to_le_bytes());
    let mut buf = vec![0u8; QUICK_CHUNK];
    let mut n = read_full(f, &mut buf)?;
    h.update(&buf[..n]);
    let mut read = n as u64;
    let chunk = QUICK_CHUNK as u64;
    if size > chunk {
        f.seek(SeekFrom::Start(size.saturating_sub(chunk).max(chunk)))?;
        n = read_full(f, &mut buf)?;
        h.update(&buf[..n]);
        read += n as u64;
    }
    Ok((*h.finalize().as_bytes(), read))
}

/// The quick hash of a content read whole: the same as [`quick_hash`] of a file with it.
#[must_use]
pub fn quick_of(data: &[u8]) -> Digest {
    let mut h = blake3::Hasher::new();
    h.update(&(data.len() as u64).to_le_bytes());
    h.update(&data[..QUICK_CHUNK.min(data.len())]);
    if data.len() > QUICK_CHUNK {
        let start = data.len().saturating_sub(QUICK_CHUNK).max(QUICK_CHUNK);
        h.update(&data[start..(start + QUICK_CHUNK).min(data.len())]);
    }
    *h.finalize().as_bytes()
}

/// The full hash of a file; stops with `Interrupted` when `cancel` is set.
///
/// # Errors
/// I/O errors; `Interrupted` if cancelled.
pub fn full_hash(path: &Path, cancel: &CancelToken, read: &AtomicU64) -> std::io::Result<Digest> {
    let mut f = File::open(path)?;
    hash_stream(&mut f, cancel, &|n| {
        read.fetch_add(n, Ordering::Relaxed);
    })
}

/// The full hash of an open file read in pieces; `on_read` gets the bytes of each piece.
fn hash_stream(
    f: &mut File,
    cancel: &CancelToken,
    on_read: &(dyn Fn(u64) + Sync),
) -> std::io::Result<Digest> {
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
        on_read(n as u64);
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

/// What one reading of a file gave (see [`read_file`]).
#[derive(Debug, Default)]
pub struct FileRead {
    pub quick: Option<Digest>,
    pub full: Option<Digest>,
    /// The content, when the file was read whole into memory.
    pub content: Option<Vec<u8>>,
    /// Size and modification time of the open file, read after its content.
    pub size: u64,
    pub modified: Option<FileTime>,
}

/// Reads a file the catalog knows with `size` bytes, opening it once (REVIEW L31): whole into
/// memory when it is small ([`SMALL_FILE`]) or `keep` asks it (an image to print), which gives
/// both hashes and the content; otherwise all of it in pieces for the full hash (`full`) or its
/// ends for the quick hash. A content that does not have `size` bytes gives no hash (the file
/// changed). `on_read` gets the bytes read as they come.
///
/// A file read in pieces for its full hash is first asked its size and date, and not read when
/// `unchanged` refuses them (it changed since the catalog was updated): seen in a diagnostic
/// package, three files always in use, 38 GB in all, were read to the end every 30 minutes to
/// find at the end that their date was not the one of the catalog (REVIEW L33). The small files
/// are not asked first: on a network share the question costs as much as their content.
///
/// # Errors
/// I/O errors; `Interrupted` if cancelled.
pub fn read_file(
    path: &Path,
    size: u64,
    full: bool,
    keep: bool,
    unchanged: &dyn Fn(u64, Option<FileTime>) -> bool,
    cancel: &CancelToken,
    on_read: &(dyn Fn(u64) + Sync),
) -> std::io::Result<FileRead> {
    let mut f = File::open(path)?;
    let mut out = FileRead::default();
    if full && !keep && size > SMALL_FILE {
        let (_, meta) = open_meta(&f)?;
        if !unchanged(meta.size, meta.modified) {
            out.size = meta.size;
            out.modified = meta.modified;
            return Ok(out);
        }
    }
    if size <= SMALL_FILE || keep {
        // One byte more than expected tells a file that grew, without reading all of it.
        let mut data = Vec::with_capacity(usize::try_from(size).unwrap_or(0).saturating_add(1));
        (&mut f)
            .take(size.saturating_add(1))
            .read_to_end(&mut data)?;
        on_read(data.len() as u64);
        if data.len() as u64 == size {
            out.quick = Some(quick_of(&data));
            out.full = Some(*blake3::hash(&data).as_bytes());
            out.content = Some(data);
        }
    } else if full {
        out.full = Some(hash_stream(&mut f, cancel, on_read)?);
    } else {
        let (quick, read) = quick_from(&mut f, size)?;
        on_read(read);
        out.quick = Some(quick);
    }
    let (_, meta) = open_meta(&f)?;
    out.size = meta.size;
    out.modified = meta.modified;
    Ok(out)
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
    /// Files modified too recently, left for a later run.
    pub recent: u64,
    /// Stopped early (cancelled or paused).
    pub interrupted: bool,
    /// Bytes read from the disk.
    pub bytes: u64,
    /// Images printed from the bytes read for their hashes, and those that could not be
    /// decoded (F11, REVIEW L31).
    pub printed: u64,
    pub not_printed: u64,
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

/// How a run reads the files (REVIEW L31).
pub struct HashOptions<'a> {
    /// Files read at the same time: several where the time goes in waiting for each file
    /// (network shares, SSDs), one on hard disks, where reading two files at once makes the
    /// heads jump between them.
    pub readers: usize,
    /// Images printed at the same time from the bytes read for their hashes; 0: none, the
    /// print pass prints them (the decoding is limited by the processor, REVIEW D60).
    pub decoders: usize,
    /// Run first on every thread the run starts (the app puts it in background mode).
    pub init: &'a (dyn Fn() + Sync),
}

fn nothing() {}

impl Default for HashOptions<'static> {
    /// One file at a time, no prints: as before REVIEW L31.
    fn default() -> Self {
        Self {
            readers: 1,
            decoders: 0,
            init: &nothing,
        }
    }
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

/// At most a given number of holders at a time.
struct Permits {
    free: Mutex<usize>,
    freed: Condvar,
}

impl Permits {
    fn new(n: usize) -> Self {
        Self {
            free: Mutex::new(n.max(1)),
            freed: Condvar::new(),
        }
    }

    fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        struct Release<'a>(&'a Permits);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                *self.0.free.lock().unwrap_or_else(PoisonError::into_inner) += 1;
                self.0.freed.notify_one();
            }
        }
        let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
        while *free == 0 {
            free = self
                .freed
                .wait(free)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *free -= 1;
        drop(free);
        let _release = Release(self);
        f()
    }
}

/// Runs `work` on the tasks `next` gives with `threads` threads (on the calling thread when
/// one) and hands each task with its result to `done` on the calling thread, as they finish.
/// `next` and `done` use the catalog connection of the calling thread; `next` returns `None` at
/// the end or when the run must stop. At most two tasks per thread wait to be started, so a stop
/// waits only for the files being read.
fn pipeline<T: Send, R: Send>(
    threads: usize,
    init: &(dyn Fn() + Sync),
    next: &mut dyn FnMut() -> Result<Option<T>, StoreError>,
    work: &(dyn Fn(&T) -> R + Sync),
    done: &mut dyn FnMut(T, R) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    if threads <= 1 {
        while let Some(task) = next()? {
            let r = work(&task);
            done(task, r)?;
        }
        return Ok(());
    }
    let (task_tx, task_rx) = crossbeam_channel::bounded::<T>(threads * 2);
    let (done_tx, done_rx) = crossbeam_channel::unbounded::<(T, R)>();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            let (task_rx, done_tx) = (task_rx.clone(), done_tx.clone());
            scope.spawn(move || {
                init();
                for task in task_rx {
                    let r = work(&task);
                    if done_tx.send((task, r)).is_err() {
                        break;
                    }
                }
            });
        }
        drop((task_rx, done_tx));
        let mut in_flight = 0usize;
        let mut result = Ok(());
        let mut finish = |result: &mut Result<(), StoreError>, (task, r): (T, R)| {
            if result.is_ok() {
                *result = done(task, r);
            }
        };
        loop {
            while let Ok(d) = done_rx.try_recv() {
                in_flight -= 1;
                finish(&mut result, d);
            }
            if result.is_err() {
                break;
            }
            if in_flight >= threads * 3 {
                match done_rx.recv() {
                    Ok(d) => {
                        in_flight -= 1;
                        finish(&mut result, d);
                        continue;
                    }
                    Err(_) => break,
                }
            }
            match next() {
                Ok(Some(task)) => {
                    if task_tx.send(task).is_err() {
                        break;
                    }
                    in_flight += 1;
                }
                Ok(None) => break,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        drop(task_tx);
        for d in done_rx {
            finish(&mut result, d);
        }
        result
    })
}

/// A file to read.
struct Task {
    state: HashState,
    path: PathBuf,
    /// The full pass (else the quick pass).
    full: bool,
    /// An image without a print: printed from its bytes if they are read whole.
    print: bool,
}

/// What became of a file.
enum Outcome {
    Read {
        quick: Option<Digest>,
        full: Option<Digest>,
        /// When asked and the content was read whole; `Err` if it could not be decoded.
        print: Option<Result<ImagePrint, String>>,
    },
    /// Changed since the catalog was updated, or while being read.
    Changed,
    Unreadable,
    /// Cancelled while being read, or not started because the run stops.
    Stopped,
}

/// The shared state of the threads of a run.
struct Readers<'a> {
    cancel: &'a CancelToken,
    halt: AtomicBool,
    io: Permits,
    decode: Permits,
    bytes: AtomicU64,
}

impl Readers<'_> {
    fn read(&self, task: &Task, progress: Option<&AtomicU64>) -> Outcome {
        if self.halt.load(Ordering::Relaxed) || self.cancel.is_cancelled() {
            return Outcome::Stopped;
        }
        let s = &task.state;
        let keep = task.print && task.full && s.size <= INLINE_PRINT_MAX;
        let on_read = |n: u64| {
            self.bytes.fetch_add(n, Ordering::Relaxed);
            if let Some(p) = progress {
                p.fetch_add(n, Ordering::Relaxed);
            }
        };
        let unchanged =
            |size: u64, modified: Option<FileTime>| size == s.size && modified == s.modified;
        let read = self.io.run(|| {
            read_file(
                &task.path,
                s.size,
                task.full,
                keep,
                &unchanged,
                self.cancel,
                &on_read,
            )
        });
        let read = match read {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Outcome::Stopped,
            Err(_) => return Outcome::Unreadable,
        };
        let hashed = if task.full {
            read.full.is_some()
        } else {
            read.quick.is_some()
        };
        if !hashed || read.size != s.size || read.modified != s.modified {
            return Outcome::Changed;
        }
        let print = match (&read.content, task.print) {
            (Some(data), true) => Some(
                self.decode
                    .run(|| media::print_bytes(data, &task.path).map_err(|e| e.to_string())),
            ),
            _ => None,
        };
        Outcome::Read {
            quick: read.quick,
            full: read.full,
            print,
        }
    }
}

/// Hashes the files of a volume reached at `root` one at a time (see [`hash_volume_with`]).
///
/// # Errors
/// [`HashError`] if the catalog cannot be read or written.
pub fn hash_volume(
    store: &Store,
    volume: VolumeId,
    root: &Path,
    recent: Option<Range<FileTime>>,
    cancel: &CancelToken,
    pause: &dyn Fn() -> bool,
    progress: &HashProgress,
) -> Result<HashReport, HashError> {
    hash_volume_with(
        store,
        volume,
        root,
        recent,
        cancel,
        pause,
        progress,
        &HashOptions::default(),
    )
}

/// Hashes the files of a volume reached at `root` (see the module documentation), reading them
/// as `options` says. Files whose modification time falls in `recent` (the last minutes) are
/// left for a later run. `pause` is asked between files (on battery, user pause): when it says
/// so the run stops after the files being read.
///
/// Only the ids of the files still to do are kept in memory; each file is read from the
/// catalog when its turn comes (NF3).
///
/// # Errors
/// [`HashError`] if the catalog cannot be read or written; problems with single files are
/// counted in the report.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn hash_volume_with(
    store: &Store,
    volume: VolumeId,
    root: &Path,
    recent: Option<Range<FileTime>>,
    cancel: &CancelToken,
    pause: &dyn Fn() -> bool,
    progress: &HashProgress,
    options: &HashOptions<'_>,
) -> Result<HashReport, HashError> {
    let is_recent = |s: &HashState| {
        s.modified
            .is_some_and(|m| recent.as_ref().is_some_and(|r| r.contains(&m)))
    };
    let started = Instant::now();
    let readers = Readers {
        cancel,
        halt: AtomicBool::new(false),
        io: Permits::new(options.readers),
        decode: Permits::new(options.decoders),
        bytes: AtomicU64::new(0),
    };
    let threads = options.readers.max(options.decoders).max(1);
    let inline_prints = options.decoders > 0;
    let wants_print =
        |s: &HashState| inline_prints && !s.printed && media::is_image_to_print(&s.name, s.size);
    let mut report = HashReport::default();
    let interrupted = Cell::new(false);
    let recent_files = Cell::new(0u64);
    // Asked between files, on this thread only (`pause` may not be shared).
    let stop = || {
        let s = readers.halt.load(Ordering::Relaxed) || cancel.is_cancelled() || pause();
        if s {
            interrupted.set(true);
            readers.halt.store(true, Ordering::Relaxed);
        }
        s
    };
    let mut hashes: Vec<HashRecord> = Vec::new();
    let mut prints: Vec<PrintRecord> = Vec::new();
    let mut last_log = Instant::now();
    let log_pass = |phase: &str, files: usize| {
        if files >= LOG_PASS_FROM {
            tracing::info!(
                volume = volume.0,
                phase,
                files,
                readers = options.readers,
                decoders = options.decoders,
                "hashing pass started"
            );
        }
    };

    // Pass 1: quick hashes of the files that share their size (F06-AC4).
    let work = store.read(|c| store::quick_work(c, volume))?;
    progress.set_phase("hashQuick");
    progress.total.store(
        work.len() as u64 * (2 * QUICK_CHUNK as u64),
        Ordering::Relaxed,
    );
    log_pass("hashQuick", work.len());
    store.read(|conn| {
        let mut folders = Folders::new(conn);
        let mut items = work.iter().enumerate();
        let taken = Cell::new(0usize);
        let mut next = || -> Result<Option<Task>, StoreError> {
            loop {
                if stop() {
                    return Ok(None);
                }
                let Some((i, &entry)) = items.next() else {
                    return Ok(None);
                };
                taken.set(i + 1);
                // Recent files are counted once, by the full pass.
                if let Some(s) = store::hash_state(conn, entry)?
                    && s.quick.is_none()
                    && !is_recent(&s)
                    && let Some(path) = folders.file(root, &s)?
                {
                    let print = wants_print(&s);
                    return Ok(Some(Task {
                        state: s,
                        path,
                        full: false,
                        print,
                    }));
                }
                progress
                    .done
                    .fetch_add(2 * QUICK_CHUNK as u64, Ordering::Relaxed);
            }
        };
        let mut done = |task: Task, outcome: Outcome| -> Result<(), StoreError> {
            let s = &task.state;
            match outcome {
                Outcome::Read { quick, full, print } => {
                    report.quick += 1;
                    if full.is_some() && s.full.is_none() {
                        report.full += 1;
                    }
                    hashes.push(HashRecord {
                        entry: s.entry,
                        size: s.size,
                        modified: s.modified,
                        quick,
                        full: full.or(s.full),
                    });
                    if let Some(p) = print {
                        prints.push(print_record(&mut report, s, &task.path, p));
                    }
                }
                Outcome::Changed => report.changed += 1,
                Outcome::Unreadable => report.unreadable += 1,
                Outcome::Stopped => {
                    interrupted.set(true);
                    return Ok(());
                }
            }
            progress
                .done
                .fetch_add(2 * QUICK_CHUNK as u64, Ordering::Relaxed);
            log_progress(
                &mut last_log,
                volume,
                "hashQuick",
                (taken.get(), work.len()),
                &report,
                &readers.bytes,
                started,
            );
            save_some(store, &mut hashes, &mut prints, SAVE_EVERY)
        };
        let read = |task: &Task| readers.read(task, None);
        pipeline(threads, options.init, &mut next, &read, &mut done)
    })?;
    drop(work);
    save_some(store, &mut hashes, &mut prints, 1)?;
    if interrupted.get() {
        return Ok(finish(report, &readers, true, recent_files.get()));
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
    log_pass("hashFull", full_work.len());
    store.read(|conn| {
        let mut folders = Folders::new(conn);
        let mut items = full_work.iter().enumerate();
        let taken = Cell::new(0usize);
        let mut next = || -> Result<Option<Task>, StoreError> {
            loop {
                if stop() {
                    return Ok(None);
                }
                let Some((i, &(_, _, entry))) = items.next() else {
                    return Ok(None);
                };
                taken.set(i + 1);
                let Some(s) = store::hash_state(conn, entry)? else {
                    continue;
                };
                if s.full.is_some() {
                    continue;
                }
                if is_recent(&s) {
                    recent_files.set(recent_files.get() + 1);
                    continue;
                }
                let Some(path) = folders.file(root, &s)? else {
                    continue;
                };
                let print = wants_print(&s);
                return Ok(Some(Task {
                    state: s,
                    path,
                    full: true,
                    print,
                }));
            }
        };
        let mut done = |task: Task, outcome: Outcome| -> Result<(), StoreError> {
            let s = &task.state;
            match outcome {
                Outcome::Read { quick, full, print } => {
                    report.full += 1;
                    hashes.push(HashRecord {
                        entry: s.entry,
                        size: s.size,
                        modified: s.modified,
                        quick: s.quick.or(quick),
                        full,
                    });
                    if let Some(p) = print {
                        prints.push(print_record(&mut report, s, &task.path, p));
                    }
                }
                Outcome::Changed => report.changed += 1,
                Outcome::Unreadable => report.unreadable += 1,
                Outcome::Stopped => {
                    interrupted.set(true);
                    readers.halt.store(true, Ordering::Relaxed);
                    return Ok(());
                }
            }
            log_progress(
                &mut last_log,
                volume,
                "hashFull",
                (taken.get(), full_work.len()),
                &report,
                &readers.bytes,
                started,
            );
            save_some(store, &mut hashes, &mut prints, SAVE_EVERY)
        };
        let read = |task: &Task| readers.read(task, Some(&progress.done));
        pipeline(threads, options.init, &mut next, &read, &mut done)
    })?;
    save_some(store, &mut hashes, &mut prints, 1)?;
    Ok(finish(
        report,
        &readers,
        interrupted.get(),
        recent_files.get(),
    ))
}

fn finish(
    mut report: HashReport,
    readers: &Readers<'_>,
    interrupted: bool,
    recent: u64,
) -> HashReport {
    report.interrupted = interrupted;
    report.recent = recent;
    report.bytes = readers.bytes.load(Ordering::Relaxed);
    report
}

/// The print of an image read for its hashes, counted in the report.
fn print_record(
    report: &mut HashReport,
    s: &HashState,
    path: &Path,
    print: Result<ImagePrint, String>,
) -> PrintRecord {
    let print = match print {
        Ok(p) => {
            report.printed += 1;
            Some(p)
        }
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "not printed");
            report.not_printed += 1;
            None
        }
    };
    PrintRecord {
        entry: s.entry,
        size: s.size,
        modified: s.modified,
        dhash: print.map(|p| p.dhash),
        width: print.map(|p| p.width),
        height: print.map(|p| p.height),
    }
}

/// Every [`PROGRESS_LOG`], how far a long round got.
fn log_progress(
    last: &mut Instant,
    volume: VolumeId,
    phase: &str,
    (position, files): (usize, usize),
    report: &HashReport,
    bytes: &AtomicU64,
    started: Instant,
) {
    if last.elapsed() < PROGRESS_LOG {
        return;
    }
    *last = Instant::now();
    tracing::info!(
        volume = volume.0,
        phase,
        position,
        files,
        quick = report.quick,
        full = report.full,
        printed = report.printed,
        mb = bytes.load(Ordering::Relaxed) / 1_000_000,
        minutes = started.elapsed().as_secs() / 60,
        "hashing progress"
    );
}

/// Saves the hashes and prints computed so far once there are at least `at_least` of either,
/// in one transaction.
fn save_some(
    store: &Store,
    hashes: &mut Vec<HashRecord>,
    prints: &mut Vec<PrintRecord>,
    at_least: usize,
) -> Result<(), StoreError> {
    if hashes.len().max(prints.len()) < at_least.max(1) {
        return Ok(());
    }
    let (h, p) = (std::mem::take(hashes), std::mem::take(prints));
    let at = unix_millis_now();
    store.write(move |w| {
        store::set_hashes(w, &h, at)?;
        store::set_prints(w, &p, at)
    })
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

    // REVIEW L31: one reading gives the same hashes as the separate ones, at every size around
    // the ends of the quick hash and the limit of the small files.
    #[test]
    fn one_reading_gives_the_same_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancelToken::new();
        let chunk = QUICK_CHUNK as u64;
        let small = SMALL_FILE;
        for size in [
            1,
            chunk - 1,
            chunk,
            chunk + 1,
            2 * chunk - 1,
            2 * chunk,
            2 * chunk + 1,
            300_000,
            small,
            small + 1,
            3 * small + 7,
        ] {
            let path = dir.path().join(format!("f{size}"));
            let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            std::fs::write(&path, &data).unwrap();
            let quick = quick_hash(&path, size).unwrap();
            let full = full_hash(&path, &cancel, &AtomicU64::new(0)).unwrap();
            assert_eq!(quick_of(&data), quick, "quick of the content, {size}");
            let read_bytes = AtomicU64::new(0);
            let count = |n: u64| {
                read_bytes.fetch_add(n, Ordering::Relaxed);
            };
            // Quick pass: small files read whole, with their full hash.
            let r = read_file(&path, size, false, false, &|_, _| true, &cancel, &count).unwrap();
            assert_eq!(r.quick, Some(quick), "{size}");
            assert_eq!(r.full, (size <= small).then_some(full), "{size}");
            assert_eq!(r.content.is_some(), size <= small, "{size}");
            assert_eq!((r.size, r.modified.is_some()), (size, true));
            // Full pass, and an image kept whole for its print.
            let r = read_file(&path, size, true, false, &|_, _| true, &cancel, &count).unwrap();
            assert_eq!(r.full, Some(full), "{size}");
            let r = read_file(&path, size, true, true, &|_, _| true, &cancel, &count).unwrap();
            assert_eq!((r.quick, r.full), (Some(quick), Some(full)), "{size}");
            assert_eq!(r.content.as_deref(), Some(&data[..]));
            assert!(read_bytes.load(Ordering::Relaxed) >= 2 * size.min(2 * chunk));
        }
        // A file that is not as large as the catalog says gives no hash.
        let path = dir.path().join("f1");
        let r = read_file(&path, 2, false, false, &|_, _| true, &cancel, &|_| {}).unwrap();
        assert_eq!((r.quick, r.full, r.size), (None, None, 1));
        // REVIEW L33: a file read in pieces that no longer has the size and date of the catalog
        // is not read at all; a small one is read (asking first would cost as much).
        let large = 3 * small + 7;
        let read_bytes = AtomicU64::new(0);
        let count = |n: u64| {
            read_bytes.fetch_add(n, Ordering::Relaxed);
        };
        let path = dir.path().join(format!("f{large}"));
        let asked = Cell::new((0u64, false));
        let changed = |size: u64, modified: Option<FileTime>| {
            asked.set((size, modified.is_some()));
            false
        };
        let r = read_file(&path, large, true, false, &changed, &cancel, &count).unwrap();
        assert_eq!((r.full, r.size, r.modified.is_some()), (None, large, true));
        assert_eq!(asked.get(), (large, true));
        assert_eq!(read_bytes.load(Ordering::Relaxed), 0);
        let path = dir.path().join(format!("f{small}"));
        let r = read_file(&path, small, true, false, &changed, &cancel, &count).unwrap();
        assert!(r.full.is_some());
        assert_eq!(read_bytes.load(Ordering::Relaxed), small);
    }

    #[test]
    fn the_pipeline_hands_every_result_back_on_the_calling_thread() {
        for threads in [1, 3] {
            let mut items = 0..100u32;
            let caller = std::thread::current().id();
            let mut seen = Vec::new();
            pipeline(
                threads,
                &nothing,
                &mut || Ok(items.next()),
                &|t| t * 2,
                &mut |t, r| {
                    assert_eq!(std::thread::current().id(), caller);
                    seen.push((t, r));
                    Ok(())
                },
            )
            .unwrap();
            seen.sort_unstable();
            assert_eq!(seen, (0..100).map(|t| (t, t * 2)).collect::<Vec<_>>());
        }
        // An error of the catalog stops it; the tasks started still end.
        let mut items = 0..100u32;
        let result = pipeline(
            4,
            &nothing,
            &mut || Ok(items.next()),
            &|t: &u32| *t,
            &mut |t, _| {
                if t == 10 {
                    Err(StoreError::NotFound("x".to_owned()))
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn permits_let_a_few_run_at_once() {
        let permits = Permits::new(2);
        let running = AtomicU64::new(0);
        let most = AtomicU64::new(0);
        std::thread::scope(|s| {
            for _ in 0..6 {
                s.spawn(|| {
                    permits.run(|| {
                        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(20));
                        running.fetch_sub(1, Ordering::SeqCst);
                    });
                });
            }
        });
        assert_eq!(most.load(Ordering::SeqCst), 2);
    }
}
