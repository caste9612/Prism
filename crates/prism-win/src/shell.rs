//! Opening files and folders **without** administrator rights (ADR-0003). Prism runs elevated,
//! and a program started by it would inherit its rights: the user's files must instead open
//! with the user's normal rights, as from Explorer.
//!
//! The request is handed to the desktop Explorer, which runs unelevated: through the shell
//! windows of the desktop (`IShellWindows`) Prism reaches the desktop folder view and its
//! `IShellDispatch2::ShellExecute`, which starts the program in Explorer's process context
//! (the technique described by Raymond Chen, "How can I launch an unelevated process from my
//! elevated process?"). It needs a desktop Explorer: without one (another shell, Explorer
//! restarting) the call fails and nothing is started.

use std::path::Path;

use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IDispatch, IServiceProvider,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    CSIDL_DESKTOP, IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellView, IShellWindows,
    SID_STopLevelBrowser, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
use windows::core::{BSTR, Interface};

/// How the started program shows its window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    Normal,
    /// No window (used by the tests to start a console program invisibly).
    Hidden,
}

/// Errors of the unelevated shell.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("the desktop shell is not available: {0}")]
    NoDesktop(windows::core::Error),
    #[error("the shell refused to start {file}: {source}")]
    Execute {
        file: String,
        source: windows::core::Error,
    },
}

/// COM initialized on the current thread for the duration of a call.
struct ComScope(bool);

impl ComScope {
    fn enter() -> Self {
        // SAFETY: initializes COM for this thread; balanced by CoUninitialize in Drop when it
        // succeeded (S_OK or S_FALSE). A thread already in another apartment gets an error and
        // keeps its own initialization, which is enough for the calls below.
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

/// The `IShellDispatch2` of the desktop Explorer.
fn desktop_shell() -> windows::core::Result<IShellDispatch2> {
    // SAFETY: plain COM calls on interfaces obtained just above, on a thread where COM is
    // initialized (`ComScope`); every out pointer refers to a local that outlives the call.
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let location = VARIANT::from(i32::try_from(CSIDL_DESKTOP).unwrap_or(0));
        let root = VARIANT::default();
        let mut hwnd = 0i32;
        let desktop: IDispatch = windows.FindWindowSW(
            &raw const location,
            &raw const root,
            SWC_DESKTOP,
            &raw mut hwnd,
            SWFO_NEEDDISPATCH,
        )?;
        let provider: IServiceProvider = desktop.cast()?;
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        let view: IShellView = browser.QueryActiveShellView()?;
        let background: IDispatch = view.GetItemObject(SVGIO_BACKGROUND)?;
        let folder_view: IShellFolderViewDual = background.cast()?;
        folder_view.Application()?.cast()
    }
}

/// Starts `file` (a program, a document or a folder) with `args`, as the desktop Explorer
/// would: with the user's normal rights.
///
/// # Errors
/// [`ShellError`] if the desktop shell is not reachable or refuses the request.
pub fn execute_unelevated(
    file: &Path,
    args: &str,
    dir: Option<&Path>,
    show: Show,
) -> Result<(), ShellError> {
    let _com = ComScope::enter();
    let shell = desktop_shell().map_err(ShellError::NoDesktop)?;
    let file_text = file.to_string_lossy().into_owned();
    let dir_text = dir
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let show_cmd = match show {
        Show::Normal => SW_SHOWNORMAL.0,
        Show::Hidden => SW_HIDE.0,
    };
    // SAFETY: COM call on a valid interface; the BSTR and VARIANTs outlive the call.
    unsafe {
        shell.ShellExecute(
            &BSTR::from(file_text.as_str()),
            &VARIANT::from(args),
            &VARIANT::from(dir_text.as_str()),
            &VARIANT::from(""),
            &VARIANT::from(show_cmd),
        )
    }
    .map_err(|source| ShellError::Execute {
        file: file_text,
        source,
    })
}

/// Opens a file or folder with its default program, unelevated (F01-AC4, F06 and later).
///
/// # Errors
/// See [`execute_unelevated`].
pub fn open(path: &Path) -> Result<(), ShellError> {
    execute_unelevated(path, "", None, Show::Normal)
}

/// Shows a file or folder selected in an Explorer window, unelevated.
///
/// # Errors
/// See [`execute_unelevated`].
pub fn reveal(path: &Path) -> Result<(), ShellError> {
    let args = format!("/select,\"{}\"", path.display());
    execute_unelevated(Path::new("explorer.exe"), &args, None, Show::Normal)
}
