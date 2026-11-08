//! A ZimaOS NAS (REVIEW D66): its RAID arrays, their disks and the health of the disks, read
//! from the NAS's own web interface with the user's login. The shared folders say nothing of
//! this (SMB only gives the size and the free space).
//!
//! The interface is the one the ZimaOS web panel uses, not a documented one: every field is
//! optional here, and an answer that cannot be read is reported as `unsupported`, never as a
//! problem of the disks. Only GET requests after the login: Prism never changes the NAS.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde::Deserialize;

/// What Prism knows of a NAS.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NasInfo {
    /// RAID arrays and single-disk data volumes.
    pub storages: Vec<NasStorage>,
    pub disks: Vec<NasDisk>,
}

/// A volume of the NAS: a RAID array, or a data partition of a single disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NasStorage {
    pub name: String,
    pub mount: String,
    /// RAID level (0, 1, 5…); `None` for a single disk.
    pub raid_level: Option<u32>,
    /// As ZimaOS says it ("ok", "degraded"…); `None` for a single disk.
    pub raid_status: Option<String>,
    /// What the array is doing ("idle", "resync", "recovery"…), and how far.
    pub activity: Option<String>,
    pub progress: Option<u32>,
    pub fs: String,
    pub size: u64,
    pub used: Option<u64>,
    /// Serial numbers of its disks.
    pub disks: Vec<String>,
}

/// A disk of the NAS.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NasDisk {
    pub serial: String,
    pub model: String,
    pub vendor: String,
    /// `ssd`, `hdd` or `unknown`.
    pub kind: String,
    /// `sata`, `nvme`, `usb`… when ZimaOS says it.
    pub interface: Option<String>,
    pub size: u64,
    /// `None` when the NAS cannot read the disk's SMART data (temperature and hours both 0).
    pub temperature: Option<i64>,
    pub power_on_hours: Option<u64>,
    pub healthy: bool,
    pub faulty: bool,
    pub missing: bool,
    /// The volume it belongs to, if any.
    pub storage: Option<String>,
    /// The disk ZimaOS itself runs from.
    pub system: bool,
}

impl NasStorage {
    /// `ok`, `attention` (a disk not healthy, an array rebuilding) or `problem` (an array not
    /// ok, a disk faulty or missing).
    #[must_use]
    pub fn state(&self, disks: &[NasDisk]) -> &'static str {
        let members: Vec<&NasDisk> = disks
            .iter()
            .filter(|d| self.disks.contains(&d.serial))
            .collect();
        let raid_bad = self
            .raid_status
            .as_deref()
            .is_some_and(|s| !s.eq_ignore_ascii_case("ok"));
        if raid_bad || members.iter().any(|d| d.faulty || d.missing) {
            "problem"
        } else if members.iter().any(|d| !d.healthy)
            || self
                .activity
                .as_deref()
                .is_some_and(|a| !a.eq_ignore_ascii_case("idle"))
        {
            "attention"
        } else {
            "ok"
        }
    }
}

