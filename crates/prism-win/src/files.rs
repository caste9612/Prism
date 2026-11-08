//! Physical identity of a file (REVIEW D62): two paths can lead to the same file — hard links on
//! NTFS, or two shares of a NAS that show the same folder (`\\nas\Photos` and
//! `\\nas\System\.media\Photos`). Deleting "one copy" of such a pair deletes the only one.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo, GetFileInformationByHandleEx, OPEN_EXISTING,
};

/// The volume serial number and the file id the file system (or the server) gives a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    pub volume_serial: u64,
    pub file_id: [u8; 16],
}

/// Whether two identities are one file: the same id on the same volume (hard links), or — for
/// two paths on the same server — the same file number in the low 32 bits of the id. A NAS can
/// show one disk under two shares with a different device part in the high bits (seen on
/// ZimaOS: `0x0000000000085cc2` and `0x0000004000085cc2` for one photo). A coincidence between
/// two real copies only refuses a deletion, never allows one.
#[must_use]
pub fn same_file(a: &FileIdentity, b: &FileIdentity, same_server: bool) -> bool {
    (a.volume_serial == b.volume_serial && a.file_id == b.file_id)
        || (same_server && a.file_id[..4] == b.file_id[..4] && a.file_id[..4] != [0; 4])
}

/// The identity of the file at `path`, or `None` when it cannot be opened or the file system
/// does not report one.
#[must_use]
pub fn identity(path: &Path) -> Option<FileIdentity> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path; only the attributes are asked for, every
    // sharing mode is allowed, and the handle is closed below.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    // SAFETY: FILE_ID_INFO is a plain C struct; all zeroes is a valid value to be overwritten.
    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    let size = u32::try_from(size_of::<FILE_ID_INFO>()).ok()?;
    // SAFETY: `handle` is open; `info` is a writable FILE_ID_INFO of `size` bytes.
    let ok = unsafe {
        GetFileInformationByHandleEx(handle, FileIdInfo, (&raw mut info).cast(), size) != 0
    };
    // SAFETY: `handle` was opened above and is closed once.
    unsafe { CloseHandle(handle) };
    let id = FileIdentity {
        volume_serial: info.VolumeSerialNumber,
        file_id: info.FileId.Identifier,
    };
    (ok && id.file_id != [0; 16]).then_some(id)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn hard_links_are_one_file_and_copies_are_two() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        std::fs::write(&a, b"same").unwrap();
        std::fs::hard_link(&a, dir.path().join("link.bin")).unwrap();
        std::fs::copy(&a, dir.path().join("copy.bin")).unwrap();
        let id = identity(&a).unwrap();
        assert_eq!(identity(&dir.path().join("link.bin")), Some(id));
        assert_ne!(identity(&dir.path().join("copy.bin")), Some(id));
        assert_eq!(identity(&dir.path().join("missing.bin")), None);
        let copy = identity(&dir.path().join("copy.bin")).unwrap();
        assert!(same_file(&id, &id, false));
        assert!(!same_file(&id, &copy, false));
    }

    #[test]
    fn two_shares_of_a_nas_can_show_one_file() {
        let id = |serial: u64, n: u64| FileIdentity {
            volume_serial: serial,
            file_id: u128::from(n).to_le_bytes(),
        };
        // One photo seen as \\nas\Big-Storage\… and \\nas\ZimaOS-HD\.media\Big-Storage\….
        let direct = id(0x1111, 0x0000_0000_0008_5cc2);
        let media = id(0x2222, 0x0000_0040_0008_5cc2);
        assert!(same_file(&direct, &media, true));
        // Not on the same server: other files.
        assert!(!same_file(&direct, &media, false));
        assert!(!same_file(
            &direct,
            &id(0x2222, 0x0000_0040_0008_5cc3),
            true
        ));
    }
}
