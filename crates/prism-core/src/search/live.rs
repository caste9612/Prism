//! The indexes in use, kept in step with the catalog (ADR-0010): a volume's index is rebuilt
//! when its catalog changes, at most every `min_interval` (REVIEW L11), on a thread of its own.
//! Searches read a snapshot and never wait for a rebuild.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, Instant};

use crate::model::VolumeId;
use crate::store::{self, Store, StoreEvent};

use super::index::VolumeIndex;

/// The current index of each volume.
#[derive(Default)]
pub struct LiveIndexes {
    map: RwLock<HashMap<VolumeId, Arc<VolumeIndex>>>,
}

impl LiveIndexes {
    /// The indexes as they are now.
    pub fn snapshot(&self) -> Vec<Arc<VolumeIndex>> {
        self.map
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    fn put(&self, ix: VolumeIndex) {
        self.map
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(ix.volume, Arc::new(ix));
    }

    fn remove(&self, volume: VolumeId) {
        self.map
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&volume);
    }

    fn has(&self, volume: VolumeId) -> bool {
        self.map
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&volume)
    }

    /// Builds the index of every volume with entries, then follows the catalog's events until
    /// `stop` is set: changed volumes are rebuilt (a new volume at once, the others at most
    /// every `min_interval`), removed ones dropped.
    pub fn maintain(&self, store: &Store, min_interval: Duration, stop: &AtomicBool) {
        let events = store.subscribe();
        let mut dirty: HashMap<VolumeId, ()> = HashMap::new();
        let mut built: HashMap<VolumeId, Instant> = HashMap::new();
        if let Ok(volumes) = store.read(store::volumes) {
            for v in volumes {
                dirty.insert(v.id, ());
            }
        }
        while !stop.load(Ordering::Relaxed) {
            match events.recv_timeout(Duration::from_millis(250)) {
                Ok(StoreEvent::CatalogChanged { volume }) => {
                    dirty.insert(volume, ());
                }
                Ok(StoreEvent::VolumeRemoved { volume }) => {
                    dirty.remove(&volume);
                    built.remove(&volume);
                    self.remove(volume);
                }
                Ok(StoreEvent::VolumeChanged { .. })
                | Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            }
            let due: Vec<VolumeId> = dirty
                .keys()
                .filter(|v| {
                    !self.has(**v) || built.get(v).is_none_or(|t| t.elapsed() >= min_interval)
                })
                .copied()
                .collect();
            for volume in due {
                dirty.remove(&volume);
                let started = Instant::now();
                match store.read(|c| {
                    if store::volume_has_entries(c, volume)? {
                        VolumeIndex::build(c, volume).map(Some)
                    } else {
                        Ok(None)
                    }
                }) {
                    Ok(Some(ix)) => {
                        tracing::debug!(
                            volume = volume.0,
                            entries = ix.len(),
                            ms = started.elapsed().as_millis(),
                            "search index rebuilt"
                        );
                        self.put(ix);
                        built.insert(volume, Instant::now());
                    }
                    Ok(None) => self.remove(volume),
                    Err(e) => tracing::warn!(volume = volume.0, error = %e, "search index"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntryId, EntryMeta, VolumeInfo, VolumeStatus};
    use crate::store::WriteOp;

    fn add(store: &Store, volume: VolumeId, parent: Option<EntryId>, name: &str) -> EntryId {
        let id = store.allocate_entry_ids(1);
        let session = store.session();
        session
            .submit(
                volume,
                vec![WriteOp::Insert {
                    id,
                    volume,
                    parent,
                    name: name.to_owned(),
                    is_dir: parent.is_none(),
                    meta: EntryMeta::default(),
                }],
            )
            .unwrap();
        session.flush().unwrap();
        id
    }

    fn wait_for(what: &str, cond: impl Fn() -> bool) {
        let start = Instant::now();
        while !cond() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn indexes_follow_the_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("catalog.db")).unwrap();
        let info = VolumeInfo::dev_folder(r"T:\");
        let volume = store
            .write(move |w| store::insert_volume(w, &info, VolumeStatus::Indexed))
            .unwrap();
        let root = add(&store, volume, None, "");
        add(&store, volume, Some(root), "first.txt");

        let live = Arc::new(LiveIndexes::default());
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (live, store, stop) = (Arc::clone(&live), store.clone(), Arc::clone(&stop));
            std::thread::spawn(move || live.maintain(&store, Duration::ZERO, &stop))
        };
        let len = |live: &LiveIndexes| live.snapshot().first().map_or(0, |ix| ix.len());
        wait_for("initial build", || len(&live) == 2);

        add(&store, volume, Some(root), "second.txt");
        wait_for("rebuild after a change", || len(&live) == 3);

        store
            .write(move |w| store::remove_volume(w, volume))
            .unwrap();
        wait_for("removal", || live.snapshot().is_empty());

        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
    }
}
