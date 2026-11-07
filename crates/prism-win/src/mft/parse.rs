//! Parsing of NTFS on-disk structures: boot sector, MFT file records, attributes, run lists.
//!
//! Pure functions over byte slices, with bounds checks everywhere: a damaged record yields an
//! error or is skipped, never a panic. References: the NTFS documentation of the Linux-NTFS
//! project and Microsoft's public headers (FILE_RECORD_SEGMENT_HEADER, ATTRIBUTE_RECORD_HEADER).

/// Errors while parsing NTFS structures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("not an NTFS boot sector")]
    NotNtfs,
    #[error("invalid NTFS geometry: {0}")]
    Geometry(&'static str),
    #[error("structure truncated at offset {0}")]
    Truncated(usize),
    #[error("update sequence mismatch in sector {0} (torn write or corruption)")]
    Fixup(usize),
    #[error("invalid run list")]
    RunList,
}

fn u8_at(b: &[u8], off: usize) -> Result<u8, ParseError> {
    b.get(off).copied().ok_or(ParseError::Truncated(off))
}

fn u16_at(b: &[u8], off: usize) -> Result<u16, ParseError> {
    b.get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(ParseError::Truncated(off))
}

fn u32_at(b: &[u8], off: usize) -> Result<u32, ParseError> {
    b.get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(ParseError::Truncated(off))
}

fn u64_at(b: &[u8], off: usize) -> Result<u64, ParseError> {
    b.get(off..off + 8)
        .and_then(|s| s.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(ParseError::Truncated(off))
}

/// Geometry from the NTFS boot sector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootInfo {
    pub bytes_per_sector: u32,
    pub bytes_per_cluster: u64,
    pub record_size: u32,
    /// First cluster of the `$MFT`.
    pub mft_lcn: u64,
}

/// Parses the first sector of an NTFS volume.
///
/// # Errors
/// [`ParseError::NotNtfs`] or [`ParseError::Geometry`] for anything that is not a sane NTFS
/// boot sector.
pub fn parse_boot_sector(b: &[u8]) -> Result<BootInfo, ParseError> {
    if b.get(3..11) != Some(b"NTFS    ".as_slice()) {
        return Err(ParseError::NotNtfs);
    }
    let bytes_per_sector = u32::from(u16_at(b, 0x0B)?);
    let sectors_per_cluster_raw = u8_at(b, 0x0D)?;
    // Values above 0x80 encode 2^(256 - value) sectors (clusters larger than 64 KiB).
    let sectors_per_cluster: u64 = if sectors_per_cluster_raw > 0x80 {
        1u64 << (256 - u32::from(sectors_per_cluster_raw))
    } else {
        u64::from(sectors_per_cluster_raw)
    };
    if !bytes_per_sector.is_power_of_two() || !(256..=4096).contains(&bytes_per_sector) {
        return Err(ParseError::Geometry("bytes per sector"));
    }
    if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
        return Err(ParseError::Geometry("sectors per cluster"));
    }
    let bytes_per_cluster = u64::from(bytes_per_sector) * sectors_per_cluster;
    let mft_lcn = u64_at(b, 0x30)?;
    // Positive: clusters per record; negative: record size is 2^(-value) bytes.
    let raw = i8::from_le_bytes([u8_at(b, 0x40)?]);
    let record_size: u64 = if raw > 0 {
        u64::try_from(raw).unwrap_or(0) * bytes_per_cluster
    } else {
        1u64 << u32::from(raw.unsigned_abs())
    };
    if !(512..=65_536).contains(&record_size) || !record_size.is_power_of_two() {
        return Err(ParseError::Geometry("file record size"));
    }
    Ok(BootInfo {
        bytes_per_sector,
        bytes_per_cluster,
        record_size: u32::try_from(record_size).map_err(|_| ParseError::Geometry("record"))?,
        mft_lcn,
    })
}

/// A contiguous run of clusters of a non-resident attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// First logical cluster on the volume; `None` for a sparse run.
    pub lcn: Option<u64>,
    /// Length in clusters.
    pub clusters: u64,
}

