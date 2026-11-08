//! NAS in the app (REVIEW D66): the RAID arrays and the disks of a ZimaOS NAS, read with the
//! login the user gave in the settings (kept in the Windows Credential Manager, never in the
//! catalog). Read after the login and then every hour; a problem that was not there at the
//! previous reading (an array not ok, a disk faulty, missing or not healthy) raises a
//! notification. Prism only reads: nothing is ever changed on the NAS.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use prism_core::model::{Volume, VolumeId, VolumeKind, VolumeStatus, unix_millis_now};
use prism_core::names;
use prism_core::store;
use serde::Serialize;
use ts_rs::TS;

use crate::dto::ApiError;
use crate::state::AppState;
use crate::zimaos::{NasDisk, NasInfo, NasStorage};

/// Where the logins are kept: the Windows Credential Manager, or memory in the tests.
pub trait Logins: Send + Sync {
    fn save(&self, server: &str, user: &str, password: &str) -> Result<(), String>;
    /// User and password.
    fn load(&self, server: &str) -> Option<(String, String)>;
    fn delete(&self, server: &str) -> Result<(), String>;
    /// The servers with a login (case-folded).
    fn servers(&self) -> Vec<String>;
}

/// Logins in the Windows Credential Manager, as `<prefix><server>` (development builds use
/// their own prefix, so they never see the logins of the installed Prism, TESTING S4).
pub struct WindowsLogins {
    pub prefix: &'static str,
}

impl WindowsLogins {
    fn target(&self, server: &str) -> String {
        format!("{}{}", self.prefix, names::fold(server))
    }
}

impl Logins for WindowsLogins {
    fn save(&self, server: &str, user: &str, password: &str) -> Result<(), String> {
        prism_win::credentials::save(&self.target(server), user, password)
            .map_err(|e| format!("Credential Manager error {e}"))
    }

    fn load(&self, server: &str) -> Option<(String, String)> {
        match prism_win::credentials::load(&self.target(server)) {
            Ok(c) => c.map(|c| (c.user, c.secret)),
            Err(e) => {
                tracing::warn!(error = e, "NAS login not readable");
                None
            }
        }
    }

    fn delete(&self, server: &str) -> Result<(), String> {
        prism_win::credentials::delete(&self.target(server))
            .map(|_| ())
            .map_err(|e| format!("Credential Manager error {e}"))
    }

    fn servers(&self) -> Vec<String> {
        prism_win::credentials::targets(self.prefix)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| t.strip_prefix(self.prefix).map(str::to_owned))
            .collect()
    }
}

/// Reads a NAS: server, user, password.
pub type Fetch = Arc<dyn Fn(&str, &str, &str) -> Result<NasInfo, String> + Send + Sync>;

/// The last reading of a NAS.
#[derive(Clone, Debug)]
pub struct NasReading {
    pub user: String,
    /// The last good reading and when it was made.
    pub info: Option<NasInfo>,
    pub read_at: Option<i64>,
    /// Why the last attempt failed (`unreachable`, `login_failed`, `unsupported`).
    pub error: Option<String>,
}

pub struct NasState {
    logins: Arc<dyn Logins>,
    fetch: Fetch,
    /// By case-folded server.
    readings: Mutex<HashMap<String, NasReading>>,
}

impl NasState {
    pub fn new(logins: Arc<dyn Logins>, fetch: Fetch) -> Self {
        Self {
            logins,
            fetch,
            readings: Mutex::new(HashMap::new()),
        }
    }

    fn readings(&self) -> MutexGuard<'_, HashMap<String, NasReading>> {
        self.readings.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn reading(&self, server: &str) -> Option<NasReading> {
        self.readings().get(&names::fold(server)).cloned()
    }

    /// The servers with a saved login.
    pub fn connected(&self) -> BTreeSet<String> {
        self.logins.servers().into_iter().collect()
    }

    /// Reads the NAS again with its saved login; `None` when it has none.
    pub fn refresh(&self, server: &str) -> Option<Result<NasInfo, String>> {
        let (user, password) = self.logins.load(server)?;
        let result = (self.fetch)(server, &user, &password);
        let mut readings = self.readings();
        let entry = readings
            .entry(names::fold(server))
            .or_insert_with(|| NasReading {
                user: user.clone(),
                info: None,
                read_at: None,
                error: None,
            });
        entry.user = user;
        match &result {
            Ok(info) => {
                entry.info = Some(info.clone());
                entry.read_at = Some(unix_millis_now());
                entry.error = None;
            }
            Err(e) => entry.error = Some(e.clone()),
        }
        Some(result)
    }

