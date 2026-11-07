//! Copy of the files missing from a backup (F15): the folder structure is recreated under a
//! destination folder (F15-AC3), nothing that exists is ever overwritten (F15-AC4), every file
//! is read back and compared with what was read from the source before it gets its final name
//! (F15-AC5).
//!
//! Each file is written to `<name>.prism-part` and renamed only after the check, so an
//! interruption never leaves a partial file under a real name. Copying again resumes: a file
//! already at its destination with the same content counts as done, a leftover `.prism-part`
//! (Prism's own) is replaced. Every write goes through the [`SandboxGuard`] when one is given
//! (development builds and tests, CLAUDE.md).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cancel::CancelToken;
use crate::hashing::full_hash;
use crate::jobs::JobProgress;
use crate::names;
use crate::sandbox::SandboxGuard;
use crate::store::Digest;

const BUFFER: usize = 1024 * 1024;
/// Suffix of a file being copied.
pub const PART_SUFFIX: &str = ".prism-part";

/// What to do when the destination already has a different file with the same name (F15-AC4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    Skip,
    /// Copy as `name (2).ext`, `name (3).ext`...
    Rename,
}

/// A file to copy, by its path relative to the source folder (stored names, `\`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyItem {
    pub rel: String,
    pub size: u64,
}

pub struct CopyRequest<'a> {
    pub source_root: &'a Path,
    pub dest_root: &'a Path,
    pub items: &'a [CopyItem],
    pub conflict: Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyOutcome {
    Copied,
    /// Copied under another name because of a conflict.
    Renamed(PathBuf),
    /// Already at the destination with the same content (a resumed copy).
    AlreadyThere,
    /// A different file has the name and the user chose to skip.
    SkippedConflict,
    Failed(String),
}

impl CopyOutcome {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Copied => "copied",
            Self::Renamed(_) => "renamed",
            Self::AlreadyThere => "already_there",
            Self::SkippedConflict => "skipped_conflict",
            Self::Failed(_) => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyResult {
    pub rel: String,
    pub size: u64,
    pub outcome: CopyOutcome,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyReport {
    pub results: Vec<CopyResult>,
    /// Stopped before the end (cancelled); copying again resumes.
    pub interrupted: bool,
}

impl CopyReport {
    #[must_use]
    pub fn count(&self, outcome: &str) -> usize {
        self.results
            .iter()
            .filter(|r| r.outcome.as_str() == outcome)
            .count()
    }

    /// Bytes written by this run.
    #[must_use]
    pub fn copied_bytes(&self) -> u64 {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, CopyOutcome::Copied | CopyOutcome::Renamed(_)))
            .map(|r| r.size)
            .sum()
    }
}

/// Every change to the destination, checked by the guard when there is one.
struct Writer<'a> {
    guard: Option<&'a SandboxGuard>,
}

impl Writer<'_> {
    fn checked(&self, path: &Path, follow: bool) -> std::io::Result<PathBuf> {
        match self.guard {
            None => Ok(path.to_path_buf()),
            Some(g) => if follow {
                g.check(path)
            } else {
                g.check_no_follow(path)
            }
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.to_string())),
        }
    }

    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir_all(self.checked(path, true)?)
    }

    fn create_new(&self, path: &Path) -> std::io::Result<File> {
        File::options()
            .write(true)
            .create_new(true)
            .open(self.checked(path, true)?)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(self.checked(path, false)?)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(self.checked(from, false)?, self.checked(to, false)?)
    }
}

/// The file system path of a relative path in stored names.
#[must_use]
pub fn to_path(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('\\').filter(|s| !s.is_empty()) {
        p.push(names::decode(part));
    }
    p
}

fn part_path(dst: &Path) -> PathBuf {
    let mut name = dst.file_name().unwrap_or_default().to_os_string();
    name.push(PART_SUFFIX);
    dst.with_file_name(name)
}

/// `name (2).ext`, `name (3).ext`... the first that does not exist.
fn free_name(dst: &Path) -> PathBuf {
    let stem = dst
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let ext = dst
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| dst.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists() && !part_path(p).exists())
        .unwrap_or_else(|| dst.to_path_buf())
}

/// Copies `src` to `part`, hashing what it reads; `None` if cancelled.
fn copy_hashed(
    src: &Path,
    out: &mut File,
    cancel: &CancelToken,
    done: &AtomicU64,
) -> std::io::Result<Option<Digest>> {
    let mut input = File::open(src)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; BUFFER];
    loop {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        done.fetch_add(n as u64, Ordering::Relaxed);
    }
    out.sync_all()?;
    Ok(Some(*h.finalize().as_bytes()))
}

enum Step {
    Done(CopyOutcome),
    Cancelled,
}

