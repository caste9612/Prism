//! Health of the disks (F17): what the SMART data of a disk says, as an overall status (good,
//! warning, critical) with the values the user cares about, and whether a new reading is worse
//! than the previous one (F17-AC3). Pure functions over the raw data returned by the disks:
//! reading them is `prism-win::health`.
//!
//! - NVMe disks: the SMART / health information log page (512 bytes).
//! - ATA/SATA disks: the SMART attribute table and its thresholds (512 bytes each).

/// Overall status of a disk (F17-AC1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HealthStatus {
    Good,
    Warning,
    Critical,
}

impl HealthStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "good" => Some(Self::Good),
            "warning" => Some(Self::Warning),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

/// What a reading says (F17-AC1); `None` where the disk does not report a value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HealthReading {
    pub status: Option<HealthStatus>,
    pub temperature_c: Option<i32>,
    pub power_on_hours: Option<u64>,
    /// Share of the rated life used (SSD), 0–100 or more.
    pub wear_percent: Option<u32>,
    /// Reallocated sectors (HDD, ATA attribute 5).
    pub reallocated: Option<u64>,
    /// Sectors waiting to be reallocated (ATA attribute 197).
    pub pending: Option<u64>,
    /// Media and data integrity errors (NVMe).
    pub media_errors: Option<u64>,
    /// Remaining spare capacity, percent (NVMe).
    pub available_spare: Option<u32>,
    /// Why the status is not good: stable codes translated by the interface.
    pub reasons: Vec<&'static str>,
}

impl HealthReading {
    fn flag(&mut self, status: HealthStatus, reason: &'static str) {
        self.status = Some(self.status.map_or(status, |s| s.max(status)));
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }
}

/// Temperatures from which a disk is reported as too hot.
const HOT_C: i32 = 65;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// The low 64 bits of a 128-bit little-endian counter (enough for any real disk).
fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// The reading of an NVMe health information log page.
#[must_use]
pub fn nvme_reading(log: &[u8]) -> Option<HealthReading> {
    if log.len() < 192 {
        return None;
    }
    let critical_warning = log[0];
    let kelvin = i32::from(u16_at(log, 1)?);
    let spare = u32::from(log[3]);
    let spare_threshold = u32::from(log[4]);
    let used = u32::from(log[5]);
    let mut r = HealthReading {
        status: Some(HealthStatus::Good),
        temperature_c: (kelvin > 0).then_some(kelvin - 273),
        power_on_hours: u64_at(log, 128),
        wear_percent: Some(used),
        media_errors: u64_at(log, 160),
        available_spare: Some(spare),
        ..HealthReading::default()
    };
    // Bits: 0 spare below threshold, 1 temperature, 2 reliability degraded, 3 read-only,
    // 4 volatile memory backup failed.
    if critical_warning & 0b0001_1101 != 0 || (spare_threshold > 0 && spare < spare_threshold) {
        r.flag(HealthStatus::Critical, "nvme_critical_warning");
    }
    if critical_warning & 0b0000_0010 != 0 {
        r.flag(HealthStatus::Warning, "temperature");
    }
    if used >= 100 {
        r.flag(HealthStatus::Critical, "worn_out");
    } else if used >= 90 {
        r.flag(HealthStatus::Warning, "wear_high");
    }
    if r.media_errors.is_some_and(|e| e > 0) {
        r.flag(HealthStatus::Warning, "media_errors");
    }
    if r.temperature_c.is_some_and(|t| t >= HOT_C) {
        r.flag(HealthStatus::Warning, "temperature");
    }
    Some(r)
}

/// A SMART attribute of an ATA disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtaAttribute {
    pub id: u8,
    /// Normalised value (higher is better) and the worst seen.
    pub current: u8,
    pub worst: u8,
    /// Raw value, 48 bits.
    pub raw: u64,
    /// The manufacturer's threshold: at or below it the attribute has failed (0: none).
    pub threshold: u8,
}

