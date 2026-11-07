//! Windows notifications ("toasts"), for events that matter while Prism is hidden in the
//! notification area, such as a new disk (F02-AC1, REVIEW L8).
//!
//! A desktop application that is not packaged shows toasts under an application identifier
//! (AUMID) registered for the user (`HKCU\Software\Classes\AppUserModelId\<id>`, with the name
//! and icon Windows displays). Release builds register Prism's identifier themselves at
//! startup; development builds borrow the identifier of Windows PowerShell, so that nothing is
//! written to the user's registry while developing.
//!
//! Clicking the toast only dismisses it: opening Prism from the toast needs a COM activator,
//! registered by the installer (T10).

use std::path::Path;

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::HSTRING;
use windows_sys::Win32::Foundation::NO_ERROR;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};

/// Identifier of Windows PowerShell, always registered: used by development builds.
pub const DEV_APP_ID: &str =
    r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

/// Errors of the notifications.
#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error("Windows refused the notification: {0}")]
    Toast(#[from] windows::core::Error),
    #[error("cannot register the application identifier (error {0})")]
    Register(u32),
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The toast content: a title and one line of text.
#[must_use]
pub fn toast_xml(title: &str, body: &str) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text>\
         </binding></visual></toast>",
        xml_escape(title),
        xml_escape(body)
    )
}

/// COM initialized (multithreaded) on the current thread for the duration of a call.
struct ComScope(bool);

impl ComScope {
    fn enter() -> Self {
        // SAFETY: initializes COM for this thread; balanced in Drop when it succeeded. A thread
        // already in a single-threaded apartment gets an error and keeps its apartment, where
        // the WinRT calls below work as well.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
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

/// Shows a toast under the application identifier `app_id`.
///
/// # Errors
/// [`NotifyError::Toast`] if Windows refuses it (notifications disabled, unknown identifier).
pub fn show_toast(app_id: &str, title: &str, body: &str) -> Result<(), NotifyError> {
    let _com = ComScope::enter();
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(toast_xml(title, body)))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?.Show(&toast)?;
    Ok(())
}

fn set_string(subkey: &str, name: &str, value: &str) -> Result<(), NotifyError> {
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let (subkey, name, data) = (wide(subkey), wide(name), wide(value));
    let bytes = u32::try_from(data.len() * 2).map_err(|_| NotifyError::Register(0))?;
    // SAFETY: HKEY_CURRENT_USER is a predefined key; `subkey`, `name` and `data` are
    // NUL-terminated UTF-16 buffers that outlive the call; `bytes` is the size of `data`.
    let rc = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            bytes,
        )
    };
    if rc == NO_ERROR {
        Ok(())
    } else {
        Err(NotifyError::Register(rc))
    }
}

/// Registers the application identifier with the name (and icon) shown on its toasts, for the
/// current user. Idempotent; the uninstaller removes the key (T10).
///
/// # Errors
/// [`NotifyError::Register`] if the registry cannot be written.
pub fn register_app_id(
    app_id: &str,
    display_name: &str,
    icon: Option<&Path>,
) -> Result<(), NotifyError> {
    let key = format!(r"Software\Classes\AppUserModelId\{app_id}");
    set_string(&key, "DisplayName", display_name)?;
    if let Some(icon) = icon {
        set_string(&key, "IconUri", &icon.to_string_lossy())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_content_is_escaped() {
        let xml = toast_xml("New disk: <Foto & Video>", "Index it?");
        assert_eq!(
            xml,
            "<toast><visual><binding template=\"ToastGeneric\">\
             <text>New disk: &lt;Foto &amp; Video&gt;</text><text>Index it?</text>\
             </binding></visual></toast>"
        );
    }

    // Shows a real toast on the desktop: run by hand (`-- --ignored`) to check notifications.
    #[test]
    #[ignore = "shows a notification on the desktop"]
    fn shows_a_real_toast() {
        let shown = show_toast(
            DEV_APP_ID,
            "Prism (test)",
            "A test notification from cargo test.",
        );
        assert!(shown.is_ok(), "{shown:?}");
    }
}