fn copy_one(
    w: &Writer<'_>,
    req: &CopyRequest<'_>,
    item: &CopyItem,
    cancel: &CancelToken,
    progress: &JobProgress,
) -> std::io::Result<Step> {
    let src = to_path(req.source_root, &item.rel);
    let mut dst = to_path(req.dest_root, &item.rel);
    let source_meta = fs::metadata(&src)?;
    let mut renamed = false;
    if dst.exists() {
        let ignore = AtomicU64::new(0);
        let same = dst.is_file()
            && fs::metadata(&dst)?.len() == source_meta.len()
            && full_hash(&dst, cancel, &ignore)? == full_hash(&src, cancel, &ignore)?;
        if same {
            progress
                .done
                .fetch_add(source_meta.len(), Ordering::Relaxed);
            return Ok(Step::Done(CopyOutcome::AlreadyThere));
        }
        match req.conflict {
            Conflict::Skip => {
                progress
                    .done
                    .fetch_add(source_meta.len(), Ordering::Relaxed);
                return Ok(Step::Done(CopyOutcome::SkippedConflict));
            }
            Conflict::Rename => {
                dst = free_name(&dst);
                renamed = true;
            }
        }
    }
    if let Some(parent) = dst.parent().filter(|p| !p.is_dir()) {
        w.create_dir_all(parent)?;
    }
    let part = part_path(&dst);
    if part.exists() {
        // Left by an interrupted copy: Prism's own file.
        w.remove_file(&part)?;
    }
    let mut out = w.create_new(&part)?;
    let before = progress.done.load(Ordering::Relaxed);
    let copied = copy_hashed(&src, &mut out, cancel, &progress.done);
    let digest = match copied {
        Ok(Some(d)) => d,
        Ok(None) => {
            drop(out);
            w.remove_file(&part)?;
            return Ok(Step::Cancelled);
        }
        Err(e) => {
            drop(out);
            let _ = w.remove_file(&part);
            return Err(e);
        }
    };
    if let Ok(t) = source_meta.modified() {
        let _ = out.set_modified(t);
    }
    drop(out);
    // Read back what was written (F15-AC5).
    let ignore = AtomicU64::new(0);
    let written = full_hash(&part, &CancelToken::new(), &ignore);
    if written.as_ref().ok() != Some(&digest) {
        let _ = w.remove_file(&part);
        progress.done.store(before + item.size, Ordering::Relaxed);
        return Ok(Step::Done(CopyOutcome::Failed(
            "the copy does not match the source".to_owned(),
        )));
    }
    // Never replace a file that appeared meanwhile (F15-AC4).
    if dst.exists() {
        w.remove_file(&part)?;
        return Ok(Step::Done(CopyOutcome::SkippedConflict));
    }
    w.rename(&part, &dst)?;
    Ok(Step::Done(if renamed {
        CopyOutcome::Renamed(dst)
    } else {
        CopyOutcome::Copied
    }))
}

/// Copies the items (F15). Errors of single files are reported in the results and do not stop
/// the others; cancelling stops between two blocks and removes the partial file.
pub fn copy_files(
    req: &CopyRequest<'_>,
    guard: Option<&SandboxGuard>,
    cancel: &CancelToken,
    progress: &JobProgress,
) -> CopyReport {
    let w = Writer { guard };
    progress.set_phase("copying");
    progress
        .total
        .store(req.items.iter().map(|i| i.size).sum(), Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    let mut report = CopyReport::default();
    for item in req.items {
        if cancel.is_cancelled() {
            report.interrupted = true;
            break;
        }
        let outcome = match copy_one(&w, req, item, cancel, progress) {
            Ok(Step::Done(o)) => o,
            Ok(Step::Cancelled) => {
                report.interrupted = true;
                break;
            }
            Err(e) if cancel.is_cancelled() && e.kind() == std::io::ErrorKind::Interrupted => {
                report.interrupted = true;
                break;
            }
            Err(e) => CopyOutcome::Failed(e.to_string()),
        };
        report.results.push(CopyResult {
            rel: item.rel.clone(),
            size: item.size,
            outcome,
        });
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_names_and_parts() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("photo.jpg");
        std::fs::write(&dst, b"x").unwrap();
        assert_eq!(free_name(&dst), dir.path().join("photo (2).jpg"));
        std::fs::write(dir.path().join("photo (2).jpg"), b"x").unwrap();
        assert_eq!(free_name(&dst), dir.path().join("photo (3).jpg"));
        assert_eq!(part_path(&dst), dir.path().join("photo.jpg.prism-part"));
        assert_eq!(
            to_path(Path::new(r"C:\x"), r"a\b.txt"),
            Path::new(r"C:\x\a\b.txt")
        );
    }
}
