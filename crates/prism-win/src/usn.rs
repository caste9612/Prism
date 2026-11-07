//! The NTFS change journal (USN journal, ADR-0007): state of the journal and records since a
//! saved position. Needs a volume handle (`\\.\X:`), i.e. administrator rights.
//!
//! Prism never creates or modifies a journal on user volumes (data is only read): volumes
//! without an active journal are updated by a new enumeration instead.

use std::fs::File;

use crate::ioctl::{FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL, as_bytes, device_io};

const ERROR_JOURNAL_DELETE_IN_PROGRESS: u32 = 1178;
const ERROR_JOURNAL_NOT_ACTIVE: u32 = 1179;
const ERROR_JOURNAL_ENTRY_DELETED: u32 = 1181;

/// File was created.
pub const REASON_FILE_CREATE: u32 = 0x0000_0100;
/// File was deleted.
pub const REASON_FILE_DELETE: u32 = 0x0000_0200;
/// Old name of a renamed or moved file.
pub const REASON_RENAME_OLD_NAME: u32 = 0x0000_1000;
/// New name of a renamed or moved file.
pub const REASON_RENAME_NEW_NAME: u32 = 0x0000_2000;
/// A hard link was added or removed.
pub const REASON_HARD_LINK_CHANGE: u32 = 0x0001_0000;
/// Last record for a file until it is opened again.
pub const REASON_CLOSE: u32 = 0x8000_0000;

/// Identity and range of the journal of a volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalInfo {
    pub id: u64,
    /// Oldest record still available.
    pub first_usn: i64,
    /// Position of the next record that will be written.
    pub next_usn: i64,
}

/// One change record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsnRecord {
    pub usn: i64,
    /// File reference of the changed file (record number and sequence).
    pub file_ref: u64,
    /// File reference of its parent folder (for a rename, the folder of this name).
    pub parent_ref: u64,
    /// `REASON_*` flags accumulated since the file was opened.
    pub reason: u32,
    pub attrs: u32,
    pub name: Vec<u16>,
}

/// Records read since a position, and the position to continue from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsnBatch {
    pub records: Vec<UsnRecord>,
    pub next_usn: i64,
}

/// Errors of the journal functions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsnError {
    /// No journal on the volume (or being deleted): update by enumeration.
    #[error("the volume has no active change journal")]
    NotActive,
    /// The journal was recreated, or the saved position was overwritten: the changes since the
    /// last update are unknown and a full reconciliation is needed (ADR-0007).
    #[error("the change journal no longer covers the saved position")]
    Lost,
    #[error("change journal I/O failed with Windows error {0}")]
    Io(u32),
    #[error("malformed change journal data at offset {0}")]
    Malformed(usize),
}

fn map_error(code: u32) -> UsnError {
    match code {
        ERROR_JOURNAL_NOT_ACTIVE | ERROR_JOURNAL_DELETE_IN_PROGRESS => UsnError::NotActive,
        ERROR_JOURNAL_ENTRY_DELETED => UsnError::Lost,
        other => UsnError::Io(other),
    }
}

/// The journal of the volume opened as `volume` (`\\.\X:`), or `None` if it has none.
///
/// # Errors
/// [`UsnError::Io`] for other failures.
pub fn query_journal(volume: &File) -> Result<Option<JournalInfo>, UsnError> {
    let mut out = [0u64; 8]; // USN_JOURNAL_DATA_V0 is 56 bytes
    match device_io(volume, FSCTL_QUERY_USN_JOURNAL, &[], &mut out) {
        Ok(n) if n >= 24 => Ok(Some(JournalInfo {
            id: out[0],
            first_usn: i64::from_le_bytes(out[1].to_le_bytes()),
            next_usn: i64::from_le_bytes(out[2].to_le_bytes()),
        })),
        Ok(n) => Err(UsnError::Malformed(n)),
        Err(code) => match map_error(code) {
            UsnError::NotActive => Ok(None),
            e => Err(e),
        },
    }
}

/// Checks that a saved position can still be continued from.
///
/// # Errors
/// [`UsnError::NotActive`] if there is no journal, [`UsnError::Lost`] if the journal was
/// recreated (different id) or the position is older than the oldest record.
pub fn check_position(
    volume: &File,
    journal_id: u64,
    next_usn: i64,
) -> Result<JournalInfo, UsnError> {
    let info = query_journal(volume)?.ok_or(UsnError::NotActive)?;
    if info.id != journal_id || next_usn < info.first_usn || next_usn > info.next_usn {
        return Err(UsnError::Lost);
    }
    Ok(info)
}

