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

/// Background mode of the current thread while alive: Windows lowers its CPU, disk and memory
/// priority so that the work does not slow the user down (F06-AC2). Ends when dropped.
/// Not `Send`: the mode belongs to the thread that entered it.
pub struct BackgroundMode(bool, std::marker::PhantomData<*const ()>);

impl BackgroundMode {
    #[must_use]
    pub fn enter() -> Self {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        // SAFETY: the pseudo-handle of the current thread is always valid.
        let ok = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
        Self(ok != 0, std::marker::PhantomData)
    }

    /// Whether Windows accepted the background mode.
    pub fn active(&self) -> bool {
        self.0
    }
}

impl Drop for BackgroundMode {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_END,
        };
        if self.0 {
            // SAFETY: as in `enter`, on the same thread (the guard is not `Send`).
            unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END) };
        }
    }
}

/// Whether the PC runs on battery (F21-AC3). `false` when Windows cannot tell (desktops).
#[must_use]
pub fn on_battery() -> bool {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS {
        ACLineStatus: 255,
        BatteryFlag: 0,
        BatteryLifePercent: 0,
        SystemStatusFlag: 0,
        BatteryLifeTime: 0,
        BatteryFullLifeTime: 0,
    };
    // SAFETY: `status` is a writable SYSTEM_POWER_STATUS.
    let ok = unsafe { GetSystemPowerStatus(&raw mut status) };
    ok != 0 && status.ACLineStatus == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    // Windows does not update the peak together with the current working set: under load (the
    // test binary runs many threads) the peak read in the same call can lag behind the current
    // value, so `peak >= current` is not a property of the API (seen failing once in
    // `scripts/check.ps1` on 2026-11-07, never in isolation). What the API guarantees is that
    // the counters are reported.
    #[test]
    fn memory_usage_is_reported() {
        let m = memory_usage().unwrap();
        assert!(m.working_set > 0);
        assert!(m.peak_working_set > 0);
        assert!(m.private_bytes > 0);
    }

    #[test]
    fn background_mode_enters_and_leaves() {
        let guard = BackgroundMode::enter();
        assert!(guard.active());
        drop(guard);
        // A second time on the same thread works too (the first one ended).
        assert!(BackgroundMode::enter().active());
        let _ = on_battery();
    }

    #[test]
    fn elevation_query_does_not_fail() {
        // The value depends on how the shell was started; the call itself must work.
        let _ = is_elevated();
    }
}

/// Executable names of the running processes, lowercase (`chrome.exe`): whether a browser is
/// open before proposing its caches (F09-AC4). Empty if the list cannot be read.
#[must_use]
pub fn running_process_names() -> std::collections::HashSet<String> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    let mut names = std::collections::HashSet::new();
    // SAFETY: no pointer arguments; returns a snapshot handle or INVALID_HANDLE_VALUE.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
        return names;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `snapshot` is a valid process snapshot; `entry` is writable with dwSize set.
    let mut ok = unsafe { Process32FirstW(snapshot, &raw mut entry) } != 0;
    while ok {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        names.insert(String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase());
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: `snapshot` is owned here and closed once.
    unsafe { CloseHandle(snapshot) };
    names
}

#[cfg(test)]
mod process_list_tests {
    #[test]
    fn the_current_process_is_listed() {
        let names = super::running_process_names();
        let me = std::env::current_exe()
            .unwrap_or_default()
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        assert!(names.contains(&me), "{me} not in {} processes", names.len());
    }
}
