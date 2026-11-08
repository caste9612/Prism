//! The named pipe through which the Explorer menu (`prism-shell`, not elevated) talks to the
//! running Prism (elevated): one text request, one text answer (F18-AC2).
//!
//! An elevated process creates the pipe, so its default access would exclude the same user's
//! non-elevated processes: the pipe gets an explicit access list (SYSTEM, administrators and
//! the current user) and a medium integrity label, so `prism-shell` can write to it. Remote
//! clients are refused.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, GetLastError,
    HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    SetNamedPipeHandleState, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::volumes::wide;

/// Requests and answers are short lines.
const BUFFER: usize = 8192;

#[derive(Debug, thiserror::Error)]
pub enum PipeError {
    #[error("{0} failed ({1})")]
    Win32(&'static str, u32),
    #[error("no answer within the time limit")]
    Timeout,
}

fn last(what: &'static str) -> PipeError {
    // SAFETY: trivially safe, reads the calling thread's last error.
    PipeError::Win32(what, unsafe { GetLastError() })
}

/// The pipe of the current user: `\\.\pipe\prism-<user>` (`prism-dev-` for development builds,
/// so that a development and an installed Prism do not answer for each other).
#[must_use]
pub fn name(dev: bool) -> String {
    let user: String = std::env::var("USERNAME")
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    format!(r"\\.\pipe\prism-{}{user}", if dev { "dev-" } else { "" })
}

/// The SID of the current process user, as text (`S-1-5-21-...`).
fn user_sid() -> Option<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-handle of the current process is valid; `token` receives a handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return None;
    }
    let mut buf = vec![0u64; 64];
    let mut returned = 0u32;
    let size = u32::try_from(buf.len() * 8).unwrap_or(0);
    // SAFETY: `token` was opened with TOKEN_QUERY; `buf` is writable for `size` bytes and
    // 8-byte aligned, as TOKEN_USER needs.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr().cast(),
            size,
            &raw mut returned,
        )
    };
    // SAFETY: `token` is owned here and closed once.
    unsafe { CloseHandle(token) };
    if ok == 0 {
        return None;
    }
    // SAFETY: on success the buffer starts with a TOKEN_USER whose SID points inside it.
    let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: `sid` is valid while `buf` lives; `text` receives a LocalAlloc'ed string.
    if unsafe { ConvertSidToStringSidW(sid, &raw mut text) } == 0 {
        return None;
    }
    // SAFETY: `text` is a NUL-terminated string allocated by the call above.
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    // SAFETY: `text` holds `len` valid UTF-16 units.
    let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    // SAFETY: `text` was allocated with LocalAlloc by ConvertSidToStringSidW.
    unsafe { LocalFree(text.cast::<core::ffi::c_void>() as HLOCAL) };
    Some(s)
}

/// SYSTEM and administrators: everything; the current user: read and write; integrity label
/// medium with "no write up", so the user's non-elevated processes can write.
fn sddl() -> String {
    let user = user_sid()
        .map(|s| format!("(A;;GRGW;;;{s})"))
        .unwrap_or_default();
    format!("D:P(A;;GA;;;SY)(A;;GA;;;BA){user}S:(ML;;NW;;;ME)")
}

struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle is owned and closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

