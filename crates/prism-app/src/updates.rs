//! Automatic updates (F22-AC3): checked at startup and every 24 hours, downloaded in the
//! background, installed only when the user confirms; the packages are signed and the
//! signature is verified by the Tauri updater before anything is installed.
//!
//! As for Orbit (ARCHITECTURE §8.1), the public key of the signatures and the address of the
//! update manifest (`latest.json` of the latest GitHub release) are in `plugins.updater` of
//! `tauri.conf.json`. Only release builds check: a development build never does.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use prism_core::model::unix_millis_now;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};
use ts_rs::TS;

use crate::dto::ApiError;

const FIRST_CHECK: Duration = Duration::from_secs(60);
const EVERY: Duration = Duration::from_secs(24 * 3600);

/// Public key of the update signatures and address of the update manifest, when this build
/// checks for updates.
static SOURCE: LazyLock<Option<Source>> = LazyLock::new(|| {
    if cfg!(debug_assertions) {
        return None;
    }
    from_config(include_str!("../tauri.conf.json"))
});

#[derive(Debug, PartialEq, Eq)]
struct Source {
    pubkey: String,
    endpoint: String,
}

/// Whether a build can check for updates: both a key and an address, the address over HTTPS.
#[must_use]
pub fn configured(pubkey: Option<&str>, endpoint: Option<&str>) -> bool {
    pubkey.is_some_and(|k| !k.trim().is_empty())
        && endpoint.is_some_and(|u| u.trim().starts_with("https://"))
}

/// Key and address from `plugins.updater` of the Tauri configuration, if both are there.
fn from_config(conf: &str) -> Option<Source> {
    let conf: serde_json::Value = serde_json::from_str(conf).ok()?;
    let updater = conf.get("plugins")?.get("updater")?;
    let pubkey = updater.get("pubkey")?.as_str()?.trim();
    let endpoint = updater.get("endpoints")?.get(0)?.as_str()?.trim();
    configured(Some(pubkey), Some(endpoint)).then(|| Source {
        pubkey: pubkey.to_owned(),
        endpoint: endpoint.to_owned(),
    })
}

/// What the interface shows about updates.
#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateView {
    /// This build checks for updates.
    pub enabled: bool,
    pub checking: bool,
    /// The newer version found, if any.
    pub available: Option<String>,
    pub notes: Option<String>,
    /// Downloaded and verified: it can be installed.
    pub ready: bool,
    pub error: Option<String>,
    #[ts(type = "number | null")]
    pub last_check: Option<i64>,
}

#[derive(Default)]
pub struct UpdateState {
    view: Mutex<UpdateView>,
    downloaded: Mutex<Option<(Update, Vec<u8>)>>,
}

impl UpdateState {
    fn view(&self) -> MutexGuard<'_, UpdateView> {
        self.view.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn snapshot(&self) -> UpdateView {
        let mut v = self.view().clone();
        v.enabled = SOURCE.is_some();
        v
    }
}

/// One check: finds a newer version and downloads it in the background.
async fn check_once(app: &AppHandle, state: &UpdateState) {
    let Some(source) = SOURCE.as_ref() else {
        return;
    };
    state.view().checking = true;
    let _ = app.emit("update-changed", ());
    let result: Result<Option<(Update, Vec<u8>)>, String> = async {
        let url = source.endpoint.parse().map_err(|e| format!("{e}"))?;
        let closing = app.clone();
        let updater = app
            .updater_builder()
            .pubkey(&source.pubkey)
            // On Windows the updater ends the process as soon as the installer starts.
            .on_before_exit(move || crate::close_catalog(&closing))
            .endpoints(vec![url])
            .map_err(|e| e.to_string())?
            .build()
            .map_err(|e| e.to_string())?;
        let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        {
            let mut v = state.view();
            v.available = Some(update.version.clone());
            v.notes = update.body.clone();
        }
        let _ = app.emit("update-changed", ());
        let bytes = update
            .download(|_, _| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some((update, bytes)))
    }
    .await;
    {
        let mut v = state.view();
        v.checking = false;
        v.last_check = Some(unix_millis_now());
        match result {
            Ok(Some(downloaded)) => {
                tracing::info!(version = %downloaded.0.version, "update downloaded");
                v.ready = true;
                v.error = None;
                *state
                    .downloaded
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(downloaded);
            }
            Ok(None) => v.error = None,
            Err(e) => {
                tracing::warn!(error = %e, "update check failed");
                v.error = Some(e);
            }
        }
    }
    let _ = app.emit("update-changed", ());
}

/// Checks soon after the start and then every 24 hours, when the build is configured.
pub fn spawn(app: &AppHandle, state: Arc<UpdateState>) {
    if SOURCE.is_none() {
        tracing::info!("automatic updates not configured in this build");
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio_sleep(FIRST_CHECK).await;
        loop {
            check_once(&app, &state).await;
            tokio_sleep(EVERY).await;
        }
    });
}

/// Sleeps without holding a thread of the async runtime.
async fn tokio_sleep(d: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}

/// Checks now, from the settings.
pub async fn check_now(app: &AppHandle, state: &UpdateState) -> Result<(), ApiError> {
    if SOURCE.is_none() {
        return Err(ApiError::new("updates_not_configured", ""));
    }
    check_once(app, state).await;
    Ok(())
}

/// Installs the downloaded update after the user confirmed it (F22-AC3). The installer of the
/// new version closes Prism and starts it again.
pub fn install(app: &AppHandle, state: &UpdateState) -> Result<(), ApiError> {
    let downloaded = state
        .downloaded
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .ok_or_else(|| ApiError::new("no_update", ""))?;
    let (update, bytes) = downloaded;
    tracing::info!(version = %update.version, "installing the update");
    update
        .install(bytes)
        .map_err(|e| ApiError::new("update_failed", e))?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_need_a_key_and_an_https_address() {
        assert!(configured(
            Some("RWQ..."),
            Some("https://example.org/latest.json")
        ));
        assert!(!configured(None, Some("https://example.org/latest.json")));
        assert!(!configured(
            Some("  "),
            Some("https://example.org/latest.json")
        ));
        assert!(!configured(
            Some("RWQ..."),
            Some("http://example.org/latest.json")
        ));
        assert!(!configured(Some("RWQ..."), None));
    }

    #[test]
    fn key_and_address_come_from_the_tauri_configuration() {
        let conf = r#"{"plugins": {"updater": {"pubkey": " RWQ... ",
            "endpoints": ["https://example.org/latest.json"]}}}"#;
        assert_eq!(
            from_config(conf),
            Some(Source {
                pubkey: "RWQ...".into(),
                endpoint: "https://example.org/latest.json".into()
            })
        );
        assert_eq!(
            from_config(r#"{"plugins": {"updater": {"pubkey": "", "endpoints": []}}}"#),
            None
        );
        assert_eq!(from_config("{}"), None);
    }

    #[test]
    fn release_builds_check_the_latest_github_release() {
        let source = from_config(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json has the public key and the address of the updates");
        assert_eq!(
            source.endpoint,
            "https://github.com/caste9612/Prism/releases/latest/download/latest.json"
        );
        // A development build never checks.
        assert_eq!(
            UpdateState::default().snapshot().enabled,
            !cfg!(debug_assertions)
        );
    }
}
