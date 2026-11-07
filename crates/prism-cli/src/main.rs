//! Prism development CLI. Not a public product: it exists to run scans,
//! benchmarks and diagnostics without the GUI (docs/ARCHITECTURE.md §2).
//!
//! Catalogs live in `%LOCALAPPDATA%\PrismDev\` (docs/TESTING.md, rule S4). Every command
//! only reads the scanned folders; the only files written are the development catalogs.

mod bench;
mod format;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use prism_core::cancel::CancelToken;
use prism_core::exclusion::{ExclusionPolicy, ExclusionRule};
use prism_core::indexer::{self, IndexProgress, IndexReport, IndexRequest};
use prism_core::model::{EntryRecord, Volume, VolumeId, VolumeInfo, VolumeKind, VolumeStatus};
use prism_core::names;
use prism_core::reconcile::ReconcileTarget;
use prism_core::scan::{Enumerator, WalkEnumerator};
use prism_core::store::{self, Store};
use prism_core::volumes::{self, DetectedVolume, VolumeMatch, VolumeSource, match_volumes};
use prism_win::mft::MftEnumerator;
use prism_win::volumes::WinVolumeSource;

use crate::format::{human_size, human_time};

#[derive(Parser)]
#[command(name = "prism-cli", version = prism_core::VERSION, about = "Prism development tool")]
struct Cli {
    /// Catalog file (default: %LOCALAPPDATA%\PrismDev\dev-catalog.db).
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    /// Log level for diagnostics (error, warn, info, debug, trace).
    #[arg(long, global = true, default_value = "warn")]
    log: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Index a folder or a drive (read-only on the disk); a second scan updates the catalog.
    Scan {
        /// Folder or drive root, e.g. C:\ or D:\Photos.
        path: PathBuf,
        #[command(flatten)]
        walk: WalkArgs,
    },
    /// List the volumes in the catalog.
    Volumes,
    /// Detect the connected volumes and match them with the catalog (ADR-0016).
    Detect {
        /// Record the result in the catalog (known volumes updated, new ones added).
        #[arg(long)]
        record: bool,
    },
    /// List a folder from the catalog (works offline).
    Ls { path: PathBuf },
    /// Show the largest items of a folder from the catalog.
    Du {
        path: PathBuf,
        /// Number of items to show.
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Catalog size and counters.
    Stats,
    /// Check the consistency of the catalog.
    Check,
    /// Measure a full indexing and an update into a fresh temporary catalog.
    Bench {
        path: PathBuf,
        #[command(flatten)]
        walk: WalkArgs,
        /// Append the result (Markdown) to this file.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Args, Clone)]
pub struct WalkArgs {
    /// Read the NTFS Master File Table instead of walking folders (administrator only; the
    /// path must be a volume root or a folder on a drive letter).
    #[arg(long)]
    pub mft: bool,
    /// Listing threads of the walker (default: number of CPUs, at most 8).
    #[arg(long)]
    threads: Option<usize>,
    /// Do not apply the default exclusions ($RECYCLE.BIN, System Volume Information).
    #[arg(long)]
    no_default_exclusions: bool,
    /// Exclude every folder with this exact name.
    #[arg(long = "exclude-dir", value_name = "NAME")]
    exclude_dirs: Vec<String>,
    /// Exclude this exact path, relative to the scanned root.
    #[arg(long = "exclude-path", value_name = "REL")]
    exclude_paths: Vec<String>,
}

impl WalkArgs {
    fn walker(&self) -> WalkEnumerator {
        self.threads
            .map_or_else(WalkEnumerator::default, WalkEnumerator::with_threads)
    }

    fn enumerator(&self) -> Box<dyn Enumerator> {
        if self.mft {
            Box::new(MftEnumerator)
        } else {
            Box::new(self.walker())
        }
    }

    /// Short description for reports.
    pub fn describe(&self) -> String {
        if self.mft {
            "MFT reader".to_owned()
        } else {
            format!("Walker, {} threads", self.walker().threads())
        }
    }

    fn policy(&self) -> ExclusionPolicy {
        let mut rules = if self.no_default_exclusions {
            Vec::new()
        } else {
            ExclusionPolicy::default_rules()
        };
        rules.extend(
            self.exclude_dirs
                .iter()
                .cloned()
                .map(ExclusionRule::DirName),
        );
        rules.extend(self.exclude_paths.iter().cloned().map(ExclusionRule::Path));
        ExclusionPolicy::new(rules)
    }
}

type CliResult = Result<(), String>;

fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&cli.log)
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    let result = match &cli.command {
        Command::Bench { path, walk, out } => bench::run(path, walk, out.as_deref()),
        command => match open_store(cli.db.as_deref()) {
            Ok(store) => run(&store, command),
            Err(e) => Err(e),
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `%LOCALAPPDATA%\PrismDev`, the folder for every development catalog (TESTING S4).
pub fn dev_data_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|d| PathBuf::from(d).join("PrismDev"))
        .ok_or_else(|| "LOCALAPPDATA is not set".to_owned())
}

fn open_store(db: Option<&Path>) -> Result<Store, String> {
    let path = match db {
        Some(p) => p.to_path_buf(),
        None => dev_data_dir()?.join("dev-catalog.db"),
    };
    Store::open(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))
}

fn run(store: &Store, command: &Command) -> CliResult {
    match command {
        Command::Scan { path, walk } => scan(store, path, walk),
        Command::Volumes => list_volumes(store),
        Command::Detect { record } => detect(store, *record),
        Command::Ls { path } => ls(store, path),
        Command::Du { path, top } => du(store, path, *top),
        Command::Stats => stats(store),
        Command::Check => check(store),
        Command::Bench { .. } => Ok(()),
    }
}

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Absolute form of a user path, without a trailing separator except for drive roots.
pub fn normalize_root(path: &Path) -> Result<String, String> {
    let raw = path.as_os_str().to_string_lossy();
    let raw = if raw.len() == 2 && raw.ends_with(':') {
        format!("{raw}\\")
    } else {
        raw.into_owned()
    };
    let abs = std::path::absolute(Path::new(&raw)).map_err(err)?;
    let mut s = abs.display().to_string().replace('/', "\\");
    while s.ends_with('\\') && !(s.len() == 3 && s.as_bytes().get(1) == Some(&b':')) {
        s.pop();
    }
    Ok(s)
}

/// Runs an indexing while printing progress to stderr.
pub fn index_with_progress(
    store: &Store,
    volume: VolumeId,
    root: &Path,
    walk: &WalkArgs,
) -> Result<IndexReport, String> {
    let progress = IndexProgress::default();
    let cancel = CancelToken::new();
    let enumerator = walk.enumerator();
    let policy = walk.policy();
    std::thread::scope(|s| {
        let job = s.spawn(|| {
            indexer::index(
                store,
                enumerator.as_ref(),
                &IndexRequest {
                    volume,
                    path: root,
                    relative: &[],
                    target: ReconcileTarget::VolumeRoot,
                    policy: &policy,
                },
                &cancel,
                &progress,
            )
        });
        while !job.is_finished() {
            std::thread::sleep(Duration::from_millis(200));
            eprint!(
                "\r  scanned {:>10}   reconciled {:>10}",
                progress.scanned.load(Ordering::Relaxed),
                progress.reconciled.load(Ordering::Relaxed)
            );
        }
        eprintln!();
        job.join()
            .map_err(|_| "the indexing thread panicked".to_owned())?
            .map_err(err)
    })
}

/// Finds the development volume registered at `mount` (a folder that is not a volume root),
/// or adds it. Volumes with a real identity are never matched by path.
fn dev_volume(store: &Store, mount: &str) -> Result<VolumeId, String> {
    let m = mount.to_owned();
    let existing = store
        .read(move |c| {
            Ok(store::volumes(c)?.into_iter().find(|v| {
                v.info.fs_serial.is_none()
                    && v.info.unc_path.is_none()
                    && v.info
                        .last_mount
                        .as_deref()
                        .is_some_and(|x| names::eq_ignore_case(x, &m))
            }))
        })
        .map_err(err)?;
    match existing {
        Some(v) => Ok(v.id),
        None => {
            let info = VolumeInfo::dev_folder(mount);
            store
                .write(move |w| store::insert_volume(w, &info, VolumeStatus::New))
                .map_err(err)
        }
    }
}

fn same_mount(a: &str, b: &str) -> bool {
    names::eq_ignore_case(a.trim_end_matches('\\'), b.trim_end_matches('\\'))
}

/// The volume for `mount`: a connected volume whose root it is (matched by identity, F02-AC3),
/// a UNC share (F02-AC5), or else a development folder volume. Also returns the path to read
/// it through: network shares are always read through their UNC path, because an elevated
/// process does not see the user's mapped letters (ADR-0016).
pub fn volume_for(store: &Store, mount: &str) -> Result<(VolumeId, String), String> {
    let detected = if mount.starts_with(r"\\") && !mount.starts_with(r"\\?\") {
        Some(DetectedVolume {
            info: VolumeInfo {
                kind: VolumeKind::Network,
                unc_path: Some(mount.to_owned()),
                ..VolumeInfo::dev_folder(mount)
            },
            mounts: vec![mount.to_owned()],
            disks: Vec::new(),
        })
    } else {
        WinVolumeSource
            .detect()
            .map_err(err)?
            .into_iter()
            .find(|d| d.mounts.iter().any(|m| same_mount(m, mount)))
    };
    let Some(mut detected) = detected else {
        return Ok((dev_volume(store, mount)?, mount.to_owned()));
    };
    let access = match (&detected.info.kind, &detected.info.unc_path) {
        (VolumeKind::Network, Some(unc)) => unc.clone(),
        _ => mount.to_owned(),
    };
    // Index through the path the user gave (a letter or a mount folder).
    detected.info.last_mount = Some(mount.to_owned());
    let known = store.read(store::volumes).map_err(err)?;
    let matched = match_volumes(std::slice::from_ref(&detected), &known)
        .pop()
        .unwrap_or(VolumeMatch::New);
    if let VolumeMatch::Ambiguous(ids) = &matched {
        return Err(format!(
            "{mount} could be one of the catalog volumes {ids:?}: the choice belongs to the \
             user (F02-AC3), not supported by the development CLI"
        ));
    }
    let id = volumes::record_detected(store, &detected, &matched)
        .map_err(err)?
        .ok_or_else(|| "volume not recorded".to_owned())?;
    Ok((id, access))
}

fn scan(store: &Store, path: &Path, walk: &WalkArgs) -> CliResult {
    let mount = normalize_root(path)?;
    let (volume, access) = volume_for(store, &mount)?;
    eprintln!("indexing {mount} through {access} (volume {volume}) ...");
    let report = index_with_progress(store, volume, Path::new(&access), walk)?;
    print_report(&report);
    Ok(())
}

fn detect(store: &Store, record: bool) -> CliResult {
    let detected = WinVolumeSource.detect().map_err(err)?;
    let known = store.read(store::volumes).map_err(err)?;
    let matches = match_volumes(&detected, &known);
    println!(
        "{:<8} {:<9} {:<6} {:>10}  {:<18} {:<8} {:<14} label / UNC",
        "mount", "kind", "fs", "capacity", "serial", "bus", "catalog"
    );
    for (d, m) in detected.iter().zip(&matches) {
        let i = &d.info;
        println!(
            "{:<8} {:<9} {:<6} {:>10}  {:<18} {:<8} {:<14} {}",
            i.last_mount.as_deref().unwrap_or("-"),
            format!("{:?}", i.display_kind()).to_lowercase(),
            i.fs_type.as_deref().unwrap_or("-"),
            i.capacity.map_or_else(|| "-".into(), human_size),
            i.fs_serial
                .map_or_else(|| "-".into(), |s| format!("{s:016X}")),
            i.bus.map_or("-", |b| b.as_str()),
            match m {
                VolumeMatch::Known(id) => format!("volume {id}"),
                VolumeMatch::New => "new".to_owned(),
                VolumeMatch::Ambiguous(ids) => format!("ambiguous {ids:?}"),
            },
            i.unc_path
                .as_deref()
                .or(i.os_label.as_deref())
                .unwrap_or("")
        );
        if record {
            volumes::record_detected(store, d, m).map_err(err)?;
        }
    }
    Ok(())
}

pub fn print_report(r: &IndexReport) {
    let s = &r.scan;
    println!(
        "found {} files, {} folders ({} excluded, {} unreadable folders) in {:.2?}",
        s.files, s.dirs, s.excluded, s.unreadable_dirs, r.scan_time
    );
    let c = &r.reconcile;
    println!(
        "catalog: {} inserted, {} updated, {} moved, {} deleted, {} unchanged in {:.2?}",
        c.inserted, c.updated, c.moved, c.deleted, c.unchanged, r.reconcile_time
    );
    for e in &s.errors {
        println!("  unreadable: {e}");
    }
}

fn list_volumes(store: &Store) -> CliResult {
    let rows = store
        .read(|c| {
            let mut out = Vec::new();
            for v in store::volumes(c)? {
                let n = store::entry_count(c, v.id)?;
                out.push((v, n));
            }
            Ok(out)
        })
        .map_err(err)?;
    println!(
        "{:>4}  {:<15} {:<9} {:>12}  {:<17}  mount",
        "id", "status", "kind", "entries", "indexed"
    );
    for (v, n) in rows {
        let kind = if v.info.fs_serial.is_none() && v.info.unc_path.is_none() {
            "folder".to_owned()
        } else {
            format!("{:?}", v.info.display_kind()).to_lowercase()
        };
        println!(
            "{:>4}  {:<15} {:<9} {:>12}  {:<17}  {}",
            v.id.0,
            v.status.as_str(),
            kind,
            n,
            v.last_indexed_at.map_or_else(|| "-".into(), human_time),
            v.info.last_mount.unwrap_or_default()
        );
    }
    Ok(())
}

/// Finds the volume whose mount contains `path` (longest match) and the entry at `path`.
fn resolve(store: &Store, path: &Path) -> Result<(Volume, EntryRecord), String> {
    let full = normalize_root(path)?;
    store
        .read(move |c| {
            let mut best: Option<(Volume, String)> = None;
            for v in store::volumes(c)? {
                let Some(mount) = v.info.last_mount.clone() else {
                    continue;
                };
                let Some(rest) = strip_prefix_ignore_case(&full, &mount) else {
                    continue;
                };
                let longer = best.as_ref().is_none_or(|(b, _)| {
                    b.info.last_mount.as_deref().map_or(0, str::len) < mount.len()
                });
                if longer {
                    best = Some((v, rest.to_owned()));
                }
            }
            let Some((volume, rel)) = best else {
                return Err(store::StoreError::NotFound(format!(
                    "{full} is not in any scanned volume (run `prism-cli scan` first)"
                )));
            };
            let entry = store::resolve_relative_path(c, volume.id, &rel)?.ok_or_else(|| {
                store::StoreError::NotFound(format!("{full} is not in the catalog"))
            })?;
            Ok((volume, entry))
        })
        .map_err(err)
}

fn strip_prefix_ignore_case<'a>(full: &'a str, prefix: &str) -> Option<&'a str> {
    let head = full.get(..prefix.len())?;
    if !names::eq_ignore_case(head, prefix) {
        return None;
    }
    let rest = full.get(prefix.len()..)?;
    if rest.is_empty() || prefix.ends_with('\\') {
        Some(rest)
    } else {
        rest.strip_prefix('\\')
    }
}