/// The attributes of an ATA SMART data page with the thresholds of the thresholds page.
#[must_use]
pub fn ata_attributes(values: &[u8], thresholds: &[u8]) -> Vec<AtaAttribute> {
    let mut out = Vec::new();
    for i in 0..30 {
        let at = 2 + i * 12;
        let Some(entry) = values.get(at..at + 12) else {
            break;
        };
        if entry[0] == 0 {
            continue;
        }
        let mut raw = [0u8; 8];
        raw[..6].copy_from_slice(&entry[5..11]);
        let threshold = (0..30)
            .filter_map(|j| thresholds.get(2 + j * 12..2 + j * 12 + 2))
            .find(|t| t[0] == entry[0])
            .map_or(0, |t| t[1]);
        out.push(AtaAttribute {
            id: entry[0],
            current: entry[3],
            worst: entry[4],
            raw: u64::from_le_bytes(raw),
            threshold,
        });
    }
    out
}

/// The reading of an ATA disk from its attributes; `failing` is the overall SMART status the
/// disk returns (RETURN STATUS).
#[must_use]
pub fn ata_reading(attrs: &[AtaAttribute], failing: bool) -> HealthReading {
    let get = |id: u8| attrs.iter().find(|a| a.id == id);
    let mut r = HealthReading {
        status: Some(HealthStatus::Good),
        // 194 (or 190 on some disks): the current temperature is the lowest byte of the raw value.
        temperature_c: get(194)
            .or_else(|| get(190))
            .map(|a| i32::from((a.raw & 0xFF) as u8))
            .filter(|t| *t > 0 && *t < 120),
        power_on_hours: get(9).map(|a| a.raw & 0xFFFF_FFFF),
        reallocated: get(5).map(|a| a.raw),
        pending: get(197).map(|a| a.raw),
        // SSD life: 231 (life left), 233 (media wearout), 177 (wear leveling), 202 (percent
        // lifetime remaining): normalised values that start at 100.
        wear_percent: [231u8, 233, 177, 202]
            .iter()
            .find_map(|id| get(*id))
            .map(|a| u32::from(100u8.saturating_sub(a.current.min(100)))),
        ..HealthReading::default()
    };
    if failing {
        r.flag(HealthStatus::Critical, "smart_failing");
    }
    if attrs
        .iter()
        .any(|a| a.threshold > 0 && a.current <= a.threshold)
    {
        r.flag(HealthStatus::Critical, "attribute_failed");
    }
    if r.reallocated.is_some_and(|n| n > 0) {
        r.flag(HealthStatus::Warning, "reallocated");
    }
    if r.pending.is_some_and(|n| n > 0) || get(198).is_some_and(|a| a.raw > 0) {
        r.flag(HealthStatus::Warning, "pending");
    }
    if r.wear_percent.is_some_and(|w| w >= 90) {
        r.flag(HealthStatus::Warning, "wear_high");
    }
    if r.temperature_c.is_some_and(|t| t >= HOT_C) {
        r.flag(HealthStatus::Warning, "temperature");
    }
    r
}

