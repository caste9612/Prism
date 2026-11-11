//! Diagnostics in the app (F20): the log folder, and a diagnostic package the user sees before
//! exporting it (the logs contain file paths, F20-AC2).

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::SystemTime;

use prism_core::diagnostics::{self, ZipEntry};
use prism_core::store;
use serde::Serialize;
use ts_rs::TS;

use crate::dto::ApiError;
use crate::search_views::disk_name;
use crate::state::AppState;

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagnosticFileView {
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
}

/// What the package will contain, shown before exporting it.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagnosticsView {
    pub files: Vec<DiagnosticFileView>,
    /// The text of `system.txt`.
    pub system: String,
    /// The folder the package is saved to.
    pub destination: String,
}

impl AppState {
    /// The folder of the log files (F20-AC1).
    pub fn logs_dir(&self) -> PathBuf {
        crate::logs_dir()
    }

    /// Where the package goes: the Downloads folder in release builds; the development data
    /// folder in debug builds, which write only inside their sandbox (CLAUDE.md).
    fn diagnostics_destination(&self) -> PathBuf {
        if self.dev {
            return self.data_dir.join("diagnostics");
        }
        std::env::var_os("USERPROFILE")
            .map(|p| PathBuf::from(p).join("Downloads"))
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| self.data_dir.clone())
    }

    /// The system report of the package: versions, rights, catalog counters, settings, disks.
    fn system_report(&self) -> Result<String, ApiError> {
        let (volumes, db_size) = self
            .store
            .read(|c| Ok((store::volumes(c)?, store::database_size(c)?)))?;
        let mut counts = Vec::with_capacity(volumes.len());
        for v in &volumes {
            counts.push(self.store.read(|c| store::entry_count(c, v.id))?);
        }
        let entries: u64 = counts.iter().sum();
        let settings = self.settings();
        let online = self.online();
        let mut r = String::new();
        let _ = writeln!(
            r,
            "Prism {} ({})",
            env!("CARGO_PKG_VERSION"),
            if self.dev {
                "development build"
            } else {
                "release"
            }
        );
        let _ = writeln!(r, "{}", prism_win::system::windows_version());
        let _ = writeln!(
            r,
            "Administrator: {}",
            if self.elevated { "yes" } else { "no" }
        );
        let _ = writeln!(r, "Data folder: {}", self.data_dir.display());
        let _ = writeln!(
            r,
            "Catalog: {} disks, {entries} entries, {} MB",
            volumes.len(),
            db_size / 1_000_000
        );
        let _ = writeln!(
            r,
            "Settings: language {}, theme {}, start with Windows {}, search shortcut {}, content analysis {}{}",
            settings.language,
            settings.theme,
            settings.autostart,
            settings.search_shortcut,
            if settings.hashing_paused {
                "paused"
            } else {
                "on"
            },
            if settings.pause_hashing_on_battery {
                ", paused on battery"
            } else {
                ""
            }
        );
        // Entries and analysis progress of every disk, and the disk of every job: a package
        // showed 185,000 entries fewer and a day-long analysis without saying where (REVIEW
        // L29, L30).
        let _ = writeln!(r, "Disks:");
        for (v, count) in volumes.iter().zip(&counts) {
            let _ = write!(
                r,
                "  #{} {} - {} {} {} GB, {}, {}, {count} entries",
                v.id.0,
                disk_name(v),
                v.info.kind.as_str(),
                v.info.fs_type.as_deref().unwrap_or("?"),
                v.info.capacity.unwrap_or(0) / 1_000_000_000,
                v.status.as_str(),
                if online.contains_key(&v.id) {
                    "connected"
                } else {
                    "not connected"
                }
            );
            if let Some(s) = self.hashing.progress(v.id) {
                let _ = write!(
                    r,
                    ", analysed {}/{} files, {}/{} images, {}/{} videos",
                    s.hashed, s.total, s.printed, s.images, s.videos_printed, s.videos
                );
            }
            let _ = writeln!(r);
        }
        drop(online);
        let _ = writeln!(r, "Jobs:");
        let now = prism_core::model::unix_millis_now();
        for j in self.engine.jobs().iter().filter(|j| !j.state.is_finished()) {
            let disk = j.volume.map_or_else(String::new, |v| format!(" #{}", v.0));
            let _ = write!(
                r,
                "  {}{disk} {} ({}) {}/{}",
                j.kind,
                j.state.as_str(),
                j.phase,
                j.done,
                j.total
            );
            if let Some(started) = j.started_at {
                let _ = write!(r, ", for {} min", (now - started).max(0) / 60_000);
            }
            let _ = writeln!(r);
        }
        Ok(r)
    }

    /// What the package will contain (F20-AC2).
    pub fn diagnostics_preview(&self) -> Result<DiagnosticsView, ApiError> {
        let system = self.system_report()?;
        let mut files = vec![DiagnosticFileView {
            name: "system.txt".to_owned(),
            size: system.len() as u64,
        }];
        files.extend(
            diagnostics::log_files(&self.logs_dir())
                .into_iter()
                .map(|(p, size)| DiagnosticFileView {
                    name: format!(
                        "logs/{}",
                        p.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    size,
                }),
        );
        Ok(DiagnosticsView {
            files,
            system,
            destination: self
                .diagnostics_destination()
                .to_string_lossy()
                .into_owned(),
        })
    }

    /// Writes the package (never over an existing file) and returns its path.
    pub fn export_diagnostics(&self) -> Result<PathBuf, ApiError> {
        let mut entries = vec![ZipEntry {
            name: "system.txt".to_owned(),
            data: self.system_report()?.into_bytes(),
        }];
        for (path, _) in diagnostics::log_files(&self.logs_dir()) {
            // The log of today is being written: what is there now is enough.
            if let Ok(data) = std::fs::read(&path) {
                entries.push(ZipEntry {
                    name: format!(
                        "logs/{}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    data,
                });
            }
        }
        let zip = diagnostics::zip_stored(&entries, SystemTime::now());
        let dir = self.diagnostics_destination();
        std::fs::create_dir_all(&dir).map_err(|e| ApiError::new("export_failed", e))?;
        let stamp = diagnostics::file_stamp(SystemTime::now());
        let mut path = dir.join(format!("prism-diagnostics-{stamp}.zip"));
        let mut n = 2;
        let mut file = loop {
            match std::fs::File::options()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(f) => break f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 100 => {
                    path = dir.join(format!("prism-diagnostics-{stamp} ({n}).zip"));
                    n += 1;
                }
                Err(e) => return Err(ApiError::new("export_failed", e)),
            }
        };
        std::io::Write::write_all(&mut file, &zip)
            .map_err(|e| ApiError::new("export_failed", e))?;
        tracing::info!(path = %path.display(), bytes = zip.len(), "diagnostic package exported");
        Ok(path)
    }
}