fn ls(store: &Store, path: &Path) -> CliResult {
    let (volume, dir) = resolve(store, path)?;
    let id = dir.id;
    let mut rows = store
        .read(move |c| {
            let mut out = Vec::new();
            for e in store::children(c, id)? {
                let stats = if e.is_dir {
                    store::dir_stats(c, e.id)?
                } else {
                    None
                };
                out.push((e, stats));
            }
            Ok(out)
        })
        .map_err(err)?;
    if volume.status != VolumeStatus::Indexed {
        println!("(volume status: {})", volume.status.as_str());
    }
    rows.sort_by(|a, b| b.0.is_dir.cmp(&a.0.is_dir).then(a.0.name.cmp(&b.0.name)));
    for (e, stats) in rows {
        let (size, files) = match stats {
            Some(s) => (s.total_size, format!("{} files", s.file_count)),
            None => (e.meta.size, String::new()),
        };
        println!(
            "{:<5} {:>10}  {:>12}  {:<17}  {}",
            if e.is_dir { "<DIR>" } else { "" },
            human_size(size),
            files,
            e.meta
                .modified
                .map_or_else(|| "-".into(), |t| human_time(t.to_unix_millis())),
            e.name
        );
    }
    Ok(())
}

fn du(store: &Store, path: &Path, top: usize) -> CliResult {
    let (_, dir) = resolve(store, path)?;
    let id = dir.id;
    let (total, mut rows) = store
        .read(move |c| {
            let total = store::dir_stats(c, id)?.unwrap_or_default();
            let mut out = Vec::new();
            for e in store::children(c, id)? {
                let size = if e.is_dir {
                    store::dir_stats(c, e.id)?.unwrap_or_default().total_size
                } else {
                    e.meta.size
                };
                out.push((size, e.is_dir, e.name));
            }
            Ok((total, out))
        })
        .map_err(err)?;
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    println!(
        "{}  total {} in {} files, {} folders",
        path.display(),
        human_size(total.total_size),
        total.file_count,
        total.dir_count
    );
    for (size, is_dir, name) in rows.into_iter().take(top) {
        let pct = if total.total_size == 0 {
            0.0
        } else {
            size as f64 * 100.0 / total.total_size as f64
        };
        println!(
            "{:>10}  {:>5.1}%  {}{}",
            human_size(size),
            pct,
            name,
            if is_dir { "\\" } else { "" }
        );
    }
    Ok(())
}

