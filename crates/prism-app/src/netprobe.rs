//! Reachability and free space of network shares (REVIEW L5).
//!
//! A mapped drive exists even when its server is off, and asking an unreachable server for its
//! free space can block for tens of seconds. So the volume monitor never asks directly: it
//! tells the probe which shares exist ([`NetProbe::want`]) and reads the last known answer
//! ([`NetProbe::status`]). The probe asks each share in a thread of its own with a time limit;
//! a share that does not answer in time counts as unreachable until it does. A share never
//! probed yet is [`NetStatus::Unknown`]: the monitor treats it as not connected, so a server
//! that is off never flickers online at startup.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use prism_core::volumes::normalize_unc;

/// What is known about a share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetStatus {
    Unknown,
    Reachable { capacity: u64, free: u64 },
    Unreachable,
}

/// Asks a share for (capacity, free space); `None` if it cannot be reached.
pub type ProbeFn = dyn Fn(&str) -> Option<(u64, u64)> + Send + Sync;

/// Timing of the probe.
#[derive(Clone, Copy, Debug)]
pub struct ProbeTiming {
    /// Longest wait for an answer.
    pub timeout: Duration,
    /// How often a reachable share is asked again (free space, still there).
    pub recheck_reachable: Duration,
    /// How often an unreachable share is tried again.
    pub recheck_unreachable: Duration,
}

impl Default for ProbeTiming {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            recheck_reachable: Duration::from_secs(30),
            recheck_unreachable: Duration::from_secs(10),
        }
    }
}

#[derive(Default)]
struct Inner {
    /// Shares to keep probing (normalized UNC → path as given).
    wanted: HashMap<String, String>,
    results: HashMap<String, (Instant, NetStatus)>,
    /// Probes still running, with their start time.
    in_flight: HashMap<String, Instant>,
}

pub struct NetProbe {
    probe: Arc<ProbeFn>,
    timing: ProbeTiming,
    inner: Mutex<Inner>,
    wake: Condvar,
}

impl NetProbe {
    pub fn new(probe: Arc<ProbeFn>, timing: ProbeTiming) -> Self {
        Self {
            probe,
            timing,
            inner: Mutex::new(Inner::default()),
            wake: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The shares to watch (UNC paths); the others are forgotten. New shares are probed at once.
    pub fn want<'a>(&self, uncs: impl IntoIterator<Item = &'a str>) {
        let wanted: HashMap<String, String> = uncs
            .into_iter()
            .map(|u| (normalize_unc(u), u.to_owned()))
            .collect();
        let mut inner = self.lock();
        let new = wanted.keys().any(|k| !inner.wanted.contains_key(k));
        inner.results.retain(|k, _| wanted.contains_key(k));
        inner.wanted = wanted;
        drop(inner);
        if new {
            self.wake.notify_all();
        }
    }

    /// Last known state of a share.
    pub fn status(&self, unc: &str) -> NetStatus {
        self.lock()
            .results
            .get(&normalize_unc(unc))
            .map_or(NetStatus::Unknown, |(_, s)| *s)
    }

    /// One pass: marks probes past their time limit as unreachable and starts the probes that
    /// are due. Returns at once; answers arrive from the probe threads.
    pub fn tick(self: &Arc<Self>) {
        let now = Instant::now();
        let mut inner = self.lock();
        let late: Vec<String> = inner
            .in_flight
            .iter()
            .filter(|(_, started)| now.duration_since(**started) >= self.timing.timeout)
            .map(|(k, _)| k.clone())
            .collect();
        for key in late {
            if inner.wanted.contains_key(&key) {
                inner.results.insert(key, (now, NetStatus::Unreachable));
            }
        }
        let due: Vec<(String, String)> = inner
            .wanted
            .iter()
            .filter(|(k, _)| !inner.in_flight.contains_key(*k))
            .filter(|(k, _)| match inner.results.get(*k) {
                None => true,
                Some((at, NetStatus::Unreachable)) => {
                    now.duration_since(*at) >= self.timing.recheck_unreachable
                }
                Some((at, _)) => now.duration_since(*at) >= self.timing.recheck_reachable,
            })
            .map(|(k, u)| (k.clone(), u.clone()))
            .collect();
        for (key, unc) in due {
            inner.in_flight.insert(key.clone(), now);
            let me = Arc::clone(self);
            let spawned = std::thread::Builder::new()
                .name("prism-net-probe".into())
                .spawn(move || {
                    let answer = (me.probe)(&unc);
                    let status = answer.map_or(NetStatus::Unreachable, |(capacity, free)| {
                        NetStatus::Reachable { capacity, free }
                    });
                    let mut inner = me.lock();
                    inner.in_flight.remove(&key);
                    if inner.wanted.contains_key(&key) {
                        inner.results.insert(key, (Instant::now(), status));
                    }
                });
            if spawned.is_err() {
                inner.in_flight.clear();
                break;
            }
        }
    }

    /// Runs forever: a pass every half second, or at once when a new share is wanted.
    pub fn run(self: &Arc<Self>) {
        loop {
            self.tick();
            let inner = self.lock();
            let _ = self
                .wake
                .wait_timeout(inner, Duration::from_millis(500))
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn fake() -> Arc<NetProbe> {
        let probe: Arc<ProbeFn> = Arc::new(|unc: &str| {
            if unc.contains("slow") {
                std::thread::sleep(Duration::from_millis(600));
                Some((1, 1))
            } else if unc.contains("fast") {
                Some((1000, 400))
            } else {
                None
            }
        });
        Arc::new(NetProbe::new(
            probe,
            ProbeTiming {
                timeout: Duration::from_millis(200),
                recheck_reachable: Duration::from_secs(60),
                recheck_unreachable: Duration::from_secs(60),
            },
        ))
    }

    fn settle(p: &Arc<NetProbe>, until: impl Fn() -> bool) {
        let start = Instant::now();
        while !until() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "probe did not settle"
            );
            p.tick();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn reachable_unreachable_and_late_shares() {
        let p = fake();
        p.want([r"\\fast\share", r"\\down\share", r"\\slow\share"]);
        assert_eq!(p.status(r"\\fast\share"), NetStatus::Unknown);
        settle(&p, || {
            p.status(r"\\fast\share") != NetStatus::Unknown
                && p.status(r"\\down\share") != NetStatus::Unknown
                && p.status(r"\\slow\share") != NetStatus::Unknown
        });
        // Keys are normalized: case and trailing backslash do not matter.
        assert_eq!(
            p.status(r"\\FAST\Share\"),
            NetStatus::Reachable {
                capacity: 1000,
                free: 400
            }
        );
        assert_eq!(p.status(r"\\down\share"), NetStatus::Unreachable);
        // Past the time limit the slow share counts as unreachable, then its late answer wins.
        assert_eq!(p.status(r"\\slow\share"), NetStatus::Unreachable);
        settle(&p, || {
            p.status(r"\\slow\share")
                == NetStatus::Reachable {
                    capacity: 1,
                    free: 1,
                }
        });
    }

    #[test]
    fn forgotten_shares_lose_their_state() {
        let p = fake();
        p.want([r"\\fast\share"]);
        settle(&p, || p.status(r"\\fast\share") != NetStatus::Unknown);
        p.want(std::iter::empty());
        assert_eq!(p.status(r"\\fast\share"), NetStatus::Unknown);
    }
}