impl NasInfo {
    /// The volume a shared folder shows: the one with its name, or — ZimaOS's own share —
    /// the data partition of the system disk.
    #[must_use]
    pub fn storage_for_share(&self, share: &str) -> Option<&NasStorage> {
        self.storages
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(share))
            .or_else(|| {
                share
                    .eq_ignore_ascii_case("ZimaOS-HD")
                    .then(|| self.storages.iter().find(|s| s.mount == "/DATA"))
                    .flatten()
            })
    }

    /// What is wrong now, as stable keys: an array not ok, a disk not healthy, faulty or
    /// missing. A key that was not there at the previous reading is news for a notification.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in &self.storages {
            if let Some(status) = s
                .raid_status
                .as_deref()
                .filter(|s| !s.eq_ignore_ascii_case("ok"))
            {
                out.push(format!("raid|{}|{}", s.name, status.to_lowercase()));
            }
        }
        for d in &self.disks {
            // A missing disk is that and nothing more: its health is not known.
            let what = if d.missing {
                Some("missing")
            } else if d.faulty {
                Some("faulty")
            } else if !d.healthy {
                Some("unhealthy")
            } else {
                None
            };
            if let Some(what) = what {
                out.push(format!("disk|{}|{what}", d.serial));
            }
        }
        out
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: Option<T>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawRaid {
    name: String,
    mount_point: String,
    raid_level: Option<i64>,
    raid_status: Option<String>,
    status: Option<String>,
    percentage: Option<f64>,
    fs_type: String,
    size: u64,
    used: Option<u64>,
    devices: Vec<RawDevice>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawDevice {
    serial: String,
    #[serde(rename = "type")]
    interface: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawDisk {
    serial: String,
    model: String,
    vendor: String,
    disk_type: String,
    disk_interface: Option<String>,
    size: u64,
    temperature: Option<i64>,
    power_on_hours: Option<u64>,
    health: Option<bool>,
    faulty: bool,
    missing: bool,
    children: Vec<RawPart>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawPart {
    storage_name: Option<String>,
    mount_point: Option<String>,
    raid: bool,
    file_system: Option<String>,
    size: Option<u64>,
    used: Option<u64>,
}

/// Reads the answers of `GET /v2/local_storage/raid` and `GET /v2/local_storage/disk`.
///
/// # Errors
/// `unsupported` when an answer is not what ZimaOS gives.
pub fn parse(raid: &str, disk: &str) -> Result<NasInfo, String> {
    let raids: Envelope<Vec<RawRaid>> =
        serde_json::from_str(raid).map_err(|_| "unsupported".to_owned())?;
    let disks: Envelope<Vec<RawDisk>> =
        serde_json::from_str(disk).map_err(|_| "unsupported".to_owned())?;
    let raids = raids.data.unwrap_or_default();
    let disks = disks.data.unwrap_or_default();

    let mut info = NasInfo::default();
    for r in &raids {
        info.storages.push(NasStorage {
            name: r.name.clone(),
            mount: r.mount_point.clone(),
            raid_level: r.raid_level.and_then(|l| u32::try_from(l).ok()),
            raid_status: r.raid_status.clone().filter(|s| !s.is_empty()),
            activity: r.status.clone().filter(|s| !s.is_empty()),
            progress: r
                .percentage
                .filter(|p| *p > 0.0)
                .map(|p| p.clamp(0.0, 100.0).round() as u32),
            fs: r.fs_type.clone(),
            size: r.size,
            used: r.used,
            disks: r.devices.iter().map(|d| d.serial.clone()).collect(),
        });
    }
    let interface_of = |serial: &str| {
        raids
            .iter()
            .flat_map(|r| &r.devices)
            .find(|d| d.serial == serial)
            .and_then(|d| d.interface.clone())
    };
    for d in &disks {
        let system = d
            .children
            .iter()
            .any(|c| c.mount_point.as_deref() == Some("/"));
        // A single-disk data volume: the data partition of the system disk (`/DATA`), or a disk
        // mounted under `/media` without RAID.
        for c in d.children.iter().filter(|c| !c.raid) {
            let (Some(name), Some(mount)) = (c.storage_name.as_deref(), c.mount_point.as_deref())
            else {
                continue;
            };
            if mount == "/DATA" || mount.starts_with("/media/") {
                info.storages.push(NasStorage {
                    name: name.to_owned(),
                    mount: mount.to_owned(),
                    fs: c.file_system.clone().unwrap_or_default(),
                    size: c.size.unwrap_or(0),
                    used: c.used,
                    disks: vec![d.serial.clone()],
                    ..NasStorage::default()
                });
            }
        }
        let storage = info
            .storages
            .iter()
            .find(|s| s.disks.contains(&d.serial))
            .map(|s| s.name.clone());
        let smart = !(d.temperature.unwrap_or(0) == 0 && d.power_on_hours.unwrap_or(0) == 0);
        let model = if d.model == "System" && !d.vendor.is_empty() {
            d.vendor.clone()
        } else {
            d.model.clone()
        };
        info.disks.push(NasDisk {
            serial: d.serial.clone(),
            model,
            vendor: d.vendor.clone(),
            kind: match d.disk_type.to_ascii_lowercase().as_str() {
                "ssd" => "ssd",
                "hdd" => "hdd",
                _ => "unknown",
            }
            .to_owned(),
            interface: d
                .disk_interface
                .clone()
                .or_else(|| interface_of(&d.serial))
                .map(|i| i.to_ascii_lowercase()),
            size: d.size,
            temperature: d.temperature.filter(|_| smart),
            power_on_hours: d.power_on_hours.filter(|_| smart),
            healthy: d.health.unwrap_or(true),
            faulty: d.faulty,
            missing: d.missing,
            storage,
            system,
        });
    }
    // Disks of an array that the disk list does not show any more.
    for r in &raids {
        for dev in &r.devices {
            if !info.disks.iter().any(|d| d.serial == dev.serial) {
                info.disks.push(NasDisk {
                    serial: dev.serial.clone(),
                    kind: "unknown".to_owned(),
                    missing: true,
                    storage: Some(r.name.clone()),
                    ..NasDisk::default()
                });
            }
        }
    }
    Ok(info)
}

/// One HTTP/1.1 request to the NAS on port 80 (its web panel is plain HTTP on the home network):
/// the status and the body. Blocking, with time limits.
fn http(
    server: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    let unreachable = |_| "unreachable".to_owned();
    let addrs: Vec<SocketAddr> = (server, 80)
        .to_socket_addrs()
        .map_err(unreachable)?
        .collect();
    // IPv4 first: a NAS name can resolve to an IPv6 address the web panel does not listen on.
    let addr = addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or_else(|| addrs.first())
        .ok_or_else(|| "unreachable".to_owned())?;
    let mut stream =
        TcpStream::connect_timeout(addr, Duration::from_secs(8)).map_err(unreachable)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(unreachable)?;
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {server}\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: {token}\r\n"));
    }
    let body = body.unwrap_or("");
    if method == "POST" {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).map_err(unreachable)?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).map_err(unreachable)?;
    let split = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "unsupported".to_owned())?;
    let head = String::from_utf8_lossy(&answer[..split]).to_ascii_lowercase();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| "unsupported".to_owned())?;
    let raw = &answer[split + 4..];
    let content = if head.contains("transfer-encoding: chunked") {
        dechunk(raw).ok_or_else(|| "unsupported".to_owned())?
    } else {
        raw.to_vec()
    };
    Ok((status, String::from_utf8_lossy(&content).into_owned()))
}

