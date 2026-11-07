//! Information about the current process: memory usage (benchmarks, NF3) and elevation.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Memory counters of the current process, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryUsage {
    pub working_set: u64,
    pub peak_working_set: u64,
    /// Private committed memory.
    pub private_bytes: u64,
}

/// Memory usage of the current process, or `None` if Windows does not report it.
#[must_use]
pub fn memory_usage() -> Option<MemoryUsage> {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?,
        ..PROCESS_MEMORY_COUNTERS::default()
    };
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that is always valid and needs no
    // closing; `counters` is a properly sized, writable PROCESS_MEMORY_COUNTERS whose `cb`
    // field holds its size, as the API requires.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, counters.cb) };
    (ok != 0).then_some(MemoryUsage {
        working_set: counters.WorkingSetSize as u64,
        peak_working_set: counters.PeakWorkingSetSize as u64,
        private_bytes: counters.PagefileUsage as u64,
    })
}

/// Whether the current process runs with an elevated (administrator) token.
#[must_use]
pub fn is_elevated() -> bool {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-handle of the current process is valid; `token` is a writable HANDLE
    // that receives a new handle on success.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    let size = u32::try_from(size_of::<TOKEN_ELEVATION>()).unwrap_or(0);
    // SAFETY: `token` was opened with TOKEN_QUERY; the buffer is a TOKEN_ELEVATION of `size`
    // bytes and `returned` is writable.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            size,
            &raw mut returned,
        )
    };
    // SAFETY: `token` is a valid handle owned by this function and closed exactly once.
    unsafe { CloseHandle(token) };
    ok != 0 && elevation.TokenIsElevated != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_usage_is_reported() {
        let m = memory_usage().unwrap();
        assert!(m.working_set > 0);
        assert!(m.peak_working_set >= m.working_set);
    }

    #[test]
    fn elevation_query_does_not_fail() {
        // The value depends on how the shell was started; the call itself must work.
        let _ = is_elevated();
    }
}
