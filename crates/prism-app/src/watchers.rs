//! Live updates of indexed volumes without a change journal Prism can use (REVIEW L6): FAT,
//! exFAT and ReFS volumes and network shares. A folder watcher (`prism-win::watch`) collects
//! the changed paths and a `refresh` job re-reads the affected folders
//! (`prism-core::refresh`, ADR-0007).
//!
//! NTFS volumes are kept up to date from the USN journal instead (`monitor::journal_tick`);
//! when Prism is not elevated (development builds only) they are not updated live at all,
//! rather than watching a whole system drive folder by folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use prism_core::model::{VolumeId, VolumeInfo, VolumeKind, VolumeStatus};
use prism_core::names;
use prism_win::watch::FolderWatcher;

use crate::state::{AppState, access_path, auto_update};

/// Beyond this many pending paths a refresh re-reads the whole volume.
const MAX_PENDING: usize = 10_000;

/// Whether a connected volume is kept up to date by a folder watcher.
pub fn needs_watcher(info: &VolumeInfo, status: VolumeStatus) -> bool {
    if !(auto_update(status) || status == VolumeStatus::Indexing) {
        return false;
    }
    match info.kind {
        VolumeKind::Network => true,
        _ => !info
            .fs_type
            .as_deref()
            .is_some_and(|f| f.eq_ignore_ascii_case("NTFS")),
    }
}

struct Running {
    root: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The running watchers, by volume.
#[derive(Default)]
pub struct Watchers {
    running: HashMap<VolumeId, Running>,
    /// Volumes whose watcher could not start, with the root tried: not retried until the
    /// volume comes back or moves.
    failed: HashMap<VolumeId, PathBuf>,
}

impl Watchers {
    /// Starts and stops watchers so that exactly the volumes that need one have one.
    pub fn sync(&mut self, state: &Arc<AppState>) {
        let Ok(known) = state.store.read(prism_core::store::volumes) else {
            return;
        };
        let wanted: HashMap<VolumeId, PathBuf> = {
            let online = state.online();
            known
                .iter()
                .filter_map(|v| {
                    let d = online.get(&v.id)?;
                    if !needs_watcher(&d.info, v.status) {
                        return None;
                    }
                    Some((v.id, access_path(d)?))
                })
                .collect()
        };
        self.running
            .retain(|id, r| wanted.get(id).is_some_and(|root| *root == r.root));
        self.failed
            .retain(|id, root| wanted.get(id).is_some_and(|r| r == root));
        for (id, root) in wanted {
            if self.running.contains_key(&id) || self.failed.contains_key(&id) {
                continue;
            }
            match FolderWatcher::start(&root) {
                Ok(watcher) => {
                    tracing::info!(volume = id.0, root = %root.display(), "watching folders");
                    let stop = Arc::new(AtomicBool::new(false));
                    let ignore = relative_components(&state.data_dir, &root);
                    let (s, flag) = (Arc::clone(state), Arc::clone(&stop));
                    let spawned = std::thread::Builder::new()
                        .name(format!("prism-watch-{}", id.0))
                        .spawn(move || watch_loop(&s, id, &watcher, &flag, ignore.as_deref()));
                    if spawned.is_ok() {
                        self.running.insert(id, Running { root, stop });
                    }
                }
                Err(e) => {
                    tracing::warn!(volume = id.0, error = %e, "cannot watch the volume");
                    self.failed.insert(id, root);
                }
            }
        }
    }
}

/// Components of `path` below `root` (stored names), if it lies there: Prism's own data
/// folder, whose changes must not trigger refreshes (they would feed themselves).
fn relative_components(path: &Path, root: &Path) -> Option<Vec<String>> {
    let path = path.to_string_lossy().replace('/', "\\");
    let root = root.to_string_lossy().replace('/', "\\");
    let root = root.trim_end_matches('\\');
    let rest = path.get(root.len()..)?;
    if !names::eq_ignore_case(path.get(..root.len())?, root) || !rest.starts_with('\\') {
        return None;
    }
    Some(
        rest.split('\\')
            .filter(|c| !c.is_empty())
            .map(|c| names::encode(std::ffi::OsStr::new(c)))
            .collect(),
    )
}

fn under(path: &[String], prefix: &[String]) -> bool {
    path.len() >= prefix.len()
        && path
            .iter()
            .zip(prefix)
            .all(|(a, b)| names::eq_ignore_case(a, b))
}

fn watch_loop(
    state: &AppState,
    volume: VolumeId,
    watcher: &FolderWatcher,
    stop: &AtomicBool,
    ignore: Option<&[String]>,
) {
    let mut pending: Vec<Vec<String>> = Vec::new();
    let mut rescan = false;
    while !stop.load(Ordering::Relaxed) {
        if let Some(batch) = watcher.next_batch(Duration::from_secs(1), Duration::from_millis(500))
        {
            rescan |= batch.rescan;
            pending.extend(
                batch
                    .paths
                    .into_iter()
                    .filter(|p| ignore.is_none_or(|i| !under(p, i))),
            );
            if pending.len() > MAX_PENDING {
                pending.clear();
                rescan = true;
            }
        }
        if (rescan || !pending.is_empty()) && state.engine.active_for(volume).is_empty() {
            match state.submit_refresh(volume, std::mem::take(&mut pending), rescan) {
                Ok(_) => rescan = false,
                Err(e) => {
                    tracing::debug!(volume = volume.0, code = %e.code, "refresh not queued");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn info(kind: VolumeKind, fs: Option<&str>) -> VolumeInfo {
        VolumeInfo {
            kind,
            fs_type: fs.map(str::to_owned),
            fs_serial: None,
            volume_guid: None,
            unc_path: None,
            os_label: None,
            capacity: None,
            free_space: None,
            bus: None,
            last_mount: None,
        }
    }

    #[test]
    fn which_volumes_are_watched() {
        let exfat = info(VolumeKind::Removable, Some("exFAT"));
        let ntfs = info(VolumeKind::Fixed, Some("NTFS"));
        let share = info(VolumeKind::Network, None);
        assert!(needs_watcher(&exfat, VolumeStatus::Indexed));
        assert!(needs_watcher(&exfat, VolumeStatus::Indexing));
        assert!(needs_watcher(&share, VolumeStatus::ReindexNeeded));
        assert!(
            !needs_watcher(&ntfs, VolumeStatus::Indexed),
            "journal instead"
        );
        assert!(!needs_watcher(&exfat, VolumeStatus::New));
        assert!(!needs_watcher(&share, VolumeStatus::Ignored));
    }

    #[test]
    fn own_data_folder_is_ignored() {
        let root = Path::new(r"E:\");
        let data = Path::new(r"e:\Users\Me\PrismDev\app");
        let ignore = relative_components(data, root).unwrap();
        assert_eq!(ignore, ["Users", "Me", "PrismDev", "app"]);
        let path = |s: &str| s.split('\\').map(str::to_owned).collect::<Vec<_>>();
        assert!(under(
            &path(r"users\me\prismdev\APP\catalog.db-wal"),
            &ignore
        ));
        assert!(!under(&path(r"Users\Me\Documents\a.txt"), &ignore));
        assert_eq!(relative_components(data, Path::new(r"D:\")), None);
        assert_eq!(
            relative_components(Path::new(r"E:\Usersx"), Path::new(r"E:\Users")),
            None
        );
    }
}
