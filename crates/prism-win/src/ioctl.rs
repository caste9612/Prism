//! `DeviceIoControl` on an open file or volume handle, with Windows error codes.

use std::ffi::c_void;
use std::fs::File;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::IO::DeviceIoControl;

pub(crate) use windows_sys::Win32::System::Ioctl::{
    FSCTL_GET_NTFS_FILE_RECORD, FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL,
};

/// Sends `code` with `input` and returns the number of bytes written into `out` (8-byte
/// aligned storage, as many output structures require), or the Windows error code.
pub(crate) fn device_io(
    file: &File,
    code: u32,
    input: &[u8],
    out: &mut [u64],
) -> Result<usize, u32> {
    let mut returned = 0u32;
    let out_len = u32::try_from(size_of_val(out)).map_err(|_| 87u32)?; // ERROR_INVALID_PARAMETER
    let in_len = u32::try_from(input.len()).map_err(|_| 87u32)?;
    let input_ptr = if input.is_empty() {
        std::ptr::null()
    } else {
        input.as_ptr().cast::<c_void>()
    };
    // SAFETY: the handle belongs to `file`, which outlives the call; `input` is readable for
    // `in_len` bytes (or null with length 0); `out` is writable for `out_len` bytes; the call
    // is synchronous (no OVERLAPPED, the handle was not opened for overlapped I/O).
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            code,
            input_ptr,
            in_len,
            out.as_mut_ptr().cast::<c_void>(),
            out_len,
            &raw mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        // SAFETY: reads the calling thread's last error, set by the failed call above.
        return Err(unsafe { GetLastError() });
    }
    Ok(returned as usize)
}

/// The first `n` bytes of an aligned output buffer.
pub(crate) fn as_bytes(out: &[u64], n: usize) -> &[u8] {
    // SAFETY: any initialized u64 storage can be viewed as bytes; the length is clamped to
    // the size of the buffer.
    let all = unsafe { std::slice::from_raw_parts(out.as_ptr().cast::<u8>(), size_of_val(out)) };
    &all[..n.min(all.len())]
}