/// Decodes a run list (mapping pairs).
///
/// # Errors
/// [`ParseError::RunList`] if the list is malformed or points before the volume start.
pub fn parse_runlist(b: &[u8]) -> Result<Vec<Run>, ParseError> {
    let mut runs = Vec::new();
    let mut pos = 0usize;
    let mut lcn: i64 = 0;
    loop {
        let header = *b.get(pos).ok_or(ParseError::RunList)?;
        if header == 0 {
            break;
        }
        let len_size = usize::from(header & 0x0F);
        let off_size = usize::from(header >> 4);
        if len_size == 0 || len_size > 8 || off_size > 8 {
            return Err(ParseError::RunList);
        }
        let len_bytes = b
            .get(pos + 1..pos + 1 + len_size)
            .ok_or(ParseError::RunList)?;
        let mut clusters: u64 = 0;
        for (i, byte) in len_bytes.iter().enumerate() {
            clusters |= u64::from(*byte) << (8 * i);
        }
        let off_bytes = b
            .get(pos + 1 + len_size..pos + 1 + len_size + off_size)
            .ok_or(ParseError::RunList)?;
        let run_lcn = if off_size == 0 {
            None
        } else {
            let mut delta: i64 = 0;
            for (i, byte) in off_bytes.iter().enumerate() {
                delta |= i64::from(*byte) << (8 * i);
            }
            // Sign-extend from the highest byte present.
            let shift = 64 - 8 * off_size;
            delta = (delta << shift) >> shift;
            lcn = lcn.checked_add(delta).ok_or(ParseError::RunList)?;
            Some(u64::try_from(lcn).map_err(|_| ParseError::RunList)?)
        };
        runs.push(Run {
            lcn: run_lcn,
            clusters,
        });
        pos += 1 + len_size + off_size;
    }
    Ok(runs)
}

/// The multi-sector protection of NTFS: the last two bytes of every 512-byte stride hold the
/// update sequence number; the original bytes are kept in the update sequence array.
/// Restores them in place.
///
/// # Errors
/// [`ParseError::Fixup`] if a stride does not end with the update sequence number.
pub fn apply_fixups(record: &mut [u8]) -> Result<(), ParseError> {
    const STRIDE: usize = 512;
    let usa_offset = usize::from(u16_at(record, 4)?);
    let usa_count = usize::from(u16_at(record, 6)?);
    if usa_count == 0 {
        return Ok(());
    }
    let usn = u16_at(record, usa_offset)?;
    for i in 1..usa_count {
        let end = i * STRIDE;
        if end > record.len() {
            break;
        }
        if u16_at(record, end - 2)? != usn {
            return Err(ParseError::Fixup(i - 1));
        }
        let saved = u16_at(record, usa_offset + 2 * i)?;
        record[end - 2..end].copy_from_slice(&saved.to_le_bytes());
    }
    Ok(())
}

/// Times and attributes from `$STANDARD_INFORMATION`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StandardInfo {
    pub created: u64,
    pub modified: u64,
    pub attrs: u32,
}

/// One `$FILE_NAME` attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileName {
    /// File reference of the parent folder (record number and sequence).
    pub parent: u64,
    pub namespace: u8,
    pub name: Vec<u16>,
}

/// `$FILE_NAME` namespaces (only DOS-only names are treated specially: they are skipped).
#[cfg(test)]
pub const NAMESPACE_POSIX: u8 = 0;
#[cfg(test)]
pub const NAMESPACE_WIN32: u8 = 1;
pub const NAMESPACE_DOS: u8 = 2;
#[cfg(test)]
pub const NAMESPACE_WIN32_AND_DOS: u8 = 3;

/// What the enumeration needs from one MFT file record (base or extension).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedRecord {
    pub sequence: u16,
    /// File reference (record number and sequence) of the base record, for extension
    /// records; `None` for base records.
    pub base: Option<u64>,
    pub is_dir: bool,
    pub standard: Option<StandardInfo>,
    pub names: Vec<FileName>,
    /// Logical size of the unnamed data stream (first extent only carries it).
    pub data_size: Option<u64>,
    /// Allocated bytes of all data streams (unnamed and alternate) found in this record.
    pub allocated: u64,
    pub has_attribute_list: bool,
}

const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;

