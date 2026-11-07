//! The catalog store: SQLite in WAL mode with **one writer thread** and a **pool of read-only
//! connections** (ADR-0008). There is no global lock: readers never wait for the writer.
//!
//! - [`Store::write`] runs a closure in its own transaction on the writer thread and returns
//!   its result after the commit. It uses a priority lane, so interactive writes are not
//!   queued behind bulk batches.
//! - [`WriteSession`] submits batches of [`WriteOp`] asynchronously (bounded queue, so a fast
//!   producer is slowed down instead of filling memory); [`WriteSession::flush`] waits for
//!   them and reports the first error.
//! - [`Store::read`] runs a closure on a read-only connection on the calling thread.
//! - [`Store::subscribe`] delivers [`StoreEvent`]s published after each commit.

mod catalog;
mod check;
mod hashes;
mod ops;
pub mod schema;

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

pub use catalog::*;
pub use check::{SnapshotEntry, check_consistency, snapshot};
pub use hashes::*;
pub use ops::WriteOp;

use crate::model::{EntryId, VolumeId};

/// Errors of the catalog store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(
        "the catalog was created by a newer version of Prism (schema {found}, this build supports {supported})"
    )]
    TooNew { found: i64, supported: i64 },
    #[error("cannot create the catalog folder {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the catalog failed the integrity check: {0}")]
    Corrupt(String),
    #[error("the catalog writer has stopped")]
    WriterGone,
    #[error("an earlier batch of this write session failed")]
    SessionFailed,
    #[error("not found: {0}")]
    NotFound(String),
}

/// Notifications published after a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreEvent {
    /// Entries of a volume changed.
    CatalogChanged { volume: VolumeId },
    /// A volume record (status, labels, information) changed or was added.
    VolumeChanged { volume: VolumeId },
    /// A volume was removed from the catalog.
    VolumeRemoved { volume: VolumeId },
}

type WriterJob = Box<dyn FnOnce(&mut Connection) + Send>;

/// Bulk batches waiting for the writer; producers block when the queue is full.
const BULK_QUEUE: usize = 4;
const BUSY_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_IDLE_READERS: usize = 8;

/// Handle to the catalog store. Cheap to clone; the writer stops when the last clone is dropped.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    prio_tx: Option<Sender<WriterJob>>,
    bulk_tx: Option<Sender<WriterJob>>,
    writer: Option<JoinHandle<()>>,
    readers: Mutex<Vec<Connection>>,
    next_entry_id: AtomicI64,
    events: Arc<EventHub>,
}

#[derive(Default)]
struct EventHub {
    subscribers: Mutex<Vec<Sender<StoreEvent>>>,
}

impl EventHub {
    fn publish(&self, events: Vec<StoreEvent>) {
        if events.is_empty() {
            return;
        }
        let mut subs = self
            .subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        subs.retain(|s| events.iter().all(|e| s.send(e.clone()).is_ok()));
    }
}

/// A write transaction on the writer thread, with the events to publish after the commit.
pub struct WriteTx<'c> {
    tx: Transaction<'c>,
    events: Vec<StoreEvent>,
}

impl WriteTx<'_> {
    /// The connection of the transaction (also usable for reads, which see its own writes).
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.tx
    }

    /// Queues an event, published only if the transaction commits.
    pub fn notify(&mut self, event: StoreEvent) {
        if !self.events.contains(&event) {
            self.events.push(event);
        }
    }
}

fn run_in_tx<T>(
    conn: &mut Connection,
    f: impl FnOnce(&mut WriteTx<'_>) -> Result<T, StoreError>,
) -> Result<(T, Vec<StoreEvent>), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut wtx = WriteTx {
        tx,
        events: Vec::new(),
    };
    let value = f(&mut wtx)?;
    let WriteTx { tx, events } = wtx;
    tx.commit()?;
    Ok((value, events))
}

fn configure_writer(conn: &Connection) -> Result<(), StoreError> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    let mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Corrupt(format!(
            "cannot enable WAL mode (journal mode is {mode})"
        )));
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "cache_size", -65_536)?; // 64 MiB
    Ok(())
}

fn open_reader(path: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "cache_size", -16_384)?; // 16 MiB
    Ok(conn)
}

const SESSION_KEY: &str = "session_open";