/// The body of a `Transfer-Encoding: chunked` answer.
fn dechunk(mut raw: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = raw.windows(2).position(|w| w == b"\r\n")?;
        let size_text = std::str::from_utf8(&raw[..line_end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        raw = &raw[line_end + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(raw.get(..size)?);
        raw = raw.get(size + 2..)?;
    }
}

/// Logs in to the ZimaOS at `server` and reads its volumes and disks. Blocking.
///
/// # Errors
/// `unreachable` (no answer), `login_failed` (wrong user or password), `unsupported` (not a
/// ZimaOS, or an interface Prism does not know).
pub fn fetch(server: &str, user: &str, password: &str) -> Result<NasInfo, String> {
    let body = serde_json::json!({ "username": user, "password": password }).to_string();
    let (status, login) = http(server, "POST", "/v1/users/login", None, Some(&body))?;
    if status == 401 || status == 403 {
        return Err("login_failed".to_owned());
    }
    let login: serde_json::Value =
        serde_json::from_str(&login).map_err(|_| "unsupported".to_owned())?;
    let Some(token) = login
        .pointer("/data/token/access_token")
        .and_then(serde_json::Value::as_str)
    else {
        return Err(
            if login.get("success").and_then(serde_json::Value::as_i64) == Some(200) {
                "unsupported"
            } else {
                "login_failed"
            }
            .to_owned(),
        );
    };
    let get = |path: &str| -> Result<String, String> {
        let (status, text) = http(server, "GET", path, Some(token), None)?;
        if (200..300).contains(&status) {
            Ok(text)
        } else {
            Err("unsupported".to_owned())
        }
    };
    let raid = get("/v2/local_storage/raid")?;
    let disk = get("/v2/local_storage/disk")?;
    parse(&raid, &disk)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    // The shape of the answers of a real ZimaOS (2 RAID 1 of HDDs, an SSD with the system and
    // the DATA partition), with invented serial numbers.
    const RAID: &str = r#"{"data":[
      {"dev_size":499972571136,"devices":[
        {"disk_type":"HDD","faulty":false,"health":true,"model":"HD502HJ","serial":"SER-A1","size":500107862016,"type":"sata","vendor":"SAMSUNG","power_on_hours":715},
        {"disk_type":"HDD","faulty":false,"health":true,"model":"HTS545050A7E680","serial":"SER-A2","size":500107862016,"type":"sata","vendor":"HGST","power_on_hours":0}],
       "fs_type":"BTRFS","mount_point":"/media/Small-Storage","name":"Small-Storage","path":"/dev/md0","percentage":0,
       "raid_level":1,"raid_status":"ok","size":499972571136,"status":"idle","used":110536704000},
      {"devices":[
        {"disk_type":"HDD","serial":"SER-B1","type":"sata","model":"WD10EARS-22Y5B1","vendor":"WD"},
        {"disk_type":"HDD","serial":"SER-B2","type":"sata","model":"ST1000DM003-1CH162","vendor":"SEAGATE"}],
       "fs_type":"BTRFS","mount_point":"/media/Big-Storage","name":"Big-Storage","raid_level":1,"raid_status":"ok",
       "size":1000069595136,"status":"idle","used":661416398848}],"message":"success"}"#;
    const DISK: &str = r#"{"data":[
      {"avail":"120034123776","children":[
        {"file_system":"FAT16","mount_point":"/mnt/boot","name":"sdb1","raid":false,"size":33554432,"storage_name":"boot","used":798720},
        {"file_system":"squashfs","mount_point":"/","name":"sdb5","raid":false,"size":6442450944,"storage_name":"/"},
        {"avail":101521596416,"file_system":"ext4","mount_point":"/DATA","name":"sdb8","raid":false,"size":106955218432,"storage_name":"DATA","used":3138236416}],
       "disk_interface":"SATA","disk_type":"SSD","health":true,"model":"System","serial":"SER-S","size":120034123776,"temperature":21,"power_on_hours":8715,"vendor":"SAMSUNG"},
      {"children":[{"file_system":"BTRFS","mount_point":"/media/Small-Storage","raid":true,"raid_level":1,"storage_name":"Small-Storage"}],
       "disk_type":"HDD","health":true,"model":"HD502HJ","serial":"SER-A1","size":500107862016,"temperature":25,"power_on_hours":715,"vendor":"SAMSUNG"},
      {"children":[{"mount_point":"/media/Small-Storage","raid":true,"storage_name":"Small-Storage"}],
       "disk_type":"HDD","health":true,"model":"HTS545050A7E680","serial":"SER-A2","size":500107862016,"temperature":0,"power_on_hours":0,"vendor":"HGST"},
      {"children":[{"mount_point":"/media/Big-Storage","raid":true,"storage_name":"Big-Storage"}],
       "disk_type":"HDD","health":true,"model":"WD10EARS-22Y5B1","serial":"SER-B1","size":1000204886016,"temperature":30,"power_on_hours":8938,"vendor":"WD"}
    ],"message":"success"}"#;

    #[test]
    fn volumes_and_disks_of_a_zimaos() {
        let info = parse(RAID, DISK).unwrap();
        let names: Vec<&str> = info.storages.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Small-Storage", "Big-Storage", "DATA"]);
        let small = &info.storages[0];
        assert_eq!(
            (small.raid_level, small.raid_status.as_deref()),
            (Some(1), Some("ok"))
        );
        assert_eq!(small.disks, ["SER-A1", "SER-A2"]);
        assert_eq!(small.used, Some(110_536_704_000));
        assert_eq!(small.state(&info.disks), "ok");
        let data = &info.storages[2];
        assert_eq!((data.raid_level, data.mount.as_str()), (None, "/DATA"));

        let system = info.disks.iter().find(|d| d.serial == "SER-S").unwrap();
        assert!(system.system);
        assert_eq!(
            (system.model.as_str(), system.kind.as_str()),
            ("SAMSUNG", "ssd")
        );
        assert_eq!(system.interface.as_deref(), Some("sata"));
        assert_eq!(system.storage.as_deref(), Some("DATA"));
        let hgst = info.disks.iter().find(|d| d.serial == "SER-A2").unwrap();
        assert_eq!(
            (hgst.temperature, hgst.power_on_hours),
            (None, None),
            "no SMART data"
        );
        assert_eq!(hgst.interface.as_deref(), Some("sata"), "from the array");
        // A member of an array the disk list leaves out counts as missing.
        let gone = info.disks.iter().find(|d| d.serial == "SER-B2").unwrap();
        assert!(gone.missing);
        assert_eq!(info.storages[1].state(&info.disks), "problem");
        assert_eq!(info.problems(), ["disk|SER-B2|missing"]);
    }

    #[test]
    fn shared_folders_are_matched_to_volumes() {
        let info = parse(RAID, DISK).unwrap();
        assert_eq!(
            info.storage_for_share("big-storage").unwrap().name,
            "Big-Storage"
        );
        assert_eq!(info.storage_for_share("ZimaOS-HD").unwrap().name, "DATA");
        assert!(info.storage_for_share("Media").is_none());
    }

    #[test]
    fn a_degraded_array_and_an_unreadable_answer() {
        let degraded = RAID.replacen(r#""raid_status":"ok""#, r#""raid_status":"degraded""#, 1);
        let info = parse(&degraded, DISK).unwrap();
        assert_eq!(info.storages[0].state(&info.disks), "problem");
        assert!(
            info.problems()
                .contains(&"raid|Small-Storage|degraded".to_owned())
        );
        assert_eq!(parse("<html>", DISK).unwrap_err(), "unsupported");
    }

    /// The real NAS, read only: `PRISM_ZIMAOS=<server>` with `PRISM_ZIMAOS_USER` and
    /// `PRISM_ZIMAOS_PASSWORD` (`cargo test -p prism-app --bin prism zimaos -- --ignored`).
    #[test]
    #[ignore = "needs a ZimaOS NAS and its login"]
    fn a_real_zimaos_answers() {
        let server = std::env::var("PRISM_ZIMAOS").unwrap();
        let user = std::env::var("PRISM_ZIMAOS_USER").unwrap();
        let password = std::env::var("PRISM_ZIMAOS_PASSWORD").unwrap();
        let info = fetch(&server, &user, &password).unwrap();
        for s in &info.storages {
            println!(
                "{} {:?} {:?} {} disks, {}",
                s.name,
                s.raid_level,
                s.raid_status,
                s.disks.len(),
                s.state(&info.disks)
            );
        }
        for d in &info.disks {
            println!(
                "{} {} {:?} {:?} °C {:?} h healthy={} in {:?}",
                d.model, d.kind, d.interface, d.temperature, d.power_on_hours, d.healthy, d.storage
            );
        }
        assert!(!info.disks.is_empty());
        assert_eq!(
            fetch(&server, &user, "not-the-password").unwrap_err(),
            "login_failed"
        );
    }

    #[test]
    fn chunked_answers_are_joined() {
        assert_eq!(
            dechunk(b"4\r\n{\"a\"\r\n3;x=1\r\n:1}\r\n0\r\n\r\n").unwrap(),
            b"{\"a\":1}"
        );
        assert!(dechunk(b"9\r\nshort\r\n").is_none());
    }
}
