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
use prism_core::model::{EntryRecord, Volume, VolumeId, VolumeInfo, VolumeStatus};
use prism_core::names;
use prism_core::reconcile::ReconcileTarget;
use prism_core::scan::WalkEnumerator;
use prism_core::store::{self, Store};

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
    /// Listing threads (default: number of CPUs, at most 8).
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
    fn enumerator(&self) -> WalkEnumerator {
        self.threads
            .map_or_else(WalkEnumerator::default, WalkEnumerator::with_threads)
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
                &enumerator,
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

/// Finds the development volume registered at `mount`, or adds it.
pub fn dev_volume(store: &Store, mount: &str) -> Result<VolumeId, String> {
    let m = mount.to_owned();
    let existing = store
        .read(move |c| store::volume_by_mount(c, &m))
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

fn scan(store: &Store, path: &Path, walk: &WalkArgs) -> CliResult {
    let mount = normalize_root(path)?;
    let volume = dev_volume(store, &mount)?;
    eprintln!("indexing {mount} (volume {volume}) ...");
    let report = index_with_progress(store, volume, Path::new(&mount), walk)?;
    print_report(&report);
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
        "{:>4}  {:<15} {:>12}  {:<17}  mount",
        "id", "status", "entries", "indexed"
    );
    for (v, n) in rows {
        println!(
            "{:>4}  {:<15} {:>12}  {:<17}  {}",
            v.id.0,
            v.status.as_str(),
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