    /// Checks the login by reading the NAS, then saves it.
    pub fn connect(&self, server: &str, user: &str, password: &str) -> Result<NasInfo, String> {
        let info = (self.fetch)(server, user, password)?;
        self.logins.save(server, user, password)?;
        self.readings().insert(
            names::fold(server),
            NasReading {
                user: user.to_owned(),
                info: Some(info.clone()),
                read_at: Some(unix_millis_now()),
                error: None,
            },
        );
        Ok(info)
    }

    /// Forgets the login and what was read.
    pub fn disconnect(&self, server: &str) -> Result<(), String> {
        self.logins.delete(server)?;
        self.readings().remove(&names::fold(server));
        Ok(())
    }

    /// Whether the last attempt of some NAS failed (it is then tried again sooner).
    pub fn any_error(&self) -> bool {
        self.readings().values().any(|r| r.error.is_some())
    }
}

/// The RAID array (or single-disk volume) of the NAS that a network share shows.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RaidView {
    pub storage: String,
    /// RAID level; `None` for a single disk.
    pub level: Option<u32>,
    /// `ok`, `attention` or `problem`.
    pub state: String,
    /// What the array is doing besides idling (`resync`, `recovery`…), and how far.
    pub activity: Option<String>,
    pub progress: Option<u32>,
    pub disks: u32,
    /// The last reading failed: this is the one before.
    pub stale: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NasStorageView {
    pub name: String,
    pub level: Option<u32>,
    /// As ZimaOS says it (`ok`, `degraded`…); `None` for a single disk.
    pub status: Option<String>,
    /// `ok`, `attention` or `problem`.
    pub state: String,
    pub activity: Option<String>,
    pub progress: Option<u32>,
    pub fs: String,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number | null")]
    pub used: Option<u64>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NasDiskView {
    pub model: String,
    pub serial: String,
    /// `ssd`, `hdd` or `unknown`.
    pub kind: String,
    pub interface: Option<String>,
    #[ts(type = "number")]
    pub size: u64,
    /// `None` when the NAS cannot read the SMART data of the disk.
    #[ts(type = "number | null")]
    pub temperature: Option<i64>,
    #[ts(type = "number | null")]
    pub power_on_hours: Option<u64>,
    pub healthy: bool,
    pub faulty: bool,
    pub missing: bool,
    /// The disk ZimaOS runs from.
    pub system: bool,
}

/// The NAS behind a network share: its volume and the disks of that volume.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NasHealthView {
    pub server: String,
    /// A login is saved for this NAS.
    pub connected: bool,
    #[ts(type = "number | null")]
    pub read_at: Option<i64>,
    /// Why the last reading failed (`unreachable`, `login_failed`, `unsupported`).
    pub error: Option<String>,
    /// `None` also when the NAS has no volume called like the share.
    pub storage: Option<NasStorageView>,
    pub disks: Vec<NasDiskView>,
}

/// A NAS of the catalog, for the settings.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NasServerView {
    /// As written in the network paths of its shares.
    pub server: String,
    /// The name learned for it (REVIEW D59).
    pub name: Option<String>,
    pub shares: u32,
    pub connected: bool,
    pub user: Option<String>,
    #[ts(type = "number | null")]
    pub read_at: Option<i64>,
    pub error: Option<String>,
    /// The worst state of its volumes (`ok`, `attention`, `problem`), once read.
    pub state: Option<String>,
    pub storages: u32,
    pub disks: u32,
}

fn storage_view(s: &NasStorage, disks: &[NasDisk]) -> NasStorageView {
    NasStorageView {
        name: s.name.clone(),
        level: s.raid_level,
        status: s.raid_status.clone(),
        state: s.state(disks).to_owned(),
        activity: s
            .activity
            .clone()
            .filter(|a| !a.eq_ignore_ascii_case("idle")),
        progress: s.progress,
        fs: s.fs.clone(),
        size: s.size,
        used: s.used,
    }
}

fn disk_view(d: &NasDisk) -> NasDiskView {
    NasDiskView {
        model: d.model.clone(),
        serial: d.serial.clone(),
        kind: d.kind.clone(),
        interface: d.interface.clone(),
        size: d.size,
        temperature: d.temperature,
        power_on_hours: d.power_on_hours,
        healthy: d.healthy,
        faulty: d.faulty,
        missing: d.missing,
        system: d.system,
    }
}

/// `ok` < `attention` < `problem`.
fn worst<'a>(states: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let rank = |s: &str| match s {
        "problem" => 2,
        "attention" => 1,
        _ => 0,
    };
    states.max_by_key(|s| rank(s))
}

