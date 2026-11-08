//! Diagnostic package (F20-AC2): the log files and a short system report in a ZIP archive that
//! the user can look at before sending it. The archive is written without compression ("stored"
//! entries), which every ZIP reader opens and needs no extra library.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A file of the package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipEntry {
    /// Name inside the archive, `/` separated.
    pub name: String,
    pub data: Vec<u8>,
}

fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    table
}

/// CRC-32 (IEEE), as ZIP uses it.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let table = crc32_table();
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// UTC year, month, day, hour, minute and second of a time.
fn civil(t: SystemTime) -> (i64, i64, i64, u64, u64, u64) {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, rem / 3600, (rem / 60) % 60, rem % 60)
}

/// The DOS date and time of a ZIP entry (local time is not known here: UTC, 2 s resolution).
fn dos_time(t: SystemTime) -> (u16, u16) {
    let (year, month, day, h, m, s) = civil(t);
    let date = (((year - 1980).clamp(0, 127) as u16) << 9) | ((month as u16) << 5) | day as u16;
    let time = ((h as u16) << 11) | ((m as u16) << 5) | (s / 2) as u16;
    (time, date)
}

/// `20261107-193010`: a UTC time for file names.
#[must_use]
pub fn file_stamp(t: SystemTime) -> String {
    let (year, month, day, h, m, s) = civil(t);
    format!("{year:04}{month:02}{day:02}-{h:02}{m:02}{s:02}")
}

/// A ZIP archive with the entries stored without compression.
#[must_use]
pub fn zip_stored(entries: &[ZipEntry], modified: SystemTime) -> Vec<u8> {
    let (time, date) = dos_time(modified);
    let mut out = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        let crc = crc32(&e.data);
        let size = u32::try_from(e.data.len()).unwrap_or(u32::MAX);
        let name = e.name.as_bytes();
        let name_len = u16::try_from(name.len()).unwrap_or(u16::MAX);
        let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        // Local file header.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&e.data);
        // Central directory entry.
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&[0u8; 8]); // extra and comment lengths, disk, internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let central_at = u32::try_from(out.len()).unwrap_or(u32::MAX);
    let central_len = u32::try_from(central.len()).unwrap_or(u32::MAX);
    let count = u16::try_from(entries.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]); // disk numbers
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_len.to_le_bytes());
    out.extend_from_slice(&central_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment
    out
}

/// The log files of a folder (`prism.*.log`), oldest first, with their sizes.
#[must_use]
pub fn log_files(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut found: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(Result::ok)
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    n.starts_with("prism") && n.ends_with(".log")
                })
                .filter_map(|e| Some((e.path(), e.metadata().ok()?.len())))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_of_known_strings() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn dos_dates() {
        // 2026-11-07 19:30:10 UTC
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_794_079_810);
        let (time, date) = dos_time(t);
        assert_eq!((date >> 9) + 1980, 2026);
        assert_eq!((date >> 5) & 0xF, 11);
        assert_eq!(date & 0x1F, 7);
        assert_eq!(
            (time >> 11, (time >> 5) & 0x3F, (time & 0x1F) * 2),
            (19, 30, 10)
        );
        assert_eq!(file_stamp(t), "20261107-193010");
    }

    /// Reads back the central directory of an archive made by `zip_stored`.
    fn read_back(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
        let end = zip.len() - 22;
        assert_eq!(&zip[end..end + 4], &0x0605_4b50u32.to_le_bytes());
        let count = u16::from_le_bytes([zip[end + 10], zip[end + 11]]) as usize;
        let mut at = u32::from_le_bytes(zip[end + 16..end + 20].try_into().unwrap()) as usize;
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(&zip[at..at + 4], &0x0201_4b50u32.to_le_bytes());
            let crc = u32::from_le_bytes(zip[at + 16..at + 20].try_into().unwrap());
            let size = u32::from_le_bytes(zip[at + 20..at + 24].try_into().unwrap()) as usize;
            let name_len = u16::from_le_bytes([zip[at + 28], zip[at + 29]]) as usize;
            let local = u32::from_le_bytes(zip[at + 42..at + 46].try_into().unwrap()) as usize;
            let name = String::from_utf8(zip[at + 46..at + 46 + name_len].to_vec()).unwrap();
            let data_at = local + 30 + name_len;
            let data = zip[data_at..data_at + size].to_vec();
            assert_eq!(crc32(&data), crc);
            out.push((name, data));
            at += 46 + name_len;
        }
        out
    }

    #[test]
    fn archives_read_back() {
        let entries = vec![
            ZipEntry {
                name: "system.txt".into(),
                data: b"Prism 0.1.0".to_vec(),
            },
            ZipEntry {
                name: "logs/prism.2026-11-07.log".into(),
                data: vec![b'x'; 5000],
            },
            ZipEntry {
                name: "vuoto.txt".into(),
                data: Vec::new(),
            },
        ];
        let zip = zip_stored(&entries, SystemTime::now());
        let back = read_back(&zip);
        assert_eq!(back.len(), 3);
        for (e, (name, data)) in entries.iter().zip(back) {
            assert_eq!((&e.name, &e.data), (&name, &data));
        }
    }

    #[test]
    fn only_log_files_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("prism.2026-11-06.log"), b"a").unwrap();
        std::fs::write(dir.path().join("prism.2026-11-07.log"), b"bb").unwrap();
        std::fs::write(dir.path().join("other.txt"), b"c").unwrap();
        let logs = log_files(dir.path());
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[1].1, 2);
        assert!(log_files(&dir.path().join("missing")).is_empty());
    }
}
