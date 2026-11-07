//! Removing files for `prism_core::fileops` (ADR-0012): to the Windows Recycle Bin through
//! `IFileOperation`, or permanently where there is no Recycle Bin and the user confirmed it.
//!
//! The Recycle Bin operation runs without any dialog, except one: if Windows cannot recycle an
//! item (too large for the Bin, a volume without Bin) it asks the user before destroying it
//! (`FOF_WANTNUKEWARNING`), instead of deleting it silently. Prism uses the Recycle Bin only on
//! local fixed volumes anyway.

use std::path::{Path, PathBuf};

use prism_core::fileops::{Disposal, Remover};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
    FOFX_RECYCLEONDELETE, FileOperation, IFileOperation, IFileOperationProgressSink, IShellItem,
    SHCreateItemFromParsingName, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
    SHEmptyRecycleBinW, SHQUERYRBINFO, SHQueryRecycleBinW,
};
use windows::core::HSTRING;

/// The [`Remover`] of the application.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShellRemover;

impl Remover for ShellRemover {
    fn remove(&self, paths: &[PathBuf], disposal: Disposal) -> Vec<(PathBuf, String)> {
        match disposal {
            Disposal::RecycleBin => match recycle(paths) {
                Ok(()) => Vec::new(),
                Err(e) => paths.iter().map(|p| (p.clone(), e.message())).collect(),
            },
            Disposal::Permanent => paths
                .iter()
                .filter_map(|p| {
                    let r = if p.is_dir() {
                        std::fs::remove_dir_all(p)
                    } else {
                        std::fs::remove_file(p)
                    };
                    r.err().map(|e| (p.clone(), e.to_string()))
                })
                .collect(),
        }
    }
}

/// COM initialized on the current thread for the duration of a call.
struct ComScope(bool);

impl ComScope {
    fn enter() -> Self {
        // SAFETY: initializes COM for this thread; balanced in Drop when it succeeded. On a
        // thread already in another apartment the call fails and that apartment is used.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self(hr.is_ok())
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful CoInitializeEx of `enter` on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// Moves `paths` to the Recycle Bin in one operation. Items that could not be moved stay on
/// the disk; the caller checks the disk afterwards.
///
/// # Errors
/// COM errors (the shell refused the whole operation).
pub fn recycle(paths: &[PathBuf]) -> windows::core::Result<()> {
    let _com = ComScope::enter();
    // SAFETY: plain COM calls on interfaces created here, on a thread where COM is initialized;
    // the HSTRINGs outlive the calls that use them.
    unsafe {
        let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        op.SetOperationFlags(
            FOF_ALLOWUNDO
                | FOFX_RECYCLEONDELETE
                | FOF_NOCONFIRMATION
                | FOF_SILENT
                | FOF_NOERRORUI
                | FOF_WANTNUKEWARNING,
        )?;
        for path in paths {
            let item: IShellItem =
                SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None)?;
            op.DeleteItem(&item, None::<&IFileOperationProgressSink>)?;
        }
        op.PerformOperations()?;
    }
    Ok(())
}

/// Items and bytes in the Recycle Bin of a volume (`C:\`), for the user running Prism.
#[must_use]
pub fn recycle_bin_info(root: &Path) -> Option<(u64, u64)> {
    let mut info = SHQUERYRBINFO {
        cbSize: u32::try_from(size_of::<SHQUERYRBINFO>()).ok()?,
        ..Default::default()
    };
    // SAFETY: `info` is a writable SHQUERYRBINFO with its size set; the root path outlives the
    // call.
    unsafe { SHQueryRecycleBinW(&HSTRING::from(root.as_os_str()), &raw mut info) }.ok()?;
    Some((
        u64::try_from(info.i64NumItems).unwrap_or(0),
        u64::try_from(info.i64Size).unwrap_or(0),
    ))
}

/// Empties the Recycle Bin of **one** volume for the current user, without any dialog. Used by
/// the tests on the test volume only; the application never empties the user's Recycle Bin.
///
/// # Errors
/// Shell errors.
pub fn empty_recycle_bin_of(root: &Path) -> windows::core::Result<()> {
    // SAFETY: the root path outlives the call; no owner window.
    unsafe {
        SHEmptyRecycleBinW(
            None,
            &HSTRING::from(root.as_os_str()),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    }
}
