//! Facts about the system for the diagnostic package (F20-AC2).

use windows_sys::Win32::Foundation::NO_ERROR;
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A string value under `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`.
fn current_version(name: &str) -> Option<String> {
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let value = wide(name);
    let mut data = [0u16; 256];
    let mut size = u32::try_from(size_of_val(&data)).unwrap_or(0);
    // SAFETY: HKEY_LOCAL_MACHINE is a predefined key; `key` and `value` are NUL-terminated;
    // `data` is writable for `size` bytes; the type output is not needed.
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    if rc != NO_ERROR {
        return None;
    }
    let len = data.iter().position(|&c| c == 0).unwrap_or(data.len());
    Some(String::from_utf16_lossy(&data[..len]))
}

/// "Windows 11 Pro 25H2 (build 26300)", as far as the registry tells.
#[must_use]
pub fn windows_version() -> String {
    let product = current_version("ProductName").unwrap_or_else(|| "Windows".to_owned());
    let display = current_version("DisplayVersion").unwrap_or_default();
    let build = current_version("CurrentBuild").unwrap_or_default();
    // ProductName still says "Windows 10" on Windows 11: the build number tells them apart.
    let product = match build.parse::<u32>() {
        Ok(b) if b >= 22_000 => product.replacen("Windows 10", "Windows 11", 1),
        _ => product,
    };
    format!("{product} {display} (build {build})")
        .replace("  ", " ")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_windows_version_is_read() {
        let v = super::windows_version();
        assert!(v.starts_with("Windows"), "{v}");
        assert!(v.contains("build "), "{v}");
    }
}
