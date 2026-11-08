//! Names of the servers of network shares reached by IP address (REVIEW D59).
//!
//! A NAS often gets its address from the router, and after a restart the address can change:
//! the catalog still says `\\192.168.1.65\Photos` while the NAS answers at `.57`. So when a
//! share answers at an IP address, its server's name (`ZimaOS.local`) is learned and saved; when
//! the share stops answering, the name is looked up again and the same share is tried at the
//! new address. The user confirms before the catalog changes (the volume monitor asks).
//!
//! Lookups can take seconds: they run in threads of their own and the monitor only reads the
//! last answers, like the reachability probe (`netprobe.rs`).

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The name of the host at an address.
pub type ReverseFn = dyn Fn(Ipv4Addr) -> Option<String> + Send + Sync;
/// The IPv4 addresses of a host name.
pub type ForwardFn = dyn Fn(&str) -> Vec<Ipv4Addr> + Send + Sync;

/// How often the name of a server that answers is asked again.
const ASK_NAME_EVERY: Duration = Duration::from_secs(600);
/// How long the addresses of a name are trusted.
const ADDRESSES_FOR: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Inner {
    /// Address as written in the UNC paths → host name.
    names: HashMap<String, String>,
    /// Names learned since the last [`ServerNames::take_learned`], to be saved.
    learned: Vec<(String, String)>,
    /// When the name of an address was last asked (or is being asked).
    asked: HashMap<String, Instant>,
    /// Host name → its addresses, as last resolved.
    addresses: HashMap<String, Resolved>,
}

#[derive(Default)]
struct Resolved {
    at: Option<Instant>,
    list: Vec<Ipv4Addr>,
    resolving: bool,
}

pub struct ServerNames {
    reverse: Arc<ReverseFn>,
    forward: Arc<ForwardFn>,
    inner: Mutex<Inner>,
}

impl ServerNames {
    /// With the names saved in the catalog.
    pub fn new(
        reverse: Arc<ReverseFn>,
        forward: Arc<ForwardFn>,
        known: Vec<(String, String)>,
    ) -> Self {
        Self {
            reverse,
            forward,
            inner: Mutex::new(Inner {
                names: known.into_iter().collect(),
                ..Inner::default()
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A share answered at `server`: when it is an IPv4 address, learn its name (now and then).
    pub fn reached(self: &Arc<Self>, server: &str) {
        let Ok(ip) = server.parse::<Ipv4Addr>() else {
            return;
        };
        {
            let mut inner = self.lock();
            if inner
                .asked
                .get(server)
                .is_some_and(|at| at.elapsed() < ASK_NAME_EVERY)
            {
                return;
            }
            inner.asked.insert(server.to_owned(), Instant::now());
        }
        let me = Arc::clone(self);
        let server = server.to_owned();
        let _ = std::thread::Builder::new()
            .name("prism-net-names".into())
            .spawn(move || {
                let Some(name) = (me.reverse)(ip) else {
                    return;
                };
                let mut inner = me.lock();
                if inner.names.get(&server) != Some(&name) {
                    tracing::info!(%server, %name, "network server name learned");
                    inner.names.insert(server.clone(), name.clone());
                    inner.learned.push((server, name));
                }
            });
    }

    /// The other addresses where the server once at `server` answers to its name now, as far as
    /// is known; a new lookup starts in the background when the answer is old.
    pub fn moved(self: &Arc<Self>, server: &str) -> Vec<String> {
        let mut inner = self.lock();
        let Some(name) = inner.names.get(server).cloned() else {
            return Vec::new();
        };
        let entry = inner.addresses.entry(name.clone()).or_default();
        let last = entry.list.clone();
        let stale = !entry.resolving && entry.at.is_none_or(|at| at.elapsed() >= ADDRESSES_FOR);
        if stale {
            entry.resolving = true;
            drop(inner);
            let me = Arc::clone(self);
            let spawned = std::thread::Builder::new()
                .name("prism-net-names".into())
                .spawn(move || {
                    let found = (me.forward)(&name);
                    let mut inner = me.lock();
                    let entry = inner.addresses.entry(name).or_default();
                    entry.list = found;
                    entry.at = Some(Instant::now());
                    entry.resolving = false;
                });
            if spawned.is_err() {
                self.lock().addresses.clear();
            }
        }
        last.iter()
            .map(Ipv4Addr::to_string)
            .filter(|a| a != server)
            .collect()
    }

    /// The name learned for the server at `server` (an address as in the UNC paths), if any.
    pub fn name_of(&self, server: &str) -> Option<String> {
        self.lock().names.get(server).cloned()
    }

    /// The names learned since the last call, to be saved in the catalog.
    pub fn take_learned(&self) -> Vec<(String, String)> {
        std::mem::take(&mut self.lock().learned)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn names() -> Arc<ServerNames> {
        let reverse: Arc<ReverseFn> = Arc::new(|ip: Ipv4Addr| {
            (ip == Ipv4Addr::new(192, 168, 1, 65)).then(|| "nas.local".to_owned())
        });
        let forward: Arc<ForwardFn> = Arc::new(|name: &str| {
            if name == "nas.local" {
                vec![Ipv4Addr::new(192, 168, 1, 57)]
            } else {
                Vec::new()
            }
        });
        Arc::new(ServerNames::new(reverse, forward, Vec::new()))
    }

    fn settle(until: impl Fn() -> bool) {
        let start = Instant::now();
        while !until() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "lookups did not settle"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_server_reached_by_address_is_found_again_by_its_name() {
        let n = names();
        // Unknown server: nothing to try.
        assert!(n.moved("192.168.1.65").is_empty());
        // Reached: its name is learned once, to be saved.
        n.reached("192.168.1.65");
        n.reached("nas"); // a name already: nothing to learn
        settle(|| !n.lock().names.is_empty());
        assert_eq!(
            n.take_learned(),
            vec![("192.168.1.65".to_owned(), "nas.local".to_owned())]
        );
        assert!(n.take_learned().is_empty());
        // Gone from its address: the name is looked up in the background, then the new address
        // is offered.
        let _ = n.moved("192.168.1.65");
        settle(|| !n.moved("192.168.1.65").is_empty());
        assert_eq!(n.moved("192.168.1.65"), vec!["192.168.1.57".to_owned()]);
    }

    #[test]
    fn saved_names_are_used_without_a_lookup_of_the_address() {
        let n = Arc::new(ServerNames::new(
            Arc::new(|_| None),
            Arc::new(|_: &str| vec![Ipv4Addr::new(10, 0, 0, 9), Ipv4Addr::new(10, 0, 0, 5)]),
            vec![("10.0.0.5".to_owned(), "box.local".to_owned())],
        ));
        let _ = n.moved("10.0.0.5");
        settle(|| !n.moved("10.0.0.5").is_empty());
        // Its own address is not a new one.
        assert_eq!(n.moved("10.0.0.5"), vec!["10.0.0.9".to_owned()]);
    }
}
