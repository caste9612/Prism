//! Detection of the connected volumes and of their identity keys (ADR-0004, ADR-0016).
//!
//! Local volumes come from `FindFirstVolumeW`/`FindNextVolumeW`; for each one:
//! mount points, drive type, label, file system, capacity, the file-system serial (64 bits on
//! NTFS/ReFS from `FILE_ID_INFO`, 32 bits elsewhere), the bus of the disk and the physical disk
//! numbers. None of this needs administrator rights. Volumes without a mount point (recovery
//! and EFI partitions) are left out: there is no path to read them through.
//!
//! **Network drives.** An elevated process — Prism always is (ADR-0003) — does not see the
//! drives mapped by the user's non-elevated session (verified on 2026-11-07: `net use` is
//! empty in an elevated shell), but it can reach the same shares through their UNC path. So
//! the mapped drives are read from two sources and merged: the persistent mappings of the
//! user (`HKCU\Network\<letter>\RemotePath`) and the mappings of the current session
//! (`WNetGetConnectionW`). Each share is identified by its UNC path, which is also the path
//! used to read it; the letter is kept for display. Only local calls are made: asking a
//! server for its label or free space can block for tens of seconds when it is offline.

use std::ffi::c_void;

use prism_core::model::{BusType, VolumeInfo, VolumeKind};
use prism_core::volumes::{DetectedVolume, VolumeError, VolumeSource};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_MORE_DATA, GetLastError, HANDLE, INVALID_HANDLE_VALUE, NO_ERROR,
};
use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;
use windows_sys::Win32::Storage::FileSystem::{
    self as fs, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo, FindFirstVolumeW,
    FindNextVolumeW, FindVolumeClose, GetDiskFreeSpaceExW, GetDriveTypeW,
    GetFileInformationByHandleEx, GetLogicalDrives, GetVolumeInformationW,
    GetVolumePathNamesForVolumeNameW, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{
    DISK_EXTENT, IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR,
    STORAGE_PROPERTY_QUERY, StorageDeviceProperty, VOLUME_DISK_EXTENTS,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;

/// The Windows [`VolumeSource`].
#[derive(Clone, Copy, Debug, Default)]
pub struct WinVolumeSource;

impl VolumeSource for WinVolumeSource {
    fn detect(&self) -> Result<Vec<DetectedVolume>, VolumeError> {
        let mut out: Vec<DetectedVolume> = volume_guid_paths()?
            .iter()
            .filter_map(|g| local_volume(g))
            .collect();
        out.extend(network_drives());
        Ok(out)
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn len_u32(buf: &[u16]) -> u32 {
    u32::try_from(buf.len()).unwrap_or(u32::MAX)
}

/// An owned handle, closed on drop.
struct Handle(HANDLE);

impl Handle {
    /// Opens `path` with the given access; `None` on failure.
    fn open(path: &str, access: u32, flags: u32) -> Option<Self> {
        let w = wide(path);
        // SAFETY: `w` is a NUL-terminated UTF-16 string that outlives the call; the security
        // attributes and template handle may be null.
        let h = unsafe {
            CreateFileW(
                w.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                flags,
                std::ptr::null_mut(),
            )
        };
        (h != INVALID_HANDLE_VALUE && !h.is_null()).then_some(Self(h))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by CreateFileW and is closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

fn volume_guid_paths() -> Result<Vec<String>, VolumeError> {
    let mut buf = [0u16; 64];
    // SAFETY: `buf` is writable and its length is passed.
    let find = unsafe { FindFirstVolumeW(buf.as_mut_ptr(), len_u32(&buf)) };
    if find == INVALID_HANDLE_VALUE {
        // SAFETY: trivially safe, reads the calling thread's last error.
        let code = unsafe { GetLastError() };
        return Err(VolumeError::Platform(format!(
            "FindFirstVolumeW failed ({code})"
        )));
    }
    let mut out = vec![from_wide(&buf)];
    loop {
        // SAFETY: `find` is a valid search handle; `buf` is writable with the given length.
        if unsafe { FindNextVolumeW(find, buf.as_mut_ptr(), len_u32(&buf)) } == 0 {
            break;
        }
        out.push(from_wide(&buf));
    }
    // SAFETY: `find` is a valid search handle, closed once.
    unsafe { FindVolumeClose(find) };
    Ok(out)
}

/// Mount points (`C:\`, `D:\Mounts\Disk\`) of a volume GUID path, letters first.
fn mount_points(guid_path: &str) -> Vec<String> {
    let w = wide(guid_path);
    let mut buf = vec![0u16; 256];
    loop {
        let mut needed = 0u32;
        // SAFETY: `w` is NUL-terminated; `buf` is writable with the given length; `needed`
        // receives the required length.
        let ok = unsafe {
            GetVolumePathNamesForVolumeNameW(
                w.as_ptr(),
                buf.as_mut_ptr(),
                len_u32(&buf),
                &raw mut needed,
            )
        };
        if ok != 0 {
            break;
        }
        // SAFETY: reads the calling thread's last error.
        if unsafe { GetLastError() } == ERROR_MORE_DATA && needed as usize > buf.len() {
            buf = vec![0u16; needed as usize];
            continue;
        }
        return Vec::new();
    }
    let mut mounts: Vec<String> = buf
        .split(|&c| c == 0)
        .take_while(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect();
    mounts.sort_by_key(|m| (m.len() != 3, m.clone()));
    mounts
}

fn local_volume(guid_path: &str) -> Option<DetectedVolume> {
    let mounts = mount_points(guid_path);
    if mounts.is_empty() {
        return None; // recovery/EFI partitions and other volumes without a path
    }
    let w = wide(guid_path);
    // SAFETY: `w` is a NUL-terminated root path.
    let kind = match unsafe { GetDriveTypeW(w.as_ptr()) } {
        DRIVE_FIXED => VolumeKind::Fixed,
        DRIVE_REMOVABLE => VolumeKind::Removable,
        _ => return None, // optical drives, RAM disks, unknown
    };
    let mut label = [0u16; 261];
    let mut fs_name = [0u16; 261];
    let mut serial32 = 0u32;
    let mut max_component = 0u32;
    let mut flags = 0u32;
    // SAFETY: every output buffer is writable with the length passed; `w` is NUL-terminated.
    let ok = unsafe {
        GetVolumeInformationW(
            w.as_ptr(),
            label.as_mut_ptr(),
            len_u32(&label),
            &raw mut serial32,
            &raw mut max_component,
            &raw mut flags,
            fs_name.as_mut_ptr(),
            len_u32(&fs_name),
        )
    };
    if ok == 0 {
        return None; // unformatted, locked or empty reader
    }
    let fs_type = from_wide(&fs_name);
    let (mut total, mut free, mut avail) = (0u64, 0u64, 0u64);
    // SAFETY: `w` is NUL-terminated; the three outputs are writable u64.
    let has_space =
        unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &raw mut avail, &raw mut total, &raw mut free) }
            != 0;
    let serial = if fs_type.eq_ignore_ascii_case("NTFS") || fs_type.eq_ignore_ascii_case("ReFS") {
        serial64(guid_path).unwrap_or(u64::from(serial32))
    } else {
        u64::from(serial32)
    };
    let device = guid_path.trim_end_matches('\\');
    let label = from_wide(&label);
    Some(DetectedVolume {
        info: VolumeInfo {
            kind,
            fs_type: Some(fs_type),
            fs_serial: Some(serial),
            volume_guid: Some(guid_path.to_owned()),
            unc_path: None,
            os_label: (!label.is_empty()).then_some(label),
            capacity: has_space.then_some(total),
            free_space: has_space.then_some(free),
            bus: bus_type(device),
            last_mount: mounts.first().cloned(),
        },
        disks: disk_numbers(device),
        mounts,
    })
}

/// The 64-bit serial of an NTFS/ReFS volume, from the root folder (no administrator needed).
fn serial64(guid_path: &str) -> Option<u64> {
    let root = Handle::open(guid_path, FILE_READ_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS)?;
    let mut info = FILE_ID_INFO::default();
    let size = u32::try_from(size_of::<FILE_ID_INFO>()).ok()?;
    // SAFETY: `root` is a valid handle; `info` is a writable FILE_ID_INFO of `size` bytes, the
    // structure required by the FileIdInfo class.
    let ok =
        unsafe { GetFileInformationByHandleEx(root.0, FileIdInfo, (&raw mut info).cast(), size) };
    (ok != 0).then_some(info.VolumeSerialNumber)
}

/// Sends an IOCTL without input buffer issues; returns the bytes written into `out`.
fn ioctl(device: &Handle, code: u32, input: &[u8], out: &mut [u64]) -> Option<usize> {
    let mut returned = 0u32;
    let out_len = u32::try_from(size_of_val(out)).ok()?;
    let in_len = u32::try_from(input.len()).ok()?;
    // SAFETY: `device` is a valid handle; `input` is readable for `in_len` bytes; `out` is
    // writable for `out_len` bytes and 8-byte aligned (u64 storage); the call is synchronous
    // (no OVERLAPPED).
    let ok = unsafe {
        DeviceIoControl(
            device.0,
            code,
            input.as_ptr().cast::<c_void>(),
            in_len,
            out.as_mut_ptr().cast::<c_void>(),
            out_len,
            &raw mut returned,
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(returned as usize)
}

fn bus_type(device: &str) -> Option<BusType> {
    let h = Handle::open(device, 0, 0)?;
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    // SAFETY: STORAGE_PROPERTY_QUERY is a plain C struct; viewing it as bytes for the duration
    // of the call is sound.
    let input = unsafe {
        std::slice::from_raw_parts(
            (&raw const query).cast::<u8>(),
            size_of::<STORAGE_PROPERTY_QUERY>(),
        )
    };
    let mut out = [0u64; 128];
    let n = ioctl(&h, IOCTL_STORAGE_QUERY_PROPERTY, input, &mut out)?;
    if n < std::mem::offset_of!(STORAGE_DEVICE_DESCRIPTOR, RawPropertiesLength) {
        return None;
    }
    // SAFETY: the driver wrote at least the fixed part of a STORAGE_DEVICE_DESCRIPTOR at the
    // start of `out`, which is 8-byte aligned.
    let desc = unsafe { std::ptr::read(out.as_ptr().cast::<STORAGE_DEVICE_DESCRIPTOR>()) };
    Some(match desc.BusType {
        fs::BusTypeUsb => BusType::Usb,
        fs::BusTypeSata => BusType::Sata,
        fs::BusTypeNvme => BusType::Nvme,
        fs::BusTypeAta | fs::BusTypeAtapi => BusType::Ata,
        fs::BusTypeScsi => BusType::Scsi,
        fs::BusTypeSas => BusType::Sas,
        fs::BusTypeRAID => BusType::Raid,
        fs::BusTypeSd => BusType::Sd,
        fs::BusTypeMmc => BusType::Mmc,
        fs::BusType1394 => BusType::Ieee1394,
        fs::BusTypeiScsi => BusType::Iscsi,
        fs::BusTypeSpaces => BusType::Spaces,
        fs::BusTypeVirtual | fs::BusTypeFileBackedVirtual => BusType::Virtual,
        _ => BusType::Other,
    })
}

fn disk_numbers(device: &str) -> Vec<u32> {
    let Some(h) = Handle::open(device, 0, 0) else {
        return Vec::new();
    };
    // Room for 32 extents (a volume spans several disks only with dynamic disks or Spaces).
    let mut out = [0u64; 1 + 32 * 3];
    let Some(n) = ioctl(&h, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, &[], &mut out) else {
        return Vec::new();
    };
    let base = std::mem::offset_of!(VOLUME_DISK_EXTENTS, Extents);
    if n < base {
        return Vec::new();
    }
    // SAFETY: the driver wrote a VOLUME_DISK_EXTENTS header at the start of `out` (aligned).
    let count = unsafe { std::ptr::read(out.as_ptr().cast::<u32>()) } as usize;
    let fits = (n - base) / size_of::<DISK_EXTENT>();
    let mut disks: Vec<u32> = (0..count.min(fits))
        .map(|i| {
            // SAFETY: extent `i` lies within the `n` bytes written by the driver
            // (i < fits); the read is unaligned-safe.
            unsafe {
                std::ptr::read_unaligned(
                    out.as_ptr()
                        .cast::<u8>()
                        .add(base + i * size_of::<DISK_EXTENT>())
                        .cast::<DISK_EXTENT>(),
                )
            }
            .DiskNumber
        })
        .collect();
    disks.sort_unstable();
    disks.dedup();
    disks
}

/// Drive letters mapped in the current session, with their UNC path.
fn session_mappings() -> Vec<(char, String)> {
    // SAFETY: no arguments; returns a bit mask of the existing drive letters.
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for (i, letter) in ('A'..='Z').enumerate() {
        if mask & (1 << i) == 0 {
            continue;
        }
        let w = wide(&format!("{letter}:\\"));
        // SAFETY: `w` is a NUL-terminated root path.
        if unsafe { GetDriveTypeW(w.as_ptr()) } != DRIVE_REMOTE {
            continue;
        }
        let local = wide(&format!("{letter}:"));
        let mut buf = vec![0u16; 1024];
        let mut len = len_u32(&buf);
        // SAFETY: `local` is NUL-terminated; `buf` is writable with `len` characters.
        let rc = unsafe { WNetGetConnectionW(local.as_ptr(), buf.as_mut_ptr(), &raw mut len) };
        if rc == NO_ERROR {
            out.push((letter, from_wide(&buf)));
        }
    }
    out
}

/// Persistent drive mappings of the current user (`HKCU\Network\<letter>\RemotePath`).
fn persistent_mappings() -> Vec<(char, String)> {
    let key = wide("Network");
    let mut hkey: HKEY = std::ptr::null_mut();
    // SAFETY: HKEY_CURRENT_USER is a predefined key; `key` is NUL-terminated; `hkey` receives
    // the opened key.
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &raw mut hkey) }
        != NO_ERROR
    {
        return Vec::new();
    }
    let value = wide("RemotePath");
    let mut out = Vec::new();
    for index in 0u32.. {
        let mut name = [0u16; 256];
        let mut name_len = len_u32(&name);
        // SAFETY: `hkey` is open; `name` is writable with `name_len` characters; the optional
        // class and time outputs are null.
        let rc = unsafe {
            RegEnumKeyExW(
                hkey,
                index,
                name.as_mut_ptr(),
                &raw mut name_len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc != NO_ERROR {
            break; // ERROR_NO_MORE_ITEMS
        }
        let sub = from_wide(&name);
        let mut chars = sub.chars();
        let (Some(letter), None) = (chars.next(), chars.next()) else {
            continue;
        };
        let mut data = [0u16; 1024];
        let mut size = u32::try_from(size_of_val(&data)).unwrap_or(0);
        // SAFETY: `hkey` is open; `name` (the subkey) and `value` are NUL-terminated; `data`
        // is writable for `size` bytes; the type output is not needed.
        let rc = unsafe {
            RegGetValueW(
                hkey,
                name.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        if rc == NO_ERROR {
            out.push((letter.to_ascii_uppercase(), from_wide(&data)));
        }
    }
    // SAFETY: `hkey` was opened above and is closed once.
    unsafe { RegCloseKey(hkey) };
    out
}

fn network_drives() -> Vec<DetectedVolume> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (letter, unc) in session_mappings().into_iter().chain(persistent_mappings()) {
        let key = prism_core::volumes::normalize_unc(&unc);
        if unc.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let root = format!("{letter}:\\");
        out.push(DetectedVolume {
            info: VolumeInfo {
                kind: VolumeKind::Network,
                fs_type: None,
                fs_serial: None,
                volume_guid: None,
                unc_path: Some(unc.clone()),
                os_label: None,
                capacity: None,
                free_space: None,
                bus: None,
                last_mount: Some(root.clone()),
            },
            mounts: vec![root, unc],
            disks: Vec::new(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::volumes::{VolumeMatch, match_volumes};

    fn system_volume(volumes: &[DetectedVolume]) -> &DetectedVolume {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let root = format!("{}\\", system.trim_end_matches('\\'));
        volumes
            .iter()
            .find(|v| v.mounts.iter().any(|m| m.eq_ignore_ascii_case(&root)))
            .unwrap_or_else(|| panic!("system volume {root} not detected: {volumes:#?}"))
    }

    #[test]
    fn detects_the_system_volume_with_its_identity() {
        let volumes = WinVolumeSource.detect().unwrap();
        let v = system_volume(&volumes);
        assert_eq!(v.info.kind, VolumeKind::Fixed);
        assert_eq!(v.info.fs_type.as_deref(), Some("NTFS"));
        assert!(
            v.info
                .volume_guid
                .as_deref()
                .unwrap()
                .starts_with(r"\\?\Volume{")
        );
        let capacity = v.info.capacity.unwrap();
        assert!(capacity > 0 && v.info.free_space.unwrap() <= capacity);
        assert!(!v.disks.is_empty(), "physical disk numbers");
        assert!(v.info.bus.is_some(), "bus type");
        assert!(v.info.last_mount.as_deref().unwrap().len() == 3);
    }

    // On NTFS the 32-bit serial shown by Windows is the low half of the 64-bit serial.
    #[test]
    fn ntfs_serial_is_the_64_bit_one() {
        let volumes = WinVolumeSource.detect().unwrap();
        let v = system_volume(&volumes);
        let root = wide(v.info.last_mount.as_deref().unwrap());
        let mut serial32 = 0u32;
        // SAFETY: `root` is NUL-terminated; only the serial output is requested.
        let ok = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                std::ptr::null_mut(),
                0,
                &raw mut serial32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        assert_ne!(ok, 0);
        let serial = v.info.fs_serial.unwrap();
        assert_eq!(serial & 0xFFFF_FFFF, u64::from(serial32));
        assert!(
            serial > u64::from(u32::MAX),
            "expected a 64-bit NTFS serial"
        );
    }

    // Detection is stable and every connected volume matches itself (and only itself).
    #[test]
    fn detection_is_stable_and_matches_itself() {
        use prism_core::model::{Volume, VolumeId, VolumeLabels, VolumeStatus};
        let first = WinVolumeSource.detect().unwrap();
        let second = WinVolumeSource.detect().unwrap();
        let keys = |v: &[DetectedVolume]| -> Vec<_> {
            v.iter()
                .map(|d| {
                    (
                        d.info.volume_guid.clone(),
                        d.info.fs_serial,
                        d.info.unc_path.clone(),
                    )
                })
                .collect()
        };
        assert_eq!(keys(&first), keys(&second));
        let catalog: Vec<Volume> = first
            .iter()
            .enumerate()
            .map(|(i, d)| Volume {
                id: VolumeId(i as i64 + 1),
                info: d.info.clone(),
                labels: VolumeLabels::default(),
                status: VolumeStatus::Indexed,
                first_seen_at: 0,
                last_seen_at: None,
                last_indexed_at: None,
                usn: None,
            })
            .collect();
        let matches = match_volumes(&second, &catalog);
        for (i, m) in matches.iter().enumerate() {
            assert_eq!(
                *m,
                VolumeMatch::Known(VolumeId(i as i64 + 1)),
                "{:?}",
                second[i]
            );
        }
    }
}
