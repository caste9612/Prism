//! Logins kept in the Windows Credential Manager (REVIEW D66): Windows encrypts them for the
//! user (DPAPI) and shows them under "Windows credentials", where the user can see and remove
//! them. Prism never writes a password anywhere else.

use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, FILETIME, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredEnumerateW,
    CredFree, CredReadW, CredWriteW,
};

use crate::volumes::wide;

/// A user name and its secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    pub user: String,
    pub secret: String,
}

impl std::fmt::Debug for Credential {
    // The secret never reaches a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

/// A NUL-terminated UTF-16 string written by Windows.
///
/// # Safety
/// `p` is null or points to a NUL-terminated UTF-16 string.
unsafe fn from_wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    // SAFETY: the string is NUL-terminated (caller's contract), so every unit read up to the
    // terminator is inside it.
    while unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` units starting at `p` were just read.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) })
}

/// Saves (or replaces) the login called `target`.
///
/// # Errors
/// The Windows error code.
pub fn save(target: &str, user: &str, secret: &str) -> Result<(), u32> {
    let mut target_w = wide(target);
    let mut user_w = wide(user);
    // As `cmdkey` and the Credential Manager write them: UTF-16 without the terminator.
    let mut blob: Vec<u8> = secret.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let cred = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target_w.as_mut_ptr(),
        Comment: std::ptr::null_mut(),
        LastWritten: FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        },
        CredentialBlobSize: u32::try_from(blob.len()).map_err(|_| 87u32)?,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: std::ptr::null_mut(),
        UserName: user_w.as_mut_ptr(),
    };
    // SAFETY: every pointer in `cred` points to a buffer alive for the whole call (the strings
    // are NUL-terminated, the blob has the stated size); Windows copies what it keeps.
    let ok = unsafe { CredWriteW(&raw const cred, 0) };
    // The copy of the secret in memory is cleared before it is released.
    blob.fill(0);
    if ok == 0 {
        // SAFETY: reads the calling thread's last error, set by the failed call above.
        return Err(unsafe { GetLastError() });
    }
    Ok(())
}

/// The login called `target`, if saved.
///
/// # Errors
/// The Windows error code (not for a login that does not exist).
pub fn load(target: &str) -> Result<Option<Credential>, u32> {
    let target_w = wide(target);
    let mut found: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `target_w` is NUL-terminated; `found` receives a buffer allocated by Windows,
    // released below with CredFree.
    let ok = unsafe { CredReadW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0, &raw mut found) };
    if ok == 0 {
        // SAFETY: reads the calling thread's last error, set by the failed call above.
        let e = unsafe { GetLastError() };
        return if e == ERROR_NOT_FOUND {
            Ok(None)
        } else {
            Err(e)
        };
    }
    // SAFETY: on success `found` points to a valid CREDENTIALW whose blob has
    // CredentialBlobSize bytes and whose user name is NUL-terminated or null.
    let credential = unsafe {
        let c = &*found;
        let blob = if c.CredentialBlob.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
        };
        let units: Vec<u16> = blob
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        Credential {
            user: from_wide(c.UserName),
            secret: String::from_utf16_lossy(&units),
        }
    };
    // SAFETY: `found` was allocated by CredReadW and is not used after this.
    unsafe { CredFree(found.cast()) };
    Ok(Some(credential))
}

/// Removes the login called `target`; `false` when there was none.
///
/// # Errors
/// The Windows error code.
pub fn delete(target: &str) -> Result<bool, u32> {
    let target_w = wide(target);
    // SAFETY: `target_w` is NUL-terminated and alive for the call.
    if unsafe { CredDeleteW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
        return Ok(true);
    }
    // SAFETY: reads the calling thread's last error, set by the failed call above.
    match unsafe { GetLastError() } {
        ERROR_NOT_FOUND => Ok(false),
        e => Err(e),
    }
}

/// The names of the saved logins that start with `prefix`.
///
/// # Errors
/// The Windows error code.
pub fn targets(prefix: &str) -> Result<Vec<String>, u32> {
    let filter = wide(&format!("{prefix}*"));
    let mut count = 0u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `filter` is NUL-terminated; `count` and `list` receive the number and an array
    // allocated by Windows, released below with CredFree.
    let ok = unsafe { CredEnumerateW(filter.as_ptr(), 0, &raw mut count, &raw mut list) };
    if ok == 0 {
        // SAFETY: reads the calling thread's last error, set by the failed call above.
        let e = unsafe { GetLastError() };
        return if e == ERROR_NOT_FOUND {
            Ok(Vec::new())
        } else {
            Err(e)
        };
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        // SAFETY: `list` holds `count` valid pointers to CREDENTIALW with NUL-terminated
        // target names.
        let name = unsafe { from_wide((**list.add(i)).TargetName) };
        out.push(name);
    }
    // SAFETY: `list` was allocated by CredEnumerateW and is not used after this.
    unsafe { CredFree(list.cast()) };
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    // A login of the test only, removed at the end (also when the test fails half-way, at the
    // next run: the name is the same).
    #[test]
    fn a_login_is_saved_read_and_removed() {
        let target = "Prism/Test/credentials-roundtrip".to_owned();
        let _ = delete(&target);
        assert_eq!(load(&target).unwrap(), None);
        save(&target, "someone", "pässword 1").unwrap();
        let read = load(&target).unwrap().unwrap();
        assert_eq!(read.user, "someone");
        assert_eq!(read.secret, "pässword 1");
        assert!(!format!("{read:?}").contains("pässword"), "never in a log");
        save(&target, "someone", "second").unwrap();
        assert_eq!(load(&target).unwrap().unwrap().secret, "second");
        assert!(
            targets("Prism/Test/credentials-")
                .unwrap()
                .contains(&target)
        );
        assert!(delete(&target).unwrap());
        assert!(!delete(&target).unwrap());
        assert_eq!(load(&target).unwrap(), None);
        assert!(
            !targets("Prism/Test/credentials-")
                .unwrap()
                .contains(&target)
        );
    }
}
