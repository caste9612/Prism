//! Names and addresses of the servers of network shares (REVIEW D59): a NAS reached by its IP
//! address can get another address from the router; its name finds it again.

use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::sync::OnceLock;

use windows_sys::Win32::Networking::WinSock::{
    AF_INET, GetNameInfoW, IN_ADDR, IN_ADDR_0, NI_MAXHOST, NI_NAMEREQD, SOCKADDR, SOCKADDR_IN,
    WSADATA, WSAStartup,
};

/// Starts Winsock once for the process (Windows keeps the count; it is never stopped).
fn winsock() -> bool {
    static STARTED: OnceLock<bool> = OnceLock::new();
    *STARTED.get_or_init(|| {
        // SAFETY: WSADATA is a plain C struct; all zeroes is a valid value to be overwritten.
        let mut data: WSADATA = unsafe { std::mem::zeroed() };
        // SAFETY: `data` is a valid, writable WSADATA for the duration of the call.
        unsafe { WSAStartup(0x0202, &raw mut data) == 0 }
    })
}

/// The name of the host at `address`, as Windows finds it (DNS, mDNS, LLMNR, NetBIOS); `None`
/// when it has none. May take a few seconds: call it off the interface thread.
#[must_use]
pub fn host_name(address: Ipv4Addr) -> Option<String> {
    if !winsock() {
        return None;
    }
    let sa = SOCKADDR_IN {
        sin_family: AF_INET,
        sin_port: 0,
        sin_addr: IN_ADDR {
            // Network byte order: the octets as they are in memory.
            S_un: IN_ADDR_0 {
                S_addr: u32::from_ne_bytes(address.octets()),
            },
        },
        sin_zero: [0; 8],
    };
    let mut host = vec![0u16; NI_MAXHOST as usize];
    let size = i32::try_from(size_of::<SOCKADDR_IN>()).ok()?;
    // SAFETY: `sa` is a valid SOCKADDR_IN of `size` bytes; `host` is a writable buffer of
    // NI_MAXHOST UTF-16 units; no service buffer is asked for.
    let rc = unsafe {
        GetNameInfoW(
            (&raw const sa).cast::<SOCKADDR>(),
            size,
            host.as_mut_ptr(),
            NI_MAXHOST,
            std::ptr::null_mut(),
            0,
            NI_NAMEREQD as i32,
        )
    };
    if rc != 0 {
        return None;
    }
    let len = host.iter().position(|&c| c == 0).unwrap_or(host.len());
    let name = String::from_utf16_lossy(&host[..len]);
    // Without a name, some resolvers answer with the address itself.
    (!name.is_empty() && name.parse::<IpAddr>().is_err()).then_some(name)
}

/// The IPv4 addresses of the host called `name` (mDNS names like `nas.local` included).
#[must_use]
pub fn ipv4_addresses(name: &str) -> Vec<Ipv4Addr> {
    let mut found: Vec<Ipv4Addr> = (name, 445)
        .to_socket_addrs()
        .map(|it| {
            it.filter_map(|a| match a.ip() {
                IpAddr::V4(v4) => Some(v4),
                IpAddr::V6(_) => None,
            })
            .collect()
        })
        .unwrap_or_default();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_local_host_has_a_name_and_an_address() {
        assert!(ipv4_addresses("localhost").contains(&Ipv4Addr::LOCALHOST));
        assert!(ipv4_addresses("no-such-host.invalid").is_empty());
        // The loopback address has a name on every Windows machine.
        assert!(host_name(Ipv4Addr::LOCALHOST).is_some());
    }
}