fn read_message(h: HANDLE) -> Option<String> {
    let mut buf = vec![0u8; BUFFER];
    let mut read = 0u32;
    // SAFETY: `h` is a connected pipe; `buf` is writable for its length; synchronous call.
    let ok = unsafe {
        ReadFile(
            h,
            buf.as_mut_ptr(),
            u32::try_from(buf.len()).unwrap_or(0),
            &raw mut read,
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then(|| String::from_utf8_lossy(&buf[..read as usize]).into_owned())
}

fn write_message(h: HANDLE, text: &str) -> bool {
    let mut written = 0u32;
    // SAFETY: `h` is a connected pipe; `text` is readable for its length; synchronous call.
    unsafe {
        WriteFile(
            h,
            text.as_ptr(),
            u32::try_from(text.len()).unwrap_or(0),
            &raw mut written,
            std::ptr::null_mut(),
        ) != 0
    }
}

/// Answers requests on pipe `name` forever, one client at a time: `handle` turns a request into
/// an answer. Returns only if the pipe cannot be created.
///
/// # Errors
/// [`PipeError`] when the security descriptor or the pipe cannot be created.
pub fn serve(name: &str, handle: impl Fn(&str) -> String) -> Result<(), PipeError> {
    let sddl = wide(&sddl());
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated; `sd` receives a LocalAlloc'ed descriptor, kept for the
    // life of the server (this function never returns while serving).
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &raw mut sd,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(last("ConvertStringSecurityDescriptorToSecurityDescriptorW"));
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    let wname = wide(name);
    loop {
        // SAFETY: `wname` is NUL-terminated; `attributes` points to a valid descriptor.
        let pipe = unsafe {
            CreateNamedPipeW(
                wname.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER as u32,
                BUFFER as u32,
                0,
                &raw const attributes,
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            let e = last("CreateNamedPipeW");
            // SAFETY: `sd` was allocated by the conversion above and is freed once.
            unsafe { LocalFree(sd as HLOCAL) };
            return Err(e);
        }
        let pipe = Owned(pipe);
        // SAFETY: `pipe` is a valid pipe handle; synchronous wait for a client.
        let connected = unsafe { ConnectNamedPipe(pipe.0, std::ptr::null_mut()) } != 0
            // SAFETY: trivially safe.
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if connected {
            if let Some(request) = read_message(pipe.0) {
                let answer = handle(request.trim());
                write_message(pipe.0, &answer);
                // SAFETY: valid connected pipe.
                unsafe { FlushFileBuffers(pipe.0) };
            }
            // SAFETY: valid pipe handle.
            unsafe { DisconnectNamedPipe(pipe.0) };
        }
    }
}

/// Sends `request` to the server of pipe `name` and returns its answer, waiting up to
/// `timeout` for the pipe to exist and be free.
///
/// # Errors
/// [`PipeError::Timeout`] when no server answers in time; [`PipeError::Win32`] on errors.
pub fn send(name: &str, request: &str, timeout: Duration) -> Result<String, PipeError> {
    let wname = wide(name);
    let start = Instant::now();
    let pipe = loop {
        // SAFETY: `wname` is NUL-terminated; plain open of an existing pipe.
        let h = unsafe {
            CreateFileW(
                wname.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE {
            break Owned(h);
        }
        if start.elapsed() >= timeout {
            return Err(PipeError::Timeout);
        }
        // SAFETY: trivially safe.
        if unsafe { GetLastError() } == ERROR_PIPE_BUSY {
            // SAFETY: `wname` is NUL-terminated.
            unsafe { WaitNamedPipeW(wname.as_ptr(), 1000) };
        } else {
            // Not created yet (Prism is starting): try again shortly.
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    let mode = PIPE_READMODE_MESSAGE;
    // SAFETY: valid pipe handle; `mode` outlives the call; the other parameters are optional.
    if unsafe {
        SetNamedPipeHandleState(pipe.0, &raw const mode, std::ptr::null(), std::ptr::null())
    } == 0
    {
        return Err(last("SetNamedPipeHandleState"));
    }
    if !write_message(pipe.0, request) {
        return Err(last("WriteFile"));
    }
    read_message(pipe.0).ok_or_else(|| last("ReadFile"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_answer() {
        let name = format!(r"\\.\pipe\prism-test-{}", std::process::id());
        let server = name.clone();
        std::thread::spawn(move || {
            let _ = serve(&server, |req| format!("ok {req}"));
        });
        let answer = send(&name, "analyze\tD:\\Foto", Duration::from_secs(5)).unwrap();
        assert_eq!(answer, "ok analyze\tD:\\Foto");
        // A second client after the first.
        assert_eq!(
            send(&name, "ping", Duration::from_secs(5)).unwrap(),
            "ok ping"
        );
        // Nobody listening: a timeout, not a hang.
        let started = Instant::now();
        assert!(matches!(
            send(
                r"\\.\pipe\prism-nobody-here",
                "x",
                Duration::from_millis(500)
            ),
            Err(PipeError::Timeout)
        ));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(sddl().starts_with("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;S-1-"));
        assert!(name.len() > 10 && super::name(true).contains("prism-dev-"));
    }
}
