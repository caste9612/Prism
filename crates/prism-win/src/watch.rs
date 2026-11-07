//! Folder watcher for volumes without a change journal (FAT, exFAT, ReFS, network shares;
//! ADR-0007): `ReadDirectoryChangesW` through the `notify` crate, with events grouped into
//! batches of changed paths. The catalog is then updated with
//! `prism_core::refresh::refresh_folders`.
//!
//! When Windows reports that notifications were lost (buffer overflow), or a path cannot be
//! placed under the watched root, the batch asks for a full rescan.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use prism_core::names;

/// Changes collected over a short period.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchBatch {
    /// Changed paths, as components relative to the watched root (stored names).
    pub paths: Vec<Vec<String>>,
    /// Notifications were lost: the whole root must be enumerated again.
    pub rescan: bool,
}

/// Errors of the watcher.
#[derive(Debug, thiserror::Error)]
#[error("cannot watch {path}: {source}")]
pub struct WatchError {
    pub path: PathBuf,
    pub source: notify::Error,
}

/// Watches a folder tree until dropped.
pub struct FolderWatcher {
    _watcher: RecommendedWatcher,
    events: Receiver<notify::Result<Event>>,
    root: PathBuf,
}

/// Longest time a batch keeps growing while events keep arriving.
const MAX_BATCH: Duration = Duration::from_secs(5);

impl FolderWatcher {
    /// Starts watching `root` and everything below it.
    ///
    /// # Errors
    /// [`WatchError`] if the folder cannot be watched.
    pub fn start(root: &Path) -> Result<Self, WatchError> {
        let err = |source| WatchError {
            path: root.to_path_buf(),
            source,
        };
        let (tx, events) = channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .map_err(err)?;
        watcher.watch(root, RecursiveMode::Recursive).map_err(err)?;
        Ok(Self {
            _watcher: watcher,
            events,
            root: root.to_path_buf(),
        })
    }

    /// Waits up to `wait` for a change, then keeps collecting until no event arrives for
    /// `quiet` (at most a few seconds). `None` if nothing changed.
    #[must_use]
    pub fn next_batch(&self, wait: Duration, quiet: Duration) -> Option<WatchBatch> {
        let first = self.events.recv_timeout(wait).ok()?;
        let started = Instant::now();
        let mut batch = WatchBatch::default();
        self.add(&mut batch, first);
        while started.elapsed() < MAX_BATCH {
            match self.events.recv_timeout(quiet) {
                Ok(event) => self.add(&mut batch, event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    batch.rescan = true;
                    break;
                }
            }
        }
        Some(batch)
    }

    fn add(&self, batch: &mut WatchBatch, event: notify::Result<Event>) {
        let Ok(event) = event else {
            batch.rescan = true;
            return;
        };
        if event.need_rescan() {
            batch.rescan = true;
        }
        for path in event.paths {
            match path.strip_prefix(&self.root) {
                Ok(rel) => {
                    let components: Vec<String> = rel
                        .components()
                        .map(|c| names::encode(c.as_os_str()))
                        .collect();
                    if !components.is_empty() && !batch.paths.contains(&components) {
                        batch.paths.push(components);
                    }
                }
                Err(_) => batch.rescan = true,
            }
        }
    }
}
