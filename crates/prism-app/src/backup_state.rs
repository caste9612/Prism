//! Backups in the app (F13–F15): checks and comparisons run on the catalog and their results
//! stay in memory (the last few) to be browsed one folder at a time; the copy of the missing
//! files is a job that writes through the development sandbox guard (CLAUDE.md).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use prism_core::backup::copy::{self, Conflict, CopyItem, CopyOutcome, CopyReport, CopyRequest};
use prism_core::backup::{self, BackupError, CompareReport, MatchMode, Side, VerifyReport};
use prism_core::cancel::CancelToken;
use prism_core::jobs::{JobId, JobSpec, Priority};
use prism_core::model::{VolumeId, unix_millis_now};
use prism_core::names;
use prism_core::store::{self, ActionRecord};

use crate::backup_views::{
    self, CompareSummaryView, CopyPreviewView, CopyResultView, ReportFileView, ReportFilterInput,
    ReportFolderView, SideInput, VerifySummaryView,
};
use crate::dto::ApiError;
use crate::state::{AppState, access_path, job_disks};

/// Results kept in memory.
const KEEP_REPORTS: usize = 4;

pub enum Report {
    Verify { source: Side, report: VerifyReport },
    Compare { report: CompareReport },
}

/// The last id given and the reports kept, oldest first.
type Reports = (u64, VecDeque<(u64, Arc<Report>)>);

#[derive(Default)]
pub struct BackupState {
    reports: Mutex<Reports>,
    copies: Mutex<HashMap<u64, CopyReport>>,
}

impl BackupState {
    fn reports(&self) -> MutexGuard<'_, Reports> {
        self.reports.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn copies(&self) -> MutexGuard<'_, HashMap<u64, CopyReport>> {
        self.copies.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn keep(&self, report: Report) -> u64 {
        let mut r = self.reports();
        r.0 += 1;
        let id = r.0;
        r.1.push_back((id, Arc::new(report)));
        while r.1.len() > KEEP_REPORTS {
            r.1.pop_front();
        }
        id
    }

    fn get(&self, id: u64) -> Result<Arc<Report>, ApiError> {
        self.reports()
            .1
            .iter()
            .find(|(k, _)| *k == id)
            .map(|(_, r)| Arc::clone(r))
            .ok_or_else(|| ApiError::new("report_expired", id))
    }
}

fn backup_error(e: BackupError) -> ApiError {
    match e {
        BackupError::Overlap => ApiError::new("backup_overlap", e),
        BackupError::Cancelled => ApiError::new("cancelled", e),
        BackupError::Store(s) => s.into(),
    }
}

/// The file system path of a catalog folder through the current mount of its volume.
fn folder_path(mount: &Path, rel: &str) -> PathBuf {
    copy::to_path(mount, rel)
}

/// What a copy needs, checked (F15-AC1).
struct CopyPlan {
    source_volume: VolumeId,
    source_root: PathBuf,
    dest_volume: VolumeId,
    dest_root: PathBuf,
    dest_components: Vec<String>,
    items: Vec<CopyItem>,
    disks: Vec<String>,
}

impl AppState {
    /// A folder of the catalog from the interface.
    fn side(&self, input: &SideInput) -> Result<Side, ApiError> {
        let volume = VolumeId(input.volume_id);
        let rel = input.components().join("\\");
        let entry = self
            .store
            .read(|c| store::resolve_relative_path(c, volume, &rel))?
            .filter(|e| e.is_dir)
            .ok_or_else(|| ApiError::new("folder_not_found", input.folder.clone()))?;
        Ok(Side {
            volume,
            root: entry.id,
        })
    }

    /// Which files of `source` have no copy in `destinations` (F13), on the catalog.
    pub fn backup_verify(
        &self,
        source: &SideInput,
        destinations: &[SideInput],
    ) -> Result<VerifySummaryView, ApiError> {
        if destinations.is_empty() {
            return Err(ApiError::new("no_destination", ""));
        }
        let src = self.side(source)?;
        let dests = destinations
            .iter()
            .map(|d| self.side(d))
            .collect::<Result<Vec<_>, _>>()?;
        let report = self
            .store
            .read(|c| Ok(backup::verify(c, &src, &dests, &CancelToken::new())))?
            .map_err(backup_error)?;
        let summary = VerifySummaryView {
            report_id: 0,
            total: report.total.into(),
            by_content: report.by_content.into(),
            by_metadata: report.by_metadata.into(),
            missing: report.missing_tally().into(),
        };
        let id = self.backup.keep(Report::Verify {
            source: src,
            report,
        });
        tracing::info!(
            report = id,
            files = summary.total.files,
            missing = summary.missing.files,
            "backup check finished"
        );
        Ok(VerifySummaryView {
            report_id: id,
            ..summary
        })
    }