/// Mask applied to a file record number taken from a file reference.
pub const RECORD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Parses one file record. Applies the fixups in place. Returns `None` for records that are
/// not in use or not initialized.
///
/// # Errors
/// [`ParseError`] for damaged records (the caller skips them).
pub fn parse_record(record: &mut [u8]) -> Result<Option<ParsedRecord>, ParseError> {
    if record.get(0..4) != Some(b"FILE".as_slice()) {
        return Ok(None); // free (zeroed) or "BAAD" record
    }
    let flags = u16_at(record, 22)?;
    if flags & 0x0001 == 0 {
        return Ok(None);
    }
    apply_fixups(record)?;
    let base_ref = u64_at(record, 32)?;
    let mut out = ParsedRecord {
        sequence: u16_at(record, 16)?,
        base: (base_ref & RECORD_MASK != 0).then_some(base_ref),
        is_dir: flags & 0x0002 != 0,
        ..ParsedRecord::default()
    };
    let used = usize::try_from(u32_at(record, 24)?).map_err(|_| ParseError::Truncated(24))?;
    let end = used.min(record.len());
    let mut off = usize::from(u16_at(record, 20)?);
    while off + 8 <= end {
        let kind = u32_at(record, off)?;
        if kind == ATTR_END {
            break;
        }
        let len =
            usize::try_from(u32_at(record, off + 4)?).map_err(|_| ParseError::Truncated(off))?;
        if len < 16 || off + len > end {
            return Err(ParseError::Truncated(off));
        }
        let attr = &record[off..off + len];
        let non_resident = u8_at(attr, 8)? != 0;
        let name_len = u8_at(attr, 9)?;
        match kind {
            ATTR_STANDARD_INFORMATION if !non_resident => {
                let v = resident_value(attr)?;
                out.standard = Some(StandardInfo {
                    created: u64_at(v, 0)?,
                    modified: u64_at(v, 8)?,
                    attrs: u32_at(v, 32)?,
                });
            }
            ATTR_ATTRIBUTE_LIST => out.has_attribute_list = true,
            ATTR_FILE_NAME if !non_resident => {
                let v = resident_value(attr)?;
                let chars = usize::from(u8_at(v, 64)?);
                let bytes = v.get(66..66 + 2 * chars).ok_or(ParseError::Truncated(66))?;
                out.names.push(FileName {
                    parent: u64_at(v, 0)?,
                    namespace: u8_at(v, 65)?,
                    name: bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect(),
                });
            }
            ATTR_DATA => {
                if non_resident {
                    // Sizes are only valid in the extent that starts at VCN 0.
                    if u64_at(attr, 16)? == 0 {
                        let attr_flags = u16_at(attr, 12)?;
                        let compressed_or_sparse = attr_flags & (0x0001 | 0x8000) != 0;
                        let allocated = if compressed_or_sparse && len >= 72 {
                            u64_at(attr, 64)?
                        } else {
                            u64_at(attr, 40)?
                        };
                        out.allocated = out.allocated.saturating_add(allocated);
                        if name_len == 0 {
                            out.data_size = Some(u64_at(attr, 48)?);
                        }
                    }
                } else if name_len == 0 {
                    // Resident data lives inside the record: no clusters allocated.
                    out.data_size = Some(u64::from(u32_at(attr, 16)?));
                }
            }
            _ => {}
        }
        off += len;
    }
    Ok(Some(out))
}

fn resident_value(attr: &[u8]) -> Result<&[u8], ParseError> {
    let len = usize::try_from(u32_at(attr, 16)?).map_err(|_| ParseError::Truncated(16))?;
    let off = usize::from(u16_at(attr, 20)?);
    attr.get(off..off + len).ok_or(ParseError::Truncated(off))
}

