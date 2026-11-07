//! The sandbox guard for destructive file operations (docs/TESTING.md, rules S1 and S2).
//!
//! In tests and in the development commands, every operation that deletes, moves, overwrites
//! or creates files goes through [`GuardedFs`]. A path is accepted only if, after resolving
//! junctions and symbolic links of its existing part, it lies strictly inside one of the
//! allowed roots (temporary folders created by the test, or the test volume `PRISMTEST`).
//! Anything else is refused, so a mistake in a test cannot reach real files.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// A path refused by the guard, or an I/O error of the guarded operation.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("refused: {0} is outside the sandbox")]
    Outside(PathBuf),
    #[error("refused: {0} is not an absolute path without `..`")]
    NotNormalized(PathBuf),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> SandboxError + '_ {
    move |source| SandboxError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Decides whether a path is inside the sandbox.
#[derive(Clone, Debug)]
pub struct SandboxGuard {
    roots: Vec<PathBuf>,
}

impl SandboxGuard {
    /// A guard allowing everything strictly inside the given folders, which must exist.
    ///
    /// # Errors
    /// [`SandboxError::Io`] if a root cannot be resolved.
    pub fn new<P: AsRef<Path>>(roots: impl IntoIterator<Item = P>) -> Result<Self, SandboxError> {
        let roots = roots
            .into_iter()
            .map(|r| fs::canonicalize(r.as_ref()).map_err(io_err(r.as_ref())))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { roots })
    }

    /// Resolves `path` (following a final link too) and checks it. Use for operations that
    /// follow links, like writing a file. Returns the resolved path to operate on.
    ///
    /// # Errors
    /// [`SandboxError::NotNormalized`] for relative paths or paths with `..`;
    /// [`SandboxError::Outside`] if the resolved path is not strictly inside a root.
    pub fn check(&self, path: &Path) -> Result<PathBuf, SandboxError> {
        self.check_impl(path, true)
    }

    /// Like [`SandboxGuard::check`], but a final link is not followed: use for operations on
    /// the link itself (delete, rename).
    ///
    /// # Errors
    /// See [`SandboxGuard::check`].
    pub fn check_no_follow(&self, path: &Path) -> Result<PathBuf, SandboxError> {
        self.check_impl(path, false)
    }

    fn check_impl(&self, path: &Path, follow_last: bool) -> Result<PathBuf, SandboxError> {
        if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
            return Err(SandboxError::NotNormalized(path.to_path_buf()));
        }
        let outside = || SandboxError::Outside(path.to_path_buf());
        // Resolve the longest existing prefix (following junctions and links), then append
        // the part that does not exist yet (and the final name, when not following it).
        let mut existing = path.to_path_buf();
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        if !follow_last {
            tail.push(existing.file_name().ok_or_else(outside)?.to_os_string());
            existing.pop();
        }
        let resolved = loop {
            match fs::canonicalize(&existing) {
                Ok(p) => break p,
                Err(_) => {
                    tail.push(existing.file_name().ok_or_else(outside)?.to_os_string());
                    if !existing.pop() {
                        return Err(outside());
                    }
                }
            }
        };
        let mut full = resolved;
        for part in tail.iter().rev() {
            full.push(part);
        }
        let inside = self
            .roots
            .iter()
            .any(|root| full != *root && full.starts_with(root));
        if inside { Ok(full) } else { Err(outside()) }
    }
}

/// File operations that modify the disk, each checked by a [`SandboxGuard`].
#[derive(Clone, Debug)]
pub struct GuardedFs {
    guard: SandboxGuard,
}

impl GuardedFs {
    #[must_use]
    pub fn new(guard: SandboxGuard) -> Self {
        Self { guard }
    }

    /// Creates a folder and its missing parents.
    ///
    /// # Errors
    /// [`SandboxError`] if refused or on I/O errors.
    pub fn create_dir_all(&self, path: &Path) -> Result<(), SandboxError> {
        let p = self.guard.check(path)?;
        fs::create_dir_all(&p).map_err(io_err(path))
    }

    /// Creates or overwrites a file with `contents`.
    ///
    /// # Errors
    /// [`SandboxError`] if refused or on I/O errors.
    pub fn write(&self, path: &Path, contents: &[u8]) -> Result<(), SandboxError> {
        let p = self.guard.check(path)?;
        fs::write(&p, contents).map_err(io_err(path))
    }