    /// Two folders side by side (F14), on the catalog.
    pub fn backup_compare(
        &self,
        a: &SideInput,
        b: &SideInput,
        by_content: bool,
    ) -> Result<CompareSummaryView, ApiError> {
        let (sa, sb) = (self.side(a)?, self.side(b)?);
        let mode = if by_content {
            MatchMode::Content
        } else {
            MatchMode::Path
        };
        let report = self
            .store
            .read(|c| Ok(backup::compare(c, &sa, &sb, mode, &CancelToken::new())))?
            .map_err(backup_error)?;
        let tallies = backup::Status::ALL
            .into_iter()
            .map(|s| report.tally(s).into())
            .collect();
        let id = self.backup.keep(Report::Compare { report });
        Ok(CompareSummaryView {
            report_id: id,
            tallies,
        })
    }

    /// One level of the result tree of a check or a comparison (F13-AC4, F14-AC3).
    pub fn backup_folder(
        &self,
        report_id: u64,
        folder: &str,
        filter: &ReportFilterInput,
    ) -> Result<ReportFolderView, ApiError> {
        let report = self.backup.get(report_id)?;
        let text = filter
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(names::fold);
        let keep = |rel: &str, size: u64| {
            size >= filter.min_size
                && text
                    .as_deref()
                    .is_none_or(|t| names::fold(&backup_views::display(rel)).contains(t))
        };
        let folder = folder.trim_matches('\\');
        Ok(match &*report {
            Report::Verify { report, .. } => {
                let shown: Vec<usize> = (0..report.missing.len())
                    .filter(|&i| keep(&report.missing[i].rel, report.missing[i].size))
                    .collect();
                let view = backup::folder_view(
                    shown.iter().map(|&i| {
                        let f = &report.missing[i];
                        (f.rel.as_str(), f.size, 0)
                    }),
                    folder,
                    1,
                );
                backup_views::folder_view(view, |pos| {
                    let f = &report.missing[shown[pos]];
                    ReportFileView {
                        name: backup_views::display(f.rel.rsplit('\\').next().unwrap_or("")),
                        rel: f.rel.clone(),
                        size: f.size,
                        status: "missing".to_owned(),
                        other: None,
                        evidence: None,
                    }
                })
            }
            Report::Compare { report } => {
                let statuses: Vec<backup::Status> = filter
                    .statuses
                    .iter()
                    .filter_map(|s| backup_views::status_of(s))
                    .collect();
                let shown: Vec<usize> = (0..report.items.len())
                    .filter(|&i| {
                        let it = &report.items[i];
                        (statuses.is_empty() || statuses.contains(&it.status))
                            && keep(&it.rel, it.size)
                    })
                    .collect();
                let view = backup::folder_view(
                    shown.iter().map(|&i| {
                        let it = &report.items[i];
                        (it.rel.as_str(), it.size, it.status as usize)
                    }),
                    folder,
                    backup::Status::ALL.len(),
                );
                backup_views::folder_view(view, |pos| {
                    let it = &report.items[shown[pos]];
                    ReportFileView {
                        name: backup_views::display(it.rel.rsplit('\\').next().unwrap_or("")),
                        rel: it.rel.clone(),
                        size: it.size,
                        status: it.status.as_str().to_owned(),
                        other: it.other.as_deref().map(backup_views::display),
                        evidence: it.evidence.map(|e| {
                            match e {
                                backup::Evidence::Content => "content",
                                backup::Evidence::Metadata => "metadata",
                            }
                            .to_owned()
                        }),
                    }
                })
            }
        })
    }

    /// The copy of the missing files of a check into `dest` (F15-AC1/AC3).
    fn copy_plan(&self, report_id: u64, dest: &SideInput) -> Result<CopyPlan, ApiError> {
        let report = self.backup.get(report_id)?;
        let Report::Verify { source, report } = &*report else {
            return Err(ApiError::new("not_a_check", report_id));
        };
        let source_detected = self.detected(source.volume)?;
        let source_mount = access_path(&source_detected)
            .ok_or_else(|| ApiError::new("volume_offline", format!("volume {}", source.volume)))?;
        let source_rel = self.store.read(|c| store::relative_path(c, source.root))?;
        let source_root = folder_path(&source_mount, &source_rel);
        let dest_volume = VolumeId(dest.volume_id);
        let dest_detected = self.detected(dest_volume)?;
        let dest_mount = access_path(&dest_detected)
            .ok_or_else(|| ApiError::new("volume_offline", format!("volume {dest_volume}")))?;
        let dest_components = dest.components();
        let mut dest_root = dest_mount;
        for c in &dest_components {
            dest_root.push(c);
        }
        // Copying a folder into itself would never end and is not a backup.
        let lower = |p: &Path| p.to_string_lossy().to_lowercase();
        if lower(&dest_root).starts_with(&lower(&source_root))
            || lower(&source_root).starts_with(&lower(&dest_root))
        {
            return Err(ApiError::new("copy_overlap", dest_root.display()));
        }
        // Development builds write only inside their sandbox (CLAUDE.md).
        if let Some(guard) = self.deletion_guard()? {
            guard
                .check(&dest_root)
                .map_err(|e| ApiError::new("outside_sandbox", e))?;
        }
        let mut disks = job_disks(&source_detected);
        for d in job_disks(&dest_detected) {
            if !disks.contains(&d) {
                disks.push(d);
            }
        }
        Ok(CopyPlan {
            source_volume: source.volume,
            source_root,
            dest_volume,
            dest_root,
            dest_components,
            items: report
                .missing
                .iter()
                .map(|f| CopyItem {
                    rel: f.rel.clone(),
                    size: f.size,
                })
                .collect(),
            disks,
        })
    }