/// Reads every record from `start` to the end of the journal, at most `limit` records (the
/// returned `next_usn` tells where to continue).
///
/// # Errors
/// [`UsnError::Lost`] if `start` was overwritten meanwhile, other [`UsnError`]s.
pub fn read_journal(
    volume: &File,
    journal_id: u64,
    start: i64,
    limit: usize,
) -> Result<UsnBatch, UsnError> {
    let mut batch = UsnBatch {
        records: Vec::new(),
        next_usn: start,
    };
    let mut out = vec![0u64; 64 * 1024 / 8];
    loop {
        // READ_USN_JOURNAL_DATA_V0: StartUsn, ReasonMask, ReturnOnlyOnClose, Timeout,
        // BytesToWaitFor, UsnJournalID. No waiting: return what is there.
        let mut input = Vec::with_capacity(40);
        input.extend_from_slice(&batch.next_usn.to_le_bytes());
        input.extend_from_slice(&u32::MAX.to_le_bytes());
        input.extend_from_slice(&0u32.to_le_bytes());
        input.extend_from_slice(&0u64.to_le_bytes());
        input.extend_from_slice(&0u64.to_le_bytes());
        input.extend_from_slice(&journal_id.to_le_bytes());
        let n = device_io(volume, FSCTL_READ_USN_JOURNAL, &input, &mut out).map_err(map_error)?;
        let (next, records) = parse_usn_buffer(as_bytes(&out, n))?;
        let done = records.is_empty() || next <= batch.next_usn;
        batch.records.extend(records);
        batch.next_usn = next.max(batch.next_usn);
        if done || batch.records.len() >= limit {
            return Ok(batch);
        }
    }
}

fn u16_at(b: &[u8], off: usize) -> Result<u16, UsnError> {
    b.get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(UsnError::Malformed(off))
}

fn u32_at(b: &[u8], off: usize) -> Result<u32, UsnError> {
    b.get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(UsnError::Malformed(off))
}