/// The server and the shared folder of a UNC path (`\\nas\Media\x` → `nas`, `Media`).
pub fn unc_share(unc: &str) -> Option<(String, String)> {
    let rest = unc.replace('/', "\\");
    let mut parts = rest.strip_prefix(r"\\")?.split('\\');
    let server = parts.next().filter(|s| !s.is_empty() && *s != "?")?;
    let share = parts.next().filter(|s| !s.is_empty())?;
    Some((server.to_owned(), share.to_owned()))
}

/// How the notification names a problem: the array, or the disk and its array.
fn problem_name(info: &NasInfo, key: &str) -> String {
    let mut parts = key.split('|');
    match (parts.next(), parts.next()) {
        (Some("raid"), Some(name)) => name.to_owned(),
        (Some("disk"), Some(serial)) => {
            let disk = info.disks.iter().find(|d| d.serial == serial);
            let model = disk
                .map(|d| d.model.clone())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| serial.to_owned());
            match disk.and_then(|d| d.storage.clone()) {
                Some(storage) => format!("{model} ({storage})"),
                None => model,
            }
        }
        _ => key.to_owned(),
    }
}

/// The problems not there at the previous reading, by the name the notification gives them.
fn news(info: &NasInfo, before: &BTreeSet<String>) -> (Vec<String>, BTreeSet<String>) {
    let now: BTreeSet<String> = info.problems().into_iter().collect();
    let mut names: Vec<String> = now
        .difference(before)
        .map(|k| problem_name(info, k))
        .collect();
    names.dedup();
    (names, now)
}

fn problems_key(server: &str) -> String {
    format!("nas_problems:{}", names::fold(server))
}