/// Why `now` is worse than `before`, if it is (F17-AC3): a worse status, or more reallocated,
/// pending sectors or media errors.
#[must_use]
pub fn worsening(before: &HealthReading, now: &HealthReading) -> Option<&'static str> {
    if now.status > before.status && now.status.is_some() && before.status.is_some() {
        return Some("status");
    }
    let more = |a: Option<u64>, b: Option<u64>| matches!((a, b), (Some(x), Some(y)) if y > x);
    if more(before.reallocated, now.reallocated) {
        return Some("reallocated");
    }
    if more(before.pending, now.pending) {
        return Some("pending");
    }
    if more(before.media_errors, now.media_errors) {
        return Some("media_errors");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nvme(critical: u8, kelvin: u16, spare: u8, used: u8, hours: u64, errors: u64) -> Vec<u8> {
        let mut log = vec![0u8; 512];
        log[0] = critical;
        log[1..3].copy_from_slice(&kelvin.to_le_bytes());
        log[3] = spare;
        log[4] = 10;
        log[5] = used;
        log[128..136].copy_from_slice(&hours.to_le_bytes());
        log[160..168].copy_from_slice(&errors.to_le_bytes());
        log
    }

    // F17-AC1: an NVMe disk in good health, worn, failing.
    #[test]
    fn nvme_health() {
        let good = nvme_reading(&nvme(0, 310, 100, 3, 12_345, 0)).unwrap();
        assert_eq!(good.status, Some(HealthStatus::Good));
        assert_eq!(good.temperature_c, Some(37));
        assert_eq!(good.power_on_hours, Some(12_345));
        assert_eq!(good.wear_percent, Some(3));
        assert_eq!(good.available_spare, Some(100));
        assert!(good.reasons.is_empty());

        let worn = nvme_reading(&nvme(0, 310, 100, 92, 1, 0)).unwrap();
        assert_eq!(worn.status, Some(HealthStatus::Warning));
        assert_eq!(worn.reasons, ["wear_high"]);
        let errors = nvme_reading(&nvme(0, 310, 100, 1, 1, 2)).unwrap();
        assert_eq!(errors.reasons, ["media_errors"]);
        let failing = nvme_reading(&nvme(0b0100, 310, 100, 1, 1, 0)).unwrap();
        assert_eq!(failing.status, Some(HealthStatus::Critical));
        let spare = nvme_reading(&nvme(0, 310, 5, 1, 1, 0)).unwrap();
        assert_eq!(spare.status, Some(HealthStatus::Critical));
        assert!(nvme_reading(&[0u8; 10]).is_none());
    }

    fn ata_page(entries: &[(u8, u8, u64)]) -> Vec<u8> {
        let mut page = vec![0u8; 512];
        for (i, (id, current, raw)) in entries.iter().enumerate() {
            let at = 2 + i * 12;
            page[at] = *id;
            page[at + 3] = *current;
            page[at + 4] = *current;
            page[at + 5..at + 11].copy_from_slice(&raw.to_le_bytes()[..6]);
        }
        page
    }

    fn thresholds(entries: &[(u8, u8)]) -> Vec<u8> {
        let mut page = vec![0u8; 512];
        for (i, (id, t)) in entries.iter().enumerate() {
            page[2 + i * 12] = *id;
            page[2 + i * 12 + 1] = *t;
        }
        page
    }

    // F17-AC1: an HDD with reallocated and pending sectors, an SSD with its wear, a failing disk.
    #[test]
    fn ata_health() {
        let hdd = ata_page(&[
            (5, 100, 0),
            (9, 98, 21_000),
            (194, 100, 0x0028_0000_0022),
            (197, 100, 0),
        ]);
        let limits = thresholds(&[(5, 36), (9, 0), (194, 0), (197, 0)]);
        let attrs = ata_attributes(&hdd, &limits);
        assert_eq!(attrs.len(), 4);
        assert_eq!(attrs[0].threshold, 36);
        let good = ata_reading(&attrs, false);
        assert_eq!(good.status, Some(HealthStatus::Good));
        assert_eq!(good.temperature_c, Some(34));
        assert_eq!(good.power_on_hours, Some(21_000));
        assert_eq!((good.reallocated, good.pending), (Some(0), Some(0)));

        let worse = ata_attributes(&ata_page(&[(5, 99, 8), (197, 100, 2)]), &limits);
        let warn = ata_reading(&worse, false);
        assert_eq!(warn.status, Some(HealthStatus::Warning));
        assert_eq!(warn.reasons, ["reallocated", "pending"]);
        assert_eq!(worsening(&good, &warn), Some("status"));

        let failed = ata_attributes(&ata_page(&[(5, 30, 900)]), &limits);
        assert_eq!(
            ata_reading(&failed, false).status,
            Some(HealthStatus::Critical)
        );
        assert_eq!(ata_reading(&attrs, true).reasons, ["smart_failing"]);

        let ssd = ata_attributes(&ata_page(&[(9, 99, 5000), (231, 87, 0)]), &thresholds(&[]));
        assert_eq!(ata_reading(&ssd, false).wear_percent, Some(13));
    }

    // F17-AC3: what counts as worse.
    #[test]
    fn worsening_readings() {
        let base = HealthReading {
            status: Some(HealthStatus::Warning),
            reallocated: Some(8),
            pending: Some(0),
            ..HealthReading::default()
        };
        let same = base.clone();
        assert_eq!(worsening(&base, &same), None);
        let more = HealthReading {
            reallocated: Some(9),
            ..base.clone()
        };
        assert_eq!(worsening(&base, &more), Some("reallocated"));
        let better = HealthReading {
            status: Some(HealthStatus::Good),
            ..base.clone()
        };
        assert_eq!(worsening(&base, &better), None);
        assert_eq!(
            HealthStatus::parse("critical"),
            Some(HealthStatus::Critical)
        );
    }
}