fn u64_at(b: &[u8], off: usize) -> Result<u64, UsnError> {
    b.get(off..off + 8)
        .and_then(|s| s.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(UsnError::Malformed(off))
}

/// Parses the output of `FSCTL_READ_USN_JOURNAL`: the next USN, then `USN_RECORD_V2` or
/// `USN_RECORD_V3` records (V3 carries 128-bit references; on NTFS the high half is zero).
///
/// # Errors
/// [`UsnError::Malformed`] for inconsistent lengths or unknown versions.
pub fn parse_usn_buffer(b: &[u8]) -> Result<(i64, Vec<UsnRecord>), UsnError> {
    let next = i64::from_le_bytes(u64_at(b, 0)?.to_le_bytes());
    let mut records = Vec::new();
    let mut pos = 8usize;
    while pos + 8 <= b.len() {
        let len = usize::try_from(u32_at(b, pos)?).map_err(|_| UsnError::Malformed(pos))?;
        if len < 60 || pos + len > b.len() {
            return Err(UsnError::Malformed(pos));
        }
        let r = &b[pos..pos + len];
        let major = u16_at(r, 4)?;
        let (file_ref, parent_ref, rest) = match major {
            2 => (u64_at(r, 8)?, u64_at(r, 16)?, 24),
            3 => (u64_at(r, 8)?, u64_at(r, 24)?, 40),
            _ => return Err(UsnError::Malformed(pos)),
        };
        let usn = i64::from_le_bytes(u64_at(r, rest)?.to_le_bytes());
        let reason = u32_at(r, rest + 16)?;
        let attrs = u32_at(r, rest + 28)?;
        let name_len = usize::from(u16_at(r, rest + 32)?);
        let name_off = usize::from(u16_at(r, rest + 34)?);
        let name = r
            .get(name_off..name_off + name_len)
            .ok_or(UsnError::Malformed(pos + name_off))?
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        records.push(UsnRecord {
            usn,
            file_ref,
            parent_ref,
            reason,
            attrs,
            name,
        });
        pos += len;
    }
    Ok((next, records))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2(file_ref: u64, parent: u64, usn: i64, reason: u32, name: &str) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let len = (60 + 2 * units.len()).div_ceil(8) * 8;
        let mut r = vec![0u8; len];
        r[0..4].copy_from_slice(&u32::try_from(len).unwrap().to_le_bytes());
        r[4..6].copy_from_slice(&2u16.to_le_bytes());
        r[8..16].copy_from_slice(&file_ref.to_le_bytes());
        r[16..24].copy_from_slice(&parent.to_le_bytes());
        r[24..32].copy_from_slice(&usn.to_le_bytes());
        r[40..44].copy_from_slice(&reason.to_le_bytes());
        r[52..56].copy_from_slice(&0x20u32.to_le_bytes());
        r[56..58].copy_from_slice(&u16::try_from(2 * units.len()).unwrap().to_le_bytes());
        r[58..60].copy_from_slice(&60u16.to_le_bytes());
        for (i, u) in units.iter().enumerate() {
            r[60 + 2 * i..62 + 2 * i].copy_from_slice(&u.to_le_bytes());
        }
        r
    }

    fn v3(file_ref: u64, parent: u64, usn: i64, reason: u32, name: &str) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let len = (76 + 2 * units.len()).div_ceil(8) * 8;
        let mut r = vec![0u8; len];
        r[0..4].copy_from_slice(&u32::try_from(len).unwrap().to_le_bytes());
        r[4..6].copy_from_slice(&3u16.to_le_bytes());
        r[8..16].copy_from_slice(&file_ref.to_le_bytes());
        r[24..32].copy_from_slice(&parent.to_le_bytes());
        r[40..48].copy_from_slice(&usn.to_le_bytes());
        r[56..60].copy_from_slice(&reason.to_le_bytes());
        r[68..72].copy_from_slice(&0x10u32.to_le_bytes());
        r[72..74].copy_from_slice(&u16::try_from(2 * units.len()).unwrap().to_le_bytes());
        r[74..76].copy_from_slice(&76u16.to_le_bytes());
        for (i, u) in units.iter().enumerate() {
            r[76 + 2 * i..78 + 2 * i].copy_from_slice(&u.to_le_bytes());
        }
        r
    }

    #[test]
    fn parses_v2_and_v3_records() {
        let mut buf = 5000i64.to_le_bytes().to_vec();
        buf.extend(v2(100, 5, 4000, REASON_FILE_CREATE, "new.txt"));
        buf.extend(v3(
            101,
            5,
            4100,
            REASON_RENAME_NEW_NAME | REASON_CLOSE,
            "Fotografie",
        ));
        let (next, records) = parse_usn_buffer(&buf).unwrap();
        assert_eq!(next, 5000);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].file_ref, 100);
        assert_eq!(records[0].usn, 4000);
        assert_eq!(String::from_utf16_lossy(&records[0].name), "new.txt");
        assert_eq!(records[0].attrs, 0x20);
        assert_eq!(records[1].parent_ref, 5);
        assert_eq!(records[1].reason, REASON_RENAME_NEW_NAME | REASON_CLOSE);
        assert_eq!(records[1].attrs, 0x10);
        assert_eq!(String::from_utf16_lossy(&records[1].name), "Fotografie");
    }

    #[test]
    fn only_the_next_usn() {
        let (next, records) = parse_usn_buffer(&77i64.to_le_bytes()).unwrap();
        assert_eq!((next, records.len()), (77, 0));
    }

    #[test]
    fn malformed_buffers_are_errors() {
        let mut buf = 1i64.to_le_bytes().to_vec();
        let mut r = v2(1, 5, 1, 0, "x");
        r[0..4].copy_from_slice(&999u32.to_le_bytes()); // longer than the buffer
        buf.extend(r);
        assert!(matches!(
            parse_usn_buffer(&buf),
            Err(UsnError::Malformed(8))
        ));
        let mut bad_version = 1i64.to_le_bytes().to_vec();
        let mut r = v2(1, 5, 1, 0, "x");
        r[4] = 9;
        bad_version.extend(r);
        assert!(parse_usn_buffer(&bad_version).is_err());
        assert!(parse_usn_buffer(&[1, 2, 3]).is_err());
    }

    #[test]
    fn errors_map_to_journal_states() {
        assert_eq!(map_error(1179), UsnError::NotActive);
        assert_eq!(map_error(1178), UsnError::NotActive);
        assert_eq!(map_error(1181), UsnError::Lost);
        assert_eq!(map_error(5), UsnError::Io(5));
    }
}