    /// Sets the last-modified time of a file.
    ///
    /// # Errors
    /// [`SandboxError`] if refused or on I/O errors.
    pub fn set_modified(&self, path: &Path, time: SystemTime) -> Result<(), SandboxError> {
        let p = self.guard.check(path)?;
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .map_err(io_err(path))?;
        file.set_modified(time).map_err(io_err(path))
    }

    /// Deletes a file.
    ///
    /// # Errors
    /// [`SandboxError`] if refused or on I/O errors.
    pub fn remove_file(&self, path: &Path) -> Result<(), SandboxError> {
        let p = self.guard.check_no_follow(path)?;
        fs::remove_file(&p).map_err(io_err(path))
    }

    /// Deletes a folder and its content (links inside are removed, not followed).
    ///
    /// # Errors
    /// [`SandboxError`] if refused or on I/O errors.
    pub fn remove_dir_all(&self, path: &Path) -> Result<(), SandboxError> {
        let p = self.guard.check_no_follow(path)?;
        fs::remove_dir_all(&p).map_err(io_err(path))
    }

    /// Renames or moves a file or folder; both paths must be inside the sandbox.
    ///
    /// # Errors
    /// [`SandboxError`] if refused or on I/O errors.
    pub fn rename(&self, from: &Path, to: &Path) -> Result<(), SandboxError> {
        let f = self.guard.check_no_follow(from)?;
        let t = self.guard.check_no_follow(to)?;
        fs::rename(&f, &t).map_err(io_err(from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_paths_inside_and_refuses_outside() {
        let inside = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let guard = SandboxGuard::new([inside.path()]).unwrap();
        let fs = GuardedFs::new(guard.clone());

        let file = inside.path().join("a").join("b.txt");
        fs.create_dir_all(file.parent().unwrap()).unwrap();
        fs.write(&file, b"x").unwrap();
        fs.remove_file(&file).unwrap();

        let victim = outside.path().join("keep.txt");
        std::fs::write(&victim, b"keep").unwrap();
        assert!(matches!(
            fs.remove_file(&victim),
            Err(SandboxError::Outside(_))
        ));
        assert!(matches!(
            fs.write(&victim, b"overwritten"),
            Err(SandboxError::Outside(_))
        ));
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        // The root itself is not a valid target.
        assert!(matches!(
            fs.remove_dir_all(inside.path()),
            Err(SandboxError::Outside(_))
        ));
    }

    #[test]
    fn refuses_relative_and_parent_components() {
        let inside = tempfile::tempdir().unwrap();
        let guard = SandboxGuard::new([inside.path()]).unwrap();
        assert!(matches!(
            guard.check(Path::new("relative.txt")),
            Err(SandboxError::NotNormalized(_))
        ));
        let escape = inside.path().join("..").join("escape.txt");
        assert!(matches!(
            guard.check(&escape),
            Err(SandboxError::NotNormalized(_))
        ));
    }

    #[test]
    fn rename_needs_both_ends_inside() {
        let inside = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let fs = GuardedFs::new(SandboxGuard::new([inside.path()]).unwrap());
        let src = inside.path().join("f.txt");
        fs.write(&src, b"x").unwrap();
        assert!(fs.rename(&src, &outside.path().join("f.txt")).is_err());
        assert!(src.exists());
        fs.rename(&src, &inside.path().join("g.txt")).unwrap();
        assert!(inside.path().join("g.txt").exists());
    }

    /// A junction inside the sandbox pointing outside must not let operations escape.
    #[cfg(windows)]
    #[test]
    fn refuses_paths_through_a_junction_to_outside() {
        let inside = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("keep.txt");
        std::fs::write(&victim, b"keep").unwrap();
        let link = inside.path().join("link");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(status.status.success(), "mklink failed: {status:?}");

        let fs = GuardedFs::new(SandboxGuard::new([inside.path()]).unwrap());
        assert!(matches!(
            fs.remove_file(&link.join("keep.txt")),
            Err(SandboxError::Outside(_))
        ));
        assert!(matches!(
            fs.write(&link.join("new.txt"), b"x"),
            Err(SandboxError::Outside(_))
        ));
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        assert!(!outside.path().join("new.txt").exists());
        // Removing the junction itself is allowed and leaves the target untouched.
        fs.remove_dir_all(&link).unwrap();
        assert!(!link.exists());
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    }
}