fn stats(store: &Store) -> CliResult {
    let (db_size, rows) = store
        .read(|c| {
            let mut rows = Vec::new();
            for v in store::volumes(c)? {
                let n = store::entry_count(c, v.id)?;
                let total = match store::root_entry(c, v.id)? {
                    Some(root) => store::dir_stats(c, root.id)?.unwrap_or_default(),
                    None => prism_core::model::DirStats::default(),
                };
                rows.push((v, n, total));
            }
            Ok((store::database_size(c)?, rows))
        })
        .map_err(err)?;
    let entries: u64 = rows.iter().map(|r| r.1).sum();
    println!("catalog  {}", store.path().display());
    println!(
        "size     {} for {} entries ({} per entry)",
        human_size(db_size),
        entries,
        if entries == 0 {
            "-".into()
        } else {
            format!("{} B", db_size / entries)
        }
    );
    for (v, n, t) in rows {
        println!(
            "  volume {:>3}  {:>10} entries  {:>10}  {} files  {} folders  {}",
            v.id.0,
            n,
            human_size(t.total_size),
            t.file_count,
            t.dir_count,
            v.info.last_mount.unwrap_or_default()
        );
    }
    Ok(())
}

fn check(store: &Store) -> CliResult {
    let problems = store.read(store::check_consistency).map_err(err)?;
    if problems.is_empty() {
        println!("catalog is consistent");
        Ok(())
    } else {
        for p in &problems {
            println!("{p}");
        }
        Err(format!("{} problems found", problems.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_matching_respects_component_boundaries() {
        assert_eq!(
            strip_prefix_ignore_case(r"C:\Users\me", r"C:\"),
            Some(r"Users\me")
        );
        assert_eq!(
            strip_prefix_ignore_case(r"d:\foto\2024", r"D:\Foto"),
            Some("2024")
        );
        assert_eq!(strip_prefix_ignore_case(r"D:\Foto", r"D:\Foto"), Some(""));
        assert_eq!(strip_prefix_ignore_case(r"D:\Fotografie", r"D:\Foto"), None);
        assert_eq!(strip_prefix_ignore_case(r"E:\x", r"D:\"), None);
    }

    #[test]
    fn roots_are_normalized() {
        assert_eq!(normalize_root(Path::new("C:")).unwrap(), r"C:\");
        assert_eq!(normalize_root(Path::new(r"C:\")).unwrap(), r"C:\");
        assert_eq!(
            normalize_root(Path::new("C:/Windows/")).unwrap(),
            r"C:\Windows"
        );
    }
}
