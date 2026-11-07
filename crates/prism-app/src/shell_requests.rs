//! Requests of the Explorer menu (F18): `prism-shell.exe` sends them on the named pipe; the
//! window comes forward and the interface opens the space of the folder or its duplicates.

use std::path::Path;
use std::sync::Arc;

use prism_core::names;
use prism_core::store;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use ts_rs::TS;

use crate::backup_state::nested;
use crate::dto::ApiError;
use crate::state::{AppState, access_path};

/// A request of the menu, as the interface receives it.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShellRequest {
    /// `analyze` or `duplicates`.
    pub action: String,
    pub path: String,
}

/// Where a path is in the catalog: the deepest catalog folder that contains it.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LocatedView {
    #[ts(type = "number")]
    pub volume_id: i64,
    #[ts(type = "number")]
    pub folder_id: i64,
    /// Path of that folder relative to the volume root (`\` separators, empty for the root).
    pub folder: String,
}

/// Parses `action\tpath`.
pub fn parse(request: &str) -> Option<ShellRequest> {
    let (action, path) = request.split_once('\t')?;
    (matches!(action, "analyze" | "duplicates") && !path.trim().is_empty()).then(|| ShellRequest {
        action: action.to_owned(),
        path: path.trim().to_owned(),
    })
}

impl AppState {
    /// The connected volume and the catalog folder of a path (F18-AC1).
    pub fn locate_path(&self, path: &str) -> Result<LocatedView, ApiError> {
        let target = Path::new(path.trim());
        let (volume, root) = self
            .online()
            .iter()
            .filter_map(|(id, d)| access_path(d).map(|root| (*id, root)))
            .filter(|(_, root)| nested(target, root))
            .max_by_key(|(_, root)| root.as_os_str().len())
            .ok_or_else(|| ApiError::new("not_in_catalog", path))?;
        // `nested` compared whole components without case: what follows the mount's
        // components is the path inside the volume.
        let parts: Vec<String> = target
            .components()
            .skip(root.components().count())
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let (folder, rel) = self.store.read(|c| {
            let mut current = store::root_entry(c, volume)?
                .ok_or_else(|| store::StoreError::NotFound(path.to_owned()))?;
            for part in &parts {
                let stored = names::encode(std::ffi::OsStr::new(part));
                match store::child_by_name(c, current.id, &stored)? {
                    Some(e) if e.is_dir => current = e,
                    _ => break,
                }
            }
            Ok((current.id, store::relative_path(c, current.id)?))
        })?;
        Ok(LocatedView {
            volume_id: volume.0,
            folder_id: folder.0,
            folder: rel
                .split('\\')
                .filter(|p| !p.is_empty())
                .map(|p| names::decode(p).to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\\"),
        })
    }
}

/// Serves the pipe of the menu for as long as Prism runs.
pub fn spawn(app: &AppHandle, state: &Arc<AppState>) {
    let app = app.clone();
    let name = prism_win::pipe::name(state.dev);
    let _ = std::thread::Builder::new()
        .name("prism-shell-pipe".into())
        .spawn(move || {
            let result = prism_win::pipe::serve(&name, |request| match parse(request) {
                Some(r) => {
                    tracing::info!(action = %r.action, path = %r.path, "Explorer menu request");
                    crate::tray::show_main(&app);
                    let _ = app.emit("shell-request", r);
                    "ok".to_owned()
                }
                None => "error".to_owned(),
            });
            if let Err(e) = result {
                tracing::warn!(error = %e, "Explorer menu pipe not available");
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_of_the_menu() {
        let r = parse("analyze\tD:\\Foto").unwrap();
        assert_eq!(
            (r.action.as_str(), r.path.as_str()),
            ("analyze", "D:\\Foto")
        );
        assert!(parse("duplicates\tC:\\").is_some());
        assert!(parse("delete\tC:\\").is_none());
        assert!(parse("analyze").is_none());
        assert!(parse("analyze\t ").is_none());
    }
}