impl AppState {
    fn known_problems(&self, server: &str) -> BTreeSet<String> {
        self.store
            .read(|c| store::setting(c, &problems_key(server)))
            .ok()
            .flatten()
            .map(|v| v.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn save_problems(&self, server: &str, problems: &BTreeSet<String>) {
        let (key, value) = (
            problems_key(server),
            problems.iter().cloned().collect::<Vec<_>>().join("\n"),
        );
        if let Err(e) = self
            .store
            .write(move |w| store::set_setting(w, &key, &value))
        {
            tracing::warn!(error = %e, "NAS problems not saved");
        }
    }

    /// Reads every NAS with a login; returns the names of the new problems (for the
    /// notification) and whether a card changes.
    pub fn nas_tick(&self) -> (Vec<String>, bool) {
        let mut found = Vec::new();
        let mut changed = false;
        for server in self.nas.connected() {
            let before = self.nas.reading(&server).map(|r| summary(&r));
            let Some(result) = self.nas.refresh(&server) else {
                continue;
            };
            match result {
                Ok(info) => {
                    let (names, now) = news(&info, &self.known_problems(&server));
                    for n in &names {
                        tracing::warn!(server = %server, problem = %n, "NAS problem");
                    }
                    found.extend(names);
                    if now != self.known_problems(&server) {
                        self.save_problems(&server, &now);
                    }
                }
                Err(e) => tracing::info!(server = %server, error = %e, "NAS not read"),
            }
            changed |= self.nas.reading(&server).map(|r| summary(&r)) != before;
        }
        (found, changed)
    }

    /// The servers of the network shares of the catalog, case-folded, with how they are written
    /// and how many shares each has.
    fn catalog_servers(&self) -> Result<BTreeMap<String, (String, u32)>, ApiError> {
        let volumes = self.store.read(store::volumes)?;
        let mut out: BTreeMap<String, (String, u32)> = BTreeMap::new();
        for v in volumes
            .iter()
            .filter(|v| v.info.kind == VolumeKind::Network && v.status != VolumeStatus::Ignored)
        {
            if let Some((server, _)) = v.info.unc_path.as_deref().and_then(unc_share) {
                out.entry(names::fold(&server))
                    .or_insert_with(|| (server, 0))
                    .1 += 1;
            }
        }
        Ok(out)
    }

    /// The NAS of the catalog and of the saved logins, for the settings.
    pub fn nas_servers(&self) -> Result<Vec<NasServerView>, ApiError> {
        let mut servers = self.catalog_servers()?;
        let connected = self.nas.connected();
        for c in &connected {
            servers.entry(c.clone()).or_insert_with(|| (c.clone(), 0));
        }
        Ok(servers
            .into_iter()
            .map(|(folded, (server, shares))| {
                let reading = self.nas.reading(&folded);
                let info = reading.as_ref().and_then(|r| r.info.as_ref());
                NasServerView {
                    name: self.names.name_of(&folded),
                    shares,
                    connected: connected.contains(&folded),
                    user: reading.as_ref().map(|r| r.user.clone()),
                    read_at: reading.as_ref().and_then(|r| r.read_at),
                    error: reading.as_ref().and_then(|r| r.error.clone()),
                    state: info.and_then(|i| {
                        worst(i.storages.iter().map(|s| s.state(&i.disks))).map(str::to_owned)
                    }),
                    storages: info.map_or(0, |i| i.storages.len() as u32),
                    disks: info.map_or(0, |i| i.disks.len() as u32),
                    server,
                }
            })
            .collect())
    }

    /// Saves the login of a NAS of the catalog after reading it once with it.
    pub fn nas_connect(&self, server: &str, user: &str, password: &str) -> Result<(), ApiError> {
        let server = server.trim();
        if !self.catalog_servers()?.contains_key(&names::fold(server)) {
            return Err(ApiError::new("nas_unknown_server", server));
        }
        if user.trim().is_empty() || password.is_empty() {
            return Err(ApiError::new("nas_login_failed", "empty user or password"));
        }
        let info = self
            .nas
            .connect(server, user.trim(), password)
            .map_err(|e| ApiError::new(&format!("nas_{e}"), server))?;
        // What is wrong already is shown now: only what comes after is news.
        self.save_problems(server, &info.problems().into_iter().collect());
        tracing::info!(server, storages = info.storages.len(), "NAS connected");
        Ok(())
    }

    pub fn nas_disconnect(&self, server: &str) -> Result<(), ApiError> {
        self.nas
            .disconnect(server)
            .map_err(|e| ApiError::new("nas_credentials", e))?;
        self.save_problems(server, &BTreeSet::new());
        tracing::info!(server, "NAS disconnected");
        Ok(())
    }

    /// The array of the NAS that a network share of the catalog shows, once read.
    pub fn raid_view(&self, volume: &Volume) -> Option<RaidView> {
        if volume.info.kind != VolumeKind::Network {
            return None;
        }
        let (server, share) = volume.info.unc_path.as_deref().and_then(unc_share)?;
        let reading = self.nas.reading(&server)?;
        let info = reading.info.as_ref()?;
        let s = info.storage_for_share(&share)?;
        Some(RaidView {
            storage: s.name.clone(),
            level: s.raid_level,
            state: s.state(&info.disks).to_owned(),
            activity: s
                .activity
                .clone()
                .filter(|a| !a.eq_ignore_ascii_case("idle")),
            progress: s.progress,
            disks: s.disks.len() as u32,
            stale: reading.error.is_some(),
        })
    }

    /// The NAS behind a network share of the catalog: its volume and its disks.
    pub fn nas_health(&self, volume: VolumeId) -> Result<Option<NasHealthView>, ApiError> {
        let v = self.store.read(|c| store::volume(c, volume))?;
        let Some((server, share)) = v
            .filter(|v| v.info.kind == VolumeKind::Network)
            .and_then(|v| v.info.unc_path.as_deref().and_then(unc_share))
        else {
            return Ok(None);
        };
        let connected = self.nas.connected().contains(&names::fold(&server));
        let reading = self.nas.reading(&server);
        let info = reading.as_ref().and_then(|r| r.info.as_ref());
        let storage = info.and_then(|i| i.storage_for_share(&share));
        Ok(Some(NasHealthView {
            connected,
            read_at: reading.as_ref().and_then(|r| r.read_at),
            error: reading.as_ref().and_then(|r| r.error.clone()),
            storage: storage.zip(info).map(|(s, i)| storage_view(s, &i.disks)),
            disks: storage
                .zip(info)
                .map(|(s, i)| {
                    i.disks
                        .iter()
                        .filter(|d| s.disks.contains(&d.serial))
                        .map(disk_view)
                        .collect()
                })
                .unwrap_or_default(),
            server,
        }))
    }
}

/// What the cards show of a reading: whether it changed is a reason to refresh them.
fn summary(r: &NasReading) -> (Option<String>, Vec<(String, &'static str)>) {
    let states = r
        .info
        .as_ref()
        .map(|i| {
            i.storages
                .iter()
                .map(|s| (s.name.clone(), s.state(&i.disks)))
                .collect()
        })
        .unwrap_or_default();
    (r.error.clone(), states)
}

/// Logins in memory, for the tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryLogins(Mutex<HashMap<String, (String, String)>>);

#[cfg(test)]
impl Logins for MemoryLogins {
    fn save(&self, server: &str, user: &str, password: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(names::fold(server), (user.to_owned(), password.to_owned()));
        Ok(())
    }
    fn load(&self, server: &str) -> Option<(String, String)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&names::fold(server))
            .cloned()
    }
    fn delete(&self, server: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&names::fold(server));
        Ok(())
    }
    fn servers(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::zimaos::{NasDisk, NasStorage};

    fn nas(disk_ok: bool) -> NasInfo {
        NasInfo {
            storages: vec![NasStorage {
                name: "Big-Storage".into(),
                mount: "/media/Big-Storage".into(),
                raid_level: Some(1),
                raid_status: Some("ok".into()),
                activity: Some("idle".into()),
                disks: vec!["A".into(), "B".into()],
                ..NasStorage::default()
            }],
            disks: vec![
                NasDisk {
                    serial: "A".into(),
                    model: "WD10EARS".into(),
                    kind: "hdd".into(),
                    healthy: true,
                    storage: Some("Big-Storage".into()),
                    ..NasDisk::default()
                },
                NasDisk {
                    serial: "B".into(),
                    model: "ST1000DM003".into(),
                    kind: "hdd".into(),
                    healthy: disk_ok,
                    storage: Some("Big-Storage".into()),
                    ..NasDisk::default()
                },
            ],
        }
    }

    #[test]
    fn shares_and_their_server() {
        assert_eq!(
            unc_share(r"\\192.168.1.65\Big-Storage"),
            Some(("192.168.1.65".into(), "Big-Storage".into()))
        );
        assert_eq!(
            unc_share(r"\\nas\Media\Film"),
            Some(("nas".into(), "Media".into()))
        );
        assert_eq!(unc_share(r"\\nas"), None);
        assert_eq!(unc_share(r"C:\x"), None);
    }

    #[test]
    fn the_login_is_kept_only_when_it_works() {
        let logins = Arc::new(MemoryLogins::default());
        let state = NasState::new(
            logins.clone(),
            Arc::new(|_: &str, _: &str, p: &str| {
                if p == "right" {
                    Ok(nas(true))
                } else {
                    Err("login_failed".to_owned())
                }
            }),
        );
        assert_eq!(
            state.connect("NAS", "me", "wrong").unwrap_err(),
            "login_failed"
        );
        assert!(state.connected().is_empty());
        state.connect("NAS", "me", "right").unwrap();
        assert_eq!(
            state.connected().into_iter().collect::<Vec<_>>(),
            [names::fold("nas")],
            "one login, whatever the case"
        );
        assert_eq!(state.reading("nas").unwrap().user, "me");
        assert!(state.refresh("nas").unwrap().is_ok());
        assert!(state.refresh("other").is_none(), "no login");
        state.disconnect("nas").unwrap();
        assert!(state.connected().is_empty());
        assert!(state.reading("nas").is_none());
    }

    #[test]
    fn a_failed_reading_keeps_the_last_good_one() {
        let fail = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let f = Arc::clone(&fail);
        let state = NasState::new(
            Arc::new(MemoryLogins::default()),
            Arc::new(move |_: &str, _: &str, _: &str| {
                if f.load(std::sync::atomic::Ordering::SeqCst) {
                    Err("unreachable".to_owned())
                } else {
                    Ok(nas(true))
                }
            }),
        );
        state.connect("nas", "me", "pw").unwrap();
        fail.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(state.refresh("nas").unwrap().is_err());
        let r = state.reading("nas").unwrap();
        assert_eq!(r.error.as_deref(), Some("unreachable"));
        assert!(r.info.is_some());
        assert!(state.any_error());
    }

    // Only problems not there before raise a notification, named so the user knows which disk.
    #[test]
    fn new_problems_only() {
        let (names, now) = news(&nas(true), &BTreeSet::new());
        assert!(names.is_empty() && now.is_empty());
        let (names, now) = news(&nas(false), &BTreeSet::new());
        assert_eq!(names, ["ST1000DM003 (Big-Storage)"]);
        let (names, _) = news(&nas(false), &now);
        assert!(names.is_empty(), "already announced");
        let mut degraded = nas(true);
        degraded.storages[0].raid_status = Some("degraded".into());
        let (names, _) = news(&degraded, &now);
        assert_eq!(names, ["Big-Storage"]);
    }

    #[test]
    fn worst_state() {
        assert_eq!(
            worst(["ok", "problem", "attention"].into_iter()),
            Some("problem")
        );
        assert_eq!(worst(["ok", "attention"].into_iter()), Some("attention"));
        assert_eq!(worst(std::iter::empty()), None);
    }
}