    /// Files, size and free space before copying (F15-AC2).
    pub fn copy_preview(
        &self,
        report_id: u64,
        dest: &SideInput,
    ) -> Result<CopyPreviewView, ApiError> {
        let plan = self.copy_plan(report_id, dest)?;
        let bytes: u64 = plan.items.iter().map(|i| i.size).sum();
        let mount = self
            .detected(plan.dest_volume)
            .ok()
            .and_then(|d| access_path(&d))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let free_space = prism_win::volumes::disk_space(&mount).map(|(_, free)| free);
        Ok(CopyPreviewView {
            files: plan.items.len() as u64,
            bytes,
            free_space,
            enough_space: free_space.is_none_or(|f| f >= bytes),
            dest_path: plan.dest_root.to_string_lossy().into_owned(),
        })
    }

    /// Starts the copy job (F15); its outcome is read with [`AppState::copy_result`].
    pub fn copy_start(
        self: &Arc<Self>,
        report_id: u64,
        dest: &SideInput,
        rename_on_conflict: bool,
    ) -> Result<JobId, ApiError> {
        let plan = self.copy_plan(report_id, dest)?;
        let guard = self.deletion_guard()?;
        let s = Arc::clone(self);
        Ok(self.engine.submit(JobSpec {
            kind: "copy".to_owned(),
            volume: Some(plan.dest_volume),
            priority: Priority::Indexing,
            disks: plan.disks.clone(),
            run: Box::new(move |ctx| {
                let report = copy::copy_files(
                    &CopyRequest {
                        source_root: &plan.source_root,
                        dest_root: &plan.dest_root,
                        items: &plan.items,
                        conflict: if rename_on_conflict {
                            Conflict::Rename
                        } else {
                            Conflict::Skip
                        },
                    },
                    guard.as_ref(),
                    &ctx.cancel,
                    &ctx.progress,
                );
                log_copies(&s, &plan, &report);
                tracing::info!(
                    from = plan.source_volume.0,
                    to = plan.dest_volume.0,
                    copied = report.count("copied") + report.count("renamed"),
                    failed = report.count("failed"),
                    interrupted = report.interrupted,
                    "copy finished"
                );
                let interrupted = report.interrupted;
                s.backup.copies().insert(ctx.id.0, report);
                // The new files enter the catalog of the destination.
                let rescan = plan.dest_components.is_empty();
                let _ = s.submit_refresh(plan.dest_volume, vec![plan.dest_components], rescan);
                if interrupted {
                    Err("cancelled".to_owned())
                } else {
                    Ok(())
                }
            }),
        }))
    }

    /// The outcome of a copy job, once finished.
    pub fn copy_result(&self, job: u64) -> Option<CopyResultView> {
        self.backup.copies().get(&job).map(Into::into)
    }
}

/// The copies in the actions log, like the deletions.
fn log_copies(state: &AppState, plan: &CopyPlan, report: &CopyReport) {
    let at = unix_millis_now();
    let records: Vec<ActionRecord> = report
        .results
        .iter()
        .filter_map(|r| {
            let (outcome, detail, path) = match &r.outcome {
                CopyOutcome::Copied => ("done", None, copy::to_path(&plan.dest_root, &r.rel)),
                CopyOutcome::Renamed(p) => ("done", Some("renamed".to_owned()), p.clone()),
                CopyOutcome::Failed(e) => (
                    "failed",
                    Some(e.clone()),
                    copy::to_path(&plan.dest_root, &r.rel),
                ),
                _ => return None,
            };
            Some(ActionRecord {
                id: 0,
                at,
                action: "copy".to_owned(),
                volume: Some(plan.dest_volume),
                path: path.to_string_lossy().into_owned(),
                size: r.size,
                outcome: outcome.to_owned(),
                detail,
            })
        })
        .collect();
    if !records.is_empty() {
        let _ = state.store.write(move |w| store::log_actions(w, &records));
    }
}