impl Store {
    /// Opens (or creates) the catalog at `path`, applies pending migrations and recovers from
    /// an unclean shutdown: volumes left in `indexing` become `reindex_needed` (F04-AC6, NF4)
    /// and the database is integrity-checked.
    ///
    /// # Errors
    /// I/O errors creating the folder, SQLite errors, a schema newer than this build, or a
    /// failed integrity check.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|source| StoreError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        let mut conn = Connection::open(path)?;
        configure_writer(&conn)?;
        schema::migrate(&mut conn)?;

        let unclean: bool = conn
            .query_row(
                "SELECT value = '1' FROM settings WHERE key = ?1",
                [SESSION_KEY],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if unclean {
            tracing::warn!("previous session did not close cleanly; checking catalog integrity");
            let result: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
            if result != "ok" {
                return Err(StoreError::Corrupt(result));
            }
        }
        let interrupted = conn.execute(
            "UPDATE volumes SET status = 'reindex_needed' WHERE status = 'indexing'",
            [],
        )?;
        if interrupted > 0 {
            tracing::warn!(
                volumes = interrupted,
                "interrupted indexing found; volumes marked for reindexing"
            );
        }
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, '1') \
             ON CONFLICT (key) DO UPDATE SET value = '1'",
            [SESSION_KEY],
        )?;
        let next_id: i64 =
            conn.query_row("SELECT coalesce(max(id), 0) + 1 FROM entries", [], |r| {
                r.get(0)
            })?;

        let (prio_tx, prio_rx) = crossbeam_channel::unbounded::<WriterJob>();
        let (bulk_tx, bulk_rx) = crossbeam_channel::bounded::<WriterJob>(BULK_QUEUE);
        let writer = std::thread::Builder::new()
            .name("prism-store-writer".into())
            .spawn(move || writer_loop(conn, &prio_rx, &bulk_rx))
            .map_err(|source| StoreError::Io {
                path: path.to_path_buf(),
                source,
            })?;

        Ok(Self {
            inner: Arc::new(Inner {
                path: path.to_path_buf(),
                prio_tx: Some(prio_tx),
                bulk_tx: Some(bulk_tx),
                writer: Some(writer),
                readers: Mutex::new(Vec::new()),
                next_entry_id: AtomicI64::new(next_id),
                events: Arc::new(EventHub::default()),
            }),
        })
    }

    /// Path of the database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Reserves `count` consecutive entry ids. Ids are never reused, even if not inserted.
    #[must_use]
    pub fn allocate_entry_ids(&self, count: u32) -> EntryId {
        EntryId(
            self.inner
                .next_entry_id
                .fetch_add(i64::from(count), Ordering::Relaxed),
        )
    }

    /// Runs `f` in a write transaction on the writer thread and returns its result after the
    /// commit. If `f` fails, the transaction is rolled back.
    ///
    /// # Errors
    /// The error returned by `f`, SQLite errors, or [`StoreError::WriterGone`].
    pub fn write<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&mut WriteTx<'_>) -> Result<T, StoreError> + Send + 'static,
    {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        let hub = Arc::clone(&self.inner.events);
        self.send(
            false,
            Box::new(move |conn| match run_in_tx(conn, f) {
                Ok((value, events)) => {
                    hub.publish(events);
                    let _ = reply_tx.send(Ok(value));
                }
                Err(e) => {
                    let _ = reply_tx.send(Err(e));
                }
            }),
        )?;
        reply_rx.recv().map_err(|_| StoreError::WriterGone)?
    }

    /// Starts a session for submitting batches of operations asynchronously.
    #[must_use]
    pub fn session(&self) -> WriteSession {
        WriteSession {
            store: self.clone(),
            state: Arc::new(Mutex::new(SessionState::default())),
        }
    }

    /// Runs `f` on a read-only connection. Reads see the state at the last commit and never
    /// wait for the writer.
    ///
    /// # Errors
    /// The error returned by `f` or SQLite errors opening the connection.
    pub fn read<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let conn = self
            .inner
            .readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop();
        let conn = match conn {
            Some(c) => c,
            None => open_reader(&self.inner.path)?,
        };
        let result = f(&conn);
        let mut pool = self
            .inner
            .readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if pool.len() < MAX_IDLE_READERS {
            pool.push(conn);
        }
        result
    }

    /// Subscribes to the events published after each commit.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<StoreEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        self.inner
            .events
            .subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(tx);
        rx
    }

    fn send(&self, bulk: bool, job: WriterJob) -> Result<(), StoreError> {
        let lane = if bulk {
            &self.inner.bulk_tx
        } else {
            &self.inner.prio_tx
        };
        lane.as_ref()
            .ok_or(StoreError::WriterGone)?
            .send(job)
            .map_err(|_| StoreError::WriterGone)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        if let Some(tx) = &self.prio_tx {
            let _ = tx.send(Box::new(|conn: &mut Connection| {
                let _ = conn.execute(
                    "UPDATE settings SET value = '0' WHERE key = ?1",
                    [SESSION_KEY],
                );
                let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            }));
        }
        self.prio_tx = None;
        self.bulk_tx = None;
        if let Some(handle) = self.writer.take() {
            let _ = handle.join();
        }
    }
}

