//! The Explorer menu entries (F18-AC1): *Analyse with Prism* and *Find duplicates here* on
//! folders, drives and the background of a folder, each running `prism-shell.exe` with the
//! action and the path. The installer registers them for the current user and the uninstaller
//! removes them (F18-AC3); development builds never touch the real keys.

use std::path::Path;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};

use crate::volumes::wide;

/// Where the entries go for the current user.
pub const CLASSES: &str = r"Software\Classes";

/// The objects that get the entries, and the placeholder of their path.
const TARGETS: [(&str, &str); 3] = [
    (r"Directory\shell", "%V"),
    (r"Drive\shell", "%V"),
    (r"Directory\Background\shell", "%V"),
];

/// The texts of the two entries, in the user's language.
pub struct MenuTexts<'a> {
    pub analyze: &'a str,
    pub duplicates: &'a str,
}

#[derive(Debug, thiserror::Error)]
#[error("registry error {0} on {1}")]
pub struct MenuError(u32, String);

/// Creates (or opens) `HKCU\path` and sets its string values (`""` is the default value).
fn set_key(path: &str, values: &[(&str, &str)]) -> Result<(), MenuError> {
    let wpath = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: HKEY_CURRENT_USER is predefined; `wpath` is NUL-terminated; `key` receives the
    // opened key; class, security and disposition are optional.
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wpath.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &raw mut key,
            std::ptr::null_mut(),
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(MenuError(rc, path.to_owned()));
    }
    let mut result = Ok(());
    for (name, value) in values {
        let wname = wide(name);
        let wvalue = wide(value);
        let bytes = u32::try_from(wvalue.len() * 2).unwrap_or(0);
        // SAFETY: `key` is open for writing; the name and data are NUL-terminated UTF-16 and
        // `bytes` is the size of the data including the terminator.
        let rc = unsafe {
            RegSetValueExW(
                key,
                if name.is_empty() {
                    std::ptr::null()
                } else {
                    wname.as_ptr()
                },
                0,
                REG_SZ,
                wvalue.as_ptr().cast(),
                bytes,
            )
        };
        if rc != ERROR_SUCCESS {
            result = Err(MenuError(rc, format!(r"{path}\{name}")));
            break;
        }
    }
    // SAFETY: `key` was opened above and is closed once.
    unsafe { RegCloseKey(key) };
    result
}

/// Registers the entries under `HKCU\<root>` (normally [`CLASSES`]) for `shell_exe`, with
/// the icon of `icon_exe`.
///
/// # Errors
/// [`MenuError`] when a key cannot be written.
pub fn register(
    root: &str,
    shell_exe: &Path,
    icon_exe: &Path,
    texts: &MenuTexts<'_>,
) -> Result<(), MenuError> {
    let exe = shell_exe.to_string_lossy();
    let icon = icon_exe.to_string_lossy();
    for (target, placeholder) in TARGETS {
        for (verb, action, text) in [
            ("Prism.Analyze", "analyze", texts.analyze),
            ("Prism.Duplicates", "duplicates", texts.duplicates),
        ] {
            let key = format!(r"{root}\{target}\{verb}");
            set_key(&key, &[("", text), ("Icon", &icon)])?;
            set_key(
                &format!(r"{key}\command"),
                &[("", &format!(r#""{exe}" {action} "{placeholder}""#))],
            )?;
        }
    }
    Ok(())
}

/// Removes the entries from `HKCU\<root>`; entries already absent are not an error.
///
/// # Errors
/// [`MenuError`] when a key exists but cannot be removed.
pub fn unregister(root: &str) -> Result<(), MenuError> {
    for (target, _) in TARGETS {
        for verb in ["Prism.Analyze", "Prism.Duplicates"] {
            let key = format!(r"{root}\{target}\{verb}");
            let wkey = wide(&key);
            // SAFETY: HKEY_CURRENT_USER is predefined; `wkey` is NUL-terminated.
            let rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wkey.as_ptr()) };
            // ERROR_FILE_NOT_FOUND (2): nothing to remove.
            if rc != ERROR_SUCCESS && rc != 2 {
                return Err(MenuError(rc, key));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};

    fn read(path: &str) -> Option<String> {
        let wpath = wide(path);
        let mut data = [0u16; 512];
        let mut size = u32::try_from(size_of_val(&data)).unwrap_or(0);
        // SAFETY: predefined root; NUL-terminated path; `data` writable for `size` bytes; the
        // default value is read with a null value name.
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wpath.as_ptr(),
                std::ptr::null(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        (rc == ERROR_SUCCESS).then(|| {
            let len = data.iter().position(|&c| c == 0).unwrap_or(0);
            String::from_utf16_lossy(&data[..len])
        })
    }

    // F18-AC1/AC3 on a scratch key of the test (never the real Software\Classes).
    #[test]
    fn entries_are_registered_and_removed() {
        let root = format!(r"Software\PrismDevTests\menu-{}", std::process::id());
        let texts = MenuTexts {
            analyze: "Analizza con Prism",
            duplicates: "Cerca duplicati qui",
        };
        register(
            &root,
            Path::new(r"C:\Program Files\Prism\prism-shell.exe"),
            Path::new(r"C:\Program Files\Prism\prism.exe"),
            &texts,
        )
        .unwrap();
        let analyze = format!(r"{root}\Directory\shell\Prism.Analyze");
        assert_eq!(read(&analyze).as_deref(), Some("Analizza con Prism"));
        assert_eq!(
            read(&format!(r"{analyze}\command")).as_deref(),
            Some(r#""C:\Program Files\Prism\prism-shell.exe" analyze "%V""#)
        );
        assert_eq!(
            read(&format!(r"{root}\Drive\shell\Prism.Duplicates")).as_deref(),
            Some("Cerca duplicati qui")
        );
        unregister(&root).unwrap();
        assert!(read(&analyze).is_none());
        unregister(&root).unwrap();
        // Remove the scratch root itself.
        let wroot = wide(r"Software\PrismDevTests");
        // SAFETY: predefined root; NUL-terminated path.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wroot.as_ptr()) };
    }
}