/// The run list and data size of the unnamed `$DATA` attribute of a record (used for record
/// 0, the `$MFT` itself). Applies the fixups in place.
///
/// # Errors
/// [`ParseError`] if the record is damaged or has no non-resident data.
pub fn data_runs(record: &mut [u8]) -> Result<(Vec<Run>, u64), ParseError> {
    if record.get(0..4) != Some(b"FILE".as_slice()) {
        return Err(ParseError::Truncated(0));
    }
    apply_fixups(record)?;
    let used = usize::try_from(u32_at(record, 24)?).map_err(|_| ParseError::Truncated(24))?;
    let end = used.min(record.len());
    let mut off = usize::from(u16_at(record, 20)?);
    while off + 8 <= end {
        let kind = u32_at(record, off)?;
        if kind == ATTR_END {
            break;
        }
        let len =
            usize::try_from(u32_at(record, off + 4)?).map_err(|_| ParseError::Truncated(off))?;
        if len < 16 || off + len > end {
            return Err(ParseError::Truncated(off));
        }
        let attr = &record[off..off + len];
        if kind == ATTR_DATA && u8_at(attr, 8)? != 0 && u8_at(attr, 9)? == 0 {
            let runs_off = usize::from(u16_at(attr, 32)?);
            let runs = parse_runlist(attr.get(runs_off..).ok_or(ParseError::RunList)?)?;
            return Ok((runs, u64_at(attr, 48)?));
        }
        off += len;
    }
    Err(ParseError::RunList)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds file records for tests, laid out as NTFS does, with the fixups applied.
    pub(crate) struct RecordBuilder {
        attrs: Vec<u8>,
        flags: u16,
        base: u64,
        sequence: u16,
    }

    impl RecordBuilder {
        pub(crate) fn new(is_dir: bool) -> Self {
            Self {
                attrs: Vec::new(),
                flags: if is_dir { 0x03 } else { 0x01 },
                base: 0,
                sequence: 7,
            }
        }

        pub(crate) fn extension_of(mut self, base: u64) -> Self {
            self.base = base;
            self
        }

        fn push_attr(&mut self, kind: u32, name: &[u16], body: &[u8]) {
            // Resident attribute: 24-byte header, name, value aligned to 8 bytes.
            let mut a = Vec::new();
            a.extend_from_slice(&kind.to_le_bytes());
            a.extend_from_slice(&0u32.to_le_bytes()); // length, patched below
            a.push(0);
            a.push(u8::try_from(name.len()).unwrap());
            a.extend_from_slice(&24u16.to_le_bytes()); // name offset
            a.extend_from_slice(&0u16.to_le_bytes()); // flags
            a.extend_from_slice(&0u16.to_le_bytes()); // id
            let value_off = (24 + 2 * name.len()).div_ceil(8) * 8;
            a.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
            a.extend_from_slice(&u16::try_from(value_off).unwrap().to_le_bytes());
            a.extend_from_slice(&0u16.to_le_bytes());
            for c in name {
                a.extend_from_slice(&c.to_le_bytes());
            }
            a.resize(value_off, 0);
            a.extend_from_slice(body);
            let total = a.len().div_ceil(8) * 8;
            a.resize(total, 0);
            a[4..8].copy_from_slice(&u32::try_from(total).unwrap().to_le_bytes());
            self.attrs.extend_from_slice(&a);
        }

        pub(crate) fn standard(mut self, created: u64, modified: u64, attrs: u32) -> Self {
            let mut v = vec![0u8; 48];
            v[0..8].copy_from_slice(&created.to_le_bytes());
            v[8..16].copy_from_slice(&modified.to_le_bytes());
            v[32..36].copy_from_slice(&attrs.to_le_bytes());
            self.push_attr(ATTR_STANDARD_INFORMATION, &[], &v);
            self
        }

        pub(crate) fn name(mut self, parent: u64, namespace: u8, name: &str) -> Self {
            let units: Vec<u16> = name.encode_utf16().collect();
            let mut v = vec![0u8; 66];
            v[0..8].copy_from_slice(&parent.to_le_bytes());
            v[64] = u8::try_from(units.len()).unwrap();
            v[65] = namespace;
            for u in &units {
                v.extend_from_slice(&u.to_le_bytes());
            }
            self.push_attr(ATTR_FILE_NAME, &[], &v);
            self
        }

        pub(crate) fn resident_data(mut self, stream: &str, len: usize) -> Self {
            let name: Vec<u16> = stream.encode_utf16().collect();
            self.push_attr(ATTR_DATA, &name, &vec![0xAA; len]);
            self
        }

        /// Non-resident data: `size` bytes, `allocated` bytes, one run at `lcn` of `clusters`.
        pub(crate) fn nonresident_data(
            mut self,
            stream: &str,
            size: u64,
            allocated: u64,
            runs: &[u8],
        ) -> Self {
            let name: Vec<u16> = stream.encode_utf16().collect();
            let mut h = vec![0u8; 64];
            h[16..24].copy_from_slice(&0u64.to_le_bytes()); // start VCN
            let runs_off = 64 + 2 * name.len();
            let runs_off = runs_off.div_ceil(8) * 8;
            h[32..34].copy_from_slice(&u16::try_from(runs_off).unwrap().to_le_bytes());
            h[40..48].copy_from_slice(&allocated.to_le_bytes());
            h[48..56].copy_from_slice(&size.to_le_bytes());
            h[56..64].copy_from_slice(&size.to_le_bytes());
            // The name follows the 64-byte header; the run list follows, aligned.
            let mut tail = vec![0u8; runs_off - 64 - 2 * name.len()];
            tail.extend_from_slice(runs);
            self.push_attr_nonresident(&h, &name, &tail);
            self
        }

        fn push_attr_nonresident(&mut self, header: &[u8], name: &[u16], tail: &[u8]) {
            let mut a = Vec::new();
            a.extend_from_slice(&ATTR_DATA.to_le_bytes());
            a.extend_from_slice(&0u32.to_le_bytes());
            a.push(1);
            a.push(u8::try_from(name.len()).unwrap());
            a.extend_from_slice(&64u16.to_le_bytes());
            a.extend_from_slice(&0u16.to_le_bytes());
            a.extend_from_slice(&0u16.to_le_bytes());
            a.extend_from_slice(&header[16..64]);
            for c in name {
                a.extend_from_slice(&c.to_le_bytes());
            }
            a.extend_from_slice(tail);
            let total = a.len().div_ceil(8) * 8;
            a.resize(total, 0);
            a[4..8].copy_from_slice(&u32::try_from(total).unwrap().to_le_bytes());
            self.attrs.extend_from_slice(&a);
        }

        pub(crate) fn attribute_list(mut self) -> Self {
            self.push_attr(ATTR_ATTRIBUTE_LIST, &[], &[0u8; 32]);
            self
        }

        /// The 1024-byte record as stored on disk (fixups applied).
        pub(crate) fn build(self) -> Vec<u8> {
            let mut r = vec![0u8; 1024];
            r[0..4].copy_from_slice(b"FILE");
            r[4..6].copy_from_slice(&48u16.to_le_bytes()); // USA offset
            r[6..8].copy_from_slice(&3u16.to_le_bytes()); // USA count (1 + 2 strides)
            r[16..18].copy_from_slice(&self.sequence.to_le_bytes());
            r[20..22].copy_from_slice(&56u16.to_le_bytes()); // first attribute
            r[22..24].copy_from_slice(&self.flags.to_le_bytes());
            r[32..40].copy_from_slice(&self.base.to_le_bytes());
            let mut off = 56;
            r[off..off + self.attrs.len()].copy_from_slice(&self.attrs);
            off += self.attrs.len();
            r[off..off + 4].copy_from_slice(&ATTR_END.to_le_bytes());
            let used = off + 8;
            r[24..28].copy_from_slice(&u32::try_from(used).unwrap().to_le_bytes());
            r[28..32].copy_from_slice(&1024u32.to_le_bytes());
            // Update sequence: USN 0x0042; save the real bytes and stamp the stride ends.
            let usn: u16 = 0x0042;
            r[48..50].copy_from_slice(&usn.to_le_bytes());
            for i in 1..3 {
                let end = i * 512;
                let saved = [r[end - 2], r[end - 1]];
                r[48 + 2 * i..48 + 2 * i + 2].copy_from_slice(&saved);
                r[end - 2..end].copy_from_slice(&usn.to_le_bytes());
            }
            r
        }
    }

    #[test]
    fn parses_a_file_record() {
        let mut r = RecordBuilder::new(false)
            .standard(100, 200, 0x20)
            .name(5 | (5 << 48), NAMESPACE_WIN32, "report.pdf")
            .name(5 | (5 << 48), NAMESPACE_DOS, "REPORT~1.PDF")
            .nonresident_data("", 10_000, 12_288, &[0x11, 0x03, 0x10, 0x00])
            .resident_data("Zone.Identifier", 26)
            .build();
        let p = parse_record(&mut r).unwrap().unwrap();
        assert!(!p.is_dir);
        assert_eq!(p.base, None);
        assert_eq!(p.sequence, 7);
        assert_eq!(
            p.standard,
            Some(StandardInfo {
                created: 100,
                modified: 200,
                attrs: 0x20
            })
        );
        assert_eq!(p.names.len(), 2);
        assert_eq!(String::from_utf16_lossy(&p.names[0].name), "report.pdf");
        assert_eq!(p.names[1].namespace, NAMESPACE_DOS);
        assert_eq!(p.data_size, Some(10_000));
        // The alternate stream is resident: it adds nothing to the allocation.
        assert_eq!(p.allocated, 12_288);
    }

    #[test]
    fn resident_data_has_no_allocation() {
        let mut r = RecordBuilder::new(false)
            .standard(1, 2, 0)
            .name(5, NAMESPACE_WIN32_AND_DOS, "tiny.txt")
            .resident_data("", 42)
            .build();
        let p = parse_record(&mut r).unwrap().unwrap();
        assert_eq!(p.data_size, Some(42));
        assert_eq!(p.allocated, 0);
    }

    #[test]
    fn extension_records_point_to_their_base() {
        let mut r = RecordBuilder::new(false)
            .extension_of(1234 | (9 << 48))
            .name(5, NAMESPACE_POSIX, "second link")
            .build();
        let p = parse_record(&mut r).unwrap().unwrap();
        assert_eq!(p.base, Some(1234 | (9 << 48)));
        assert!(p.standard.is_none());
        let mut with_list = RecordBuilder::new(true).attribute_list().build();
        assert!(
            parse_record(&mut with_list)
                .unwrap()
                .unwrap()
                .has_attribute_list
        );
    }

    #[test]
    fn free_and_damaged_records() {
        let mut zero = vec![0u8; 1024];
        assert_eq!(parse_record(&mut zero).unwrap(), None);
        let mut free = RecordBuilder::new(false).build();
        free[22] = 0; // not in use
        assert_eq!(parse_record(&mut free).unwrap(), None);
        let mut torn = RecordBuilder::new(false).standard(1, 2, 3).build();
        torn[1022] ^= 0xFF; // the second stride lost its update sequence number
        assert_eq!(parse_record(&mut torn), Err(ParseError::Fixup(1)));
        let mut truncated = RecordBuilder::new(false).standard(1, 2, 3).build();
        truncated[60..64].copy_from_slice(&5000u32.to_le_bytes()); // attribute too long
        assert!(parse_record(&mut truncated).is_err());
    }

    #[test]
    fn fixups_restore_the_original_bytes() {
        let mut r = RecordBuilder::new(false)
            .standard(1, 2, 3)
            .resident_data("", 800) // spans the first stride end
            .build();
        assert_eq!(&r[510..512], &0x0042u16.to_le_bytes());
        apply_fixups(&mut r).unwrap();
        assert_eq!(&r[510..512], &[0xAA, 0xAA]);
    }

    #[test]
    fn run_lists() {
        // 0x21: 1-byte length, 2-byte offset. Run 1: 0x10 clusters at 0x0400.
        // Run 2: 0x08 clusters at 0x0400 - 0x0100 (negative delta). Run 3: sparse 4 clusters.
        let runs = parse_runlist(&[
            0x21, 0x10, 0x00, 0x04, 0x21, 0x08, 0x00, 0xFF, 0x01, 0x04, 0x00,
        ])
        .unwrap();
        assert_eq!(
            runs,
            vec![
                Run {
                    lcn: Some(0x400),
                    clusters: 0x10
                },
                Run {
                    lcn: Some(0x300),
                    clusters: 8
                },
                Run {
                    lcn: None,
                    clusters: 4
                },
            ]
        );
        assert_eq!(parse_runlist(&[0x21, 0x10]), Err(ParseError::RunList));
        assert_eq!(
            parse_runlist(&[0x11, 0x01, 0x80, 0x00]),
            Err(ParseError::RunList)
        );
    }

    #[test]
    fn mft_data_runs_from_record_zero() {
        let mut r = RecordBuilder::new(false)
            .standard(1, 2, 6)
            .name(5, NAMESPACE_WIN32_AND_DOS, "$MFT")
            .nonresident_data("", 4096 * 100, 4096 * 100, &[0x11, 0x64, 0x20, 0x00])
            .build();
        let (runs, size) = data_runs(&mut r).unwrap();
        assert_eq!(size, 409_600);
        assert_eq!(
            runs,
            vec![Run {
                lcn: Some(0x20),
                clusters: 100
            }]
        );
    }

    #[test]
    fn boot_sector_geometry() {
        let mut b = vec![0u8; 512];
        b[3..11].copy_from_slice(b"NTFS    ");
        b[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
        b[0x0D] = 8;
        b[0x30..0x38].copy_from_slice(&786_432u64.to_le_bytes());
        b[0x40] = 0xF6; // -10 → 1024-byte records
        let g = parse_boot_sector(&b).unwrap();
        assert_eq!(g.bytes_per_cluster, 4096);
        assert_eq!(g.record_size, 1024);
        assert_eq!(g.mft_lcn, 786_432);
        b[3] = b'X';
        assert_eq!(parse_boot_sector(&b), Err(ParseError::NotNtfs));
    }
}