fn run_job(conn: &mut Connection, job: WriterJob) {
    // A panicking job must not take the writer down: its transaction is rolled back when the
    // `Transaction` is dropped during unwinding, and its caller sees `WriterGone`.
    if std::panic::catch_unwind(AssertUnwindSafe(|| job(conn))).is_err() {
        tracing::error!("a catalog write job panicked; its transaction was rolled back");
    }
}

fn writer_loop(mut conn: Connection, prio: &Receiver<WriterJob>, bulk: &Receiver<WriterJob>) {
    loop {
        // Interactive writes first: a bulk batch waits at most for one other job.
        if let Ok(job) = prio.try_recv() {
            run_job(&mut conn, job);
            continue;
        }
        let job = crossbeam_channel::select! {
            recv(prio) -> msg => msg,
            recv(bulk) -> msg => msg,
        };
        match job {
            Ok(job) => run_job(&mut conn, job),
            Err(_) => break,
        }
    }
    // Drain whatever is left (the final checkpoint job, late batches).
    for job in prio.try_iter().chain(bulk.try_iter()) {
        run_job(&mut conn, job);
    }
}

/// A stream of operation batches submitted asynchronously to the writer.
///
/// Each batch is applied in its own transaction, in submission order. After the first failing
/// batch, the following batches of the same session are skipped (they may depend on it) and
/// [`WriteSession::flush`] returns the error.
pub struct WriteSession {
    store: Store,
    state: Arc<Mutex<SessionState>>,
}

#[derive(Default)]
struct SessionState {
    failed: bool,
    error: Option<StoreError>,
}

impl WriteSession {
    /// Queues a batch. Blocks while the writer queue is full.
    ///
    /// # Errors
    /// The error of an earlier batch of this session, or [`StoreError::WriterGone`].
    pub fn submit(&self, volume: VolumeId, ops: Vec<WriteOp>) -> Result<(), StoreError> {
        self.check()?;
        if ops.is_empty() {
            return Ok(());
        }
        let slot = Arc::clone(&self.state);
        let hub = Arc::clone(&self.store.inner.events);
        self.store.send(
            true,
            Box::new(move |conn| {
                if slot.lock().unwrap_or_else(PoisonError::into_inner).failed {
                    return;
                }
                let result = run_in_tx(conn, |wtx| {
                    for op in &ops {
                        ops::apply(wtx.conn(), op)?;
                    }
                    wtx.notify(StoreEvent::CatalogChanged { volume });
                    Ok(())
                });
                match result {
                    Ok(((), events)) => hub.publish(events),
                    Err(e) => {
                        tracing::error!(error = %e, "catalog batch failed");
                        let mut state = slot.lock().unwrap_or_else(PoisonError::into_inner);
                        state.failed = true;
                        state.error = Some(e);
                    }
                }
            }),
        )
    }

    /// Waits until every batch submitted so far has been applied.
    ///
    /// # Errors
    /// The first error of this session, or [`StoreError::WriterGone`].
    pub fn flush(&self) -> Result<(), StoreError> {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        self.store.send(
            true,
            Box::new(move |_conn| {
                let _ = reply_tx.send(());
            }),
        )?;
        reply_rx.recv().map_err(|_| StoreError::WriterGone)?;
        self.check()
    }

    /// Returns the session error (the first time, the original error; afterwards
    /// [`StoreError::SessionFailed`]).
    fn check(&self) -> Result<(), StoreError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.failed {
            return Err(state.error.take().unwrap_or(StoreError::SessionFailed));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
