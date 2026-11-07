//! `MftEnumerator`: enumerates an NTFS volume by reading its Master File Table directly
//! (ADR-0006, ADR-0017). Requires administrator rights (raw volume access).
//!
//! 1. Read the boot sector of `\\.\X:` for the geometry, then record 0 (`$MFT` itself) for the
//!    run list of the MFT (it may be fragmented).
//! 2. Read the MFT sequentially in large chunks and parse every file record. Extension
//!    records (attributes that did not fit in the base record, e.g. many hard links) are merged
//!    into their base record by reference, so `$ATTRIBUTE_LIST` itself never needs decoding.
//! 3. Build the folder tree from the `$FILE_NAME` attributes (parent reference + name), apply
//!    the exclusion policy while walking it from the requested folder, and produce a
//!    [`ScanTree`] — the same structure the walker produces.
//!
//! Semantics, chosen to match the walker where both can see the same thing:
//! - DOS 8.3 names are skipped; every other name is an entry, so a file with several hard
//!   links appears once per name. Its allocated space is counted on one name only (the
//!   smallest parent/name pair), the other names report 0, so used space is not counted twice.
//! - The NTFS metadata files (records below 24: `$MFT`, `$Extend`...) are hidden, as in a
//!   directory listing.
//! - `size` is the length of the unnamed data stream; `alloc_size` adds the clusters of the
//!   alternate data streams (they occupy the disk too); resident data allocates nothing.
//! - Times and attributes come from `$STANDARD_INFORMATION`.
//!
//! The volume is read through the device, not through the file-system cache: changes that
//! are still only in the cache appear later, through the USN journal (ADR-0007).

mod parse;

use std::collections::VecDeque;
use std::fs::File;
use std::os::windows::fs::FileExt;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Instant;

use prism_core::exclusion::{ChildCheck, RuleCursor};
use prism_core::model::{ATTR_DIRECTORY, EntryMeta, FileTime};
use prism_core::names;
use prism_core::scan::{
    Enumerator, NodeId, ScanChild, ScanError, ScanRequest, ScanStats, ScanTree, ScanTreeBuilder,
};

pub use parse::{BootInfo, ParseError};
use parse::{NAMESPACE_DOS, ParsedRecord, RECORD_MASK};

/// Record number of the root folder.
const ROOT_RECORD: u64 = 5;
/// Records below this number are NTFS metadata files (or reserved).
const FIRST_USER_RECORD: u64 = 24;
/// Bytes read from the MFT per request.
const CHUNK: u64 = 4 << 20;
/// Bits of `$STANDARD_INFORMATION` flags that are not Win32 attributes.
const NTFS_INTERNAL_FLAGS: u32 = 0x1000_0000 | 0x2000_0000;
const ATTR_NORMAL: u32 = 0x80;

/// Reads NTFS volumes through their MFT.
#[derive(Clone, Copy, Debug, Default)]
pub struct MftEnumerator;

/// Every file record of the volume after merging extension records, stored compactly
/// (columns instead of one struct per record, names in a single buffer): about 8 bytes per
/// MFT slot, 48 per record in use and 24 plus the UTF-16 text per name, instead of ~90 bytes
/// per slot with one struct and one allocation per name.
struct Table {
    /// Per record number: sequence number (low 16 bits) and the flags below.
    state: Vec<u32>,
    /// Per record number: index into `details`, or `NONE`.
    slot: Vec<u32>,
    details: Vec<Detail>,
    names: Vec<NameRef>,
    /// UTF-16 code units of every name.
    arena: Vec<u16>,
}

const IN_USE: u32 = 1 << 16;
const IS_DIR: u32 = 1 << 17;
const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Detail {
    created: u64,
    modified: u64,
    size: u64,
    allocated: u64,
    attrs: u32,
    /// First name of the record (linked through `NameRef::next`), or `NONE`.
    first_name: u32,
}

#[derive(Clone, Copy)]
struct NameRef {
    /// Parent folder reference (record number and sequence).
    parent: u64,
    child: u32,
    next: u32,
    offset: u32,
    len: u16,
}

impl Table {
    fn new(records: usize) -> Self {
        Self {
            state: vec![0; records],
            slot: vec![NONE; records],
            details: Vec::new(),
            names: Vec::new(),
            arena: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        self.state.len()
    }

    fn in_use(&self, n: u64) -> bool {
        self.state_of(n) & IN_USE != 0
    }

    fn is_dir(&self, n: u64) -> bool {
        self.state_of(n) & IS_DIR != 0
    }

    fn sequence(&self, n: u64) -> u16 {
        u16::try_from(self.state_of(n) & 0xFFFF).unwrap_or(0)
    }

    fn state_of(&self, n: u64) -> u32 {
        usize::try_from(n)
            .ok()
            .and_then(|i| self.state.get(i))
            .copied()
            .unwrap_or(0)
    }

    fn detail(&self, n: u64) -> Detail {
        usize::try_from(n)
            .ok()
            .and_then(|i| self.slot.get(i))
            .and_then(|&s| self.details.get(s as usize))
            .copied()
            .unwrap_or(Detail {
                first_name: NONE,
                ..Detail::default()
            })
    }

    fn detail_mut(&mut self, n: usize) -> Option<&mut Detail> {
        let slot = self.slot.get_mut(n)?;
        if *slot == NONE {
            *slot = u32::try_from(self.details.len()).ok()?;
            self.details.push(Detail {
                first_name: NONE,
                ..Detail::default()
            });
        }
        self.details.get_mut(*slot as usize)
    }

    /// Indexes of the names of record `n`.
    fn names_of(&self, n: u64) -> impl Iterator<Item = u32> + '_ {
        let mut next = self.detail(n).first_name;
        std::iter::from_fn(move || {
            let current = next;
            let name = self.names.get(current as usize)?;
            next = name.next;
            Some(current)
        })
    }

    fn units(&self, name: u32) -> &[u16] {
        self.names.get(name as usize).map_or(&[], |r| {
            let start = r.offset as usize;
            self.arena
                .get(start..start + usize::from(r.len))
                .unwrap_or(&[])
        })
    }

    fn merge(&mut self, number: u64, p: ParsedRecord) {
        let (target, is_base) = match p.base {
            None => (number, true),
            Some(base_ref) => {
                let base = base_ref & RECORD_MASK;
                // An extension written for an older file in a reused record is ignored.
                let seq = u16::try_from(base_ref >> 48).unwrap_or(0);
                if self.in_use(base) && self.sequence(base) != seq {
                    return;
                }
                (base, false)
            }
        };
        let Ok(t) = usize::try_from(target) else {
            return;
        };
        if t >= self.state.len() {
            return;
        }
        if is_base {
            self.state[t] = u32::from(p.sequence) | IN_USE | if p.is_dir { IS_DIR } else { 0 };
        }
        let first_new_name = u32::try_from(self.names.len()).unwrap_or(NONE);
        let mut added = 0u32;
        for n in p.names.into_iter().filter(|n| n.namespace != NAMESPACE_DOS) {
            let (Ok(offset), Ok(len), Ok(child)) = (
                u32::try_from(self.arena.len()),
                u16::try_from(n.name.len()),
                u32::try_from(target),
            ) else {
                continue;
            };
            self.arena.extend_from_slice(&n.name);
            let index = first_new_name + added;
            self.names.push(NameRef {
                parent: n.parent,
                child,
                next: index + 1, // the last one is patched below
                offset,
                len,
            });
            added += 1;
        }
        let Some(d) = self.detail_mut(t) else {
            return;
        };
        if let Some(si) = p.standard {
            d.created = si.created;
            d.modified = si.modified;
            d.attrs = si.attrs;
        }
        if let Some(size) = p.data_size {
            d.size = size;
        }
        d.allocated = d.allocated.saturating_add(p.allocated);
        if added > 0 {
            // Prepend the new names to the record's list.
            let previous_first = d.first_name;
            d.first_name = first_new_name;
            let last = (first_new_name + added - 1) as usize;
            if let Some(name) = self.names.get_mut(last) {
                name.next = previous_first;
            }
        }
    }

    fn reference(&self, n: u64) -> u64 {
        n | (u64::from(self.sequence(n)) << 48)
    }

    fn meta(&self, n: u64, counts_allocation: bool) -> EntryMeta {
        entry_meta(
            self.is_dir(n),
            &self.detail(n),
            self.reference(n),
            counts_allocation,
        )
    }

    /// Whether name `name` of its record carries the allocation of a hard-linked file: the
    /// smallest (parent, name) pair does, the others report 0.
    fn is_primary_name(&self, name: u32) -> bool {
        let Some(me) = self.names.get(name as usize) else {
            return true;
        };
        let key = |i: u32| {
            (
                self.names
                    .get(i as usize)
                    .map_or(0, |r| r.parent & RECORD_MASK),
                self.units(i),
            )
        };
        let mine = key(name);
        self.names_of(u64::from(me.child))
            .all(|other| key(other) >= mine)
    }
}

/// The catalog metadata of a file record, shared by the full MFT read and by single-file
/// fetches (USN updates), so both produce exactly the same values (F05-AC3).
fn entry_meta(is_dir: bool, d: &Detail, reference: u64, counts_allocation: bool) -> EntryMeta {
    let mut attrs = d.attrs & !NTFS_INTERNAL_FLAGS;
    if is_dir {
        attrs |= ATTR_DIRECTORY;
    }
    if attrs == 0 {
        attrs = ATTR_NORMAL;
    }
    let time = |t: u64| (t != 0).then(|| FileTime(i64::try_from(t).unwrap_or(i64::MAX)));
    EntryMeta {
        size: if is_dir { 0 } else { d.size },
        alloc_size: (!is_dir).then_some(if counts_allocation { d.allocated } else { 0 }),
        created: time(d.created),
        modified: time(d.modified),
        attrs,
        file_ref: Some(reference),
    }
}

/// One file read through its own MFT records (base and extensions), as needed to apply a
/// change journal entry. Same semantics as the full MFT read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileState {
    /// File reference (record number and sequence).
    pub file_ref: u64,
    pub is_dir: bool,
    /// Every non-DOS name: (parent folder reference, UTF-16 name).
    pub names: Vec<(u64, Vec<u16>)>,
    detail: Detail,
}

impl FileState {
    fn from_parts(file_ref: u64, parts: Vec<ParsedRecord>) -> Self {
        let mut state = Self {
            file_ref,
            is_dir: false,
            names: Vec::new(),
            detail: Detail::default(),
        };
        for (i, p) in parts.into_iter().enumerate() {
            if i == 0 {
                state.is_dir = p.is_dir;
            }
            if let Some(si) = p.standard {
                state.detail.created = si.created;
                state.detail.modified = si.modified;
                state.detail.attrs = si.attrs;
            }
            if let Some(size) = p.data_size {
                state.detail.size = size;
            }
            state.detail.allocated = state.detail.allocated.saturating_add(p.allocated);
            state.names.extend(
                p.names
                    .into_iter()
                    .filter(|n| n.namespace != NAMESPACE_DOS)
                    .map(|n| (n.parent, n.name)),
            );
        }
        state
    }

    /// Catalog metadata for name `index` (only the primary name of a hard-linked file counts
    /// the allocated space, as in the full read).
    #[must_use]
    pub fn meta(&self, index: usize) -> EntryMeta {
        let key = |i: usize| {
            self.names
                .get(i)
                .map(|(p, n)| (p & RECORD_MASK, n.as_slice()))
        };
        let mine = key(index);
        let primary = self.is_dir || (0..self.names.len()).all(|o| key(o) >= mine);
        entry_meta(self.is_dir, &self.detail, self.file_ref, primary)
    }

    /// Stored form of name `index` (see `prism_core::names`).
    #[must_use]
    pub fn name(&self, index: usize) -> String {
        self.names
            .get(index)
            .map(|(_, n)| names::encode_wide(n))
            .unwrap_or_default()
    }
}

/// Errors when fetching a single file record.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("FSCTL_GET_NTFS_FILE_RECORD failed with Windows error {0}")]
    Io(u32),
    #[error("damaged file record: {0}")]
    Parse(#[from] ParseError),
    /// The non-resident attribute list read from the disk is not consistent yet (the file
    /// system has not written it back from its cache): try again later.
    #[error("the attribute list of file {0:#x} is not on the disk yet: retry later")]
    Stale(u64),
}

/// Reads a non-resident `$ATTRIBUTE_LIST` from the volume clusters. The clusters are read
/// from the disk, which can lag the file-system cache: a list that does not parse is
/// reported as an error, never used.
fn read_attribute_list(
    volume: &File,
    runs: &[parse::Run],
    size: u64,
) -> Result<Vec<u64>, ParseError> {
    let mut boot = vec![0u8; 4096];
    read_exact_at(volume, &mut boot, 0).map_err(|_| ParseError::Truncated(0))?;
    let geo = parse::parse_boot_sector(&boot)?;
    let cluster = geo.bytes_per_cluster;
    let mut data = Vec::new();
    for run in runs {
        let lcn = run.lcn.ok_or(ParseError::RunList)?;
        let len = usize::try_from(run.clusters * cluster).map_err(|_| ParseError::RunList)?;
        if len > 16 << 20 {
            return Err(ParseError::RunList);
        }
        let mut buf = vec![0u8; len];
        read_exact_at(volume, &mut buf, lcn * cluster).map_err(|_| ParseError::Truncated(0))?;
        data.extend_from_slice(&buf);
    }
    let size = usize::try_from(size).map_err(|_| ParseError::RunList)?;
    parse::attribute_list_holders(data.get(..size).ok_or(ParseError::Truncated(size))?)
}

/// Reads one file — its base record and the extension records listed in its attribute
/// list — through the file system (so cached, not yet written changes are seen).
///
/// Returns `None` if the file no longer exists (record free, reused by another file, or an
/// extension record).
///
/// # Errors
/// [`FetchError`] on I/O errors, damaged records, or a non-resident attribute list.
pub fn fetch_file(volume: &File, file_ref: u64) -> Result<Option<FileState>, FetchError> {
    let number = file_ref & RECORD_MASK;
    let sequence = u16::try_from(file_ref >> 48).unwrap_or(0);
    let Some(mut base) = fetch_record(volume, number)? else {
        return Ok(None);
    };
    let Some(parsed) = parse::parse_fetched_record(&mut base)? else {
        return Ok(None);
    };
    if parsed.base.is_some() || parsed.sequence != sequence {
        return Ok(None);
    }
    let holders = match &parsed.attribute_list_data {
        Some((runs, size)) => {
            read_attribute_list(volume, runs, *size).map_err(|_| FetchError::Stale(file_ref))?
        }
        None => parsed.extension_records.clone(),
    };
    let extensions: Vec<u64> = holders
        .into_iter()
        .filter(|r| r & RECORD_MASK != number)
        .collect();
    let mut parts = vec![parsed];
    for ext in extensions {
        let Some(mut raw) = fetch_record(volume, ext & RECORD_MASK)? else {
            continue;
        };
        if let Some(p) = parse::parse_fetched_record(&mut raw)?
            && p.base.map(|b| b & RECORD_MASK) == Some(number)
        {
            parts.push(p);
        }
    }
    Ok(Some(FileState::from_parts(file_ref, parts)))
}

/// One raw file record by number, or `None` if the record is not in use.
fn fetch_record(volume: &File, number: u64) -> Result<Option<Vec<u8>>, FetchError> {
    let input = i64::try_from(number).unwrap_or(i64::MAX).to_le_bytes();
    // Output: record number (8 bytes), record length (4), record (up to 64 KiB).
    let mut out = vec![0u64; (12 + 65_536) / 8 + 1];
    let n = crate::ioctl::device_io(
        volume,
        crate::ioctl::FSCTL_GET_NTFS_FILE_RECORD,
        &input,
        &mut out,
    )
    .map_err(FetchError::Io)?;
    let bytes: &[u8] = crate::ioctl::as_bytes(&out, n);
    if bytes.len() < 12 {
        return Err(FetchError::Parse(ParseError::Truncated(0)));
    }
    let returned = u64::from_le_bytes(bytes[0..8].try_into().unwrap_or([0; 8])) & RECORD_MASK;
    let len = u32::from_le_bytes(bytes[8..12].try_into().unwrap_or([0; 4])) as usize;
    // For a record not in use, Windows returns the closest preceding record in use.
    if returned != number {
        return Ok(None);
    }
    bytes
        .get(12..12 + len)
        .map(|r| Some(r.to_vec()))
        .ok_or(FetchError::Parse(ParseError::Truncated(12)))
}

fn unsupported(root: &Path, reason: impl Into<String>) -> ScanError {
    ScanError::Unsupported {
        path: root.to_path_buf(),
        reason: reason.into(),
    }
}

/// `\\.\X:` for a path on drive X (plain or verbatim), `\\?\Volume{...}` for a volume path.
fn device_path(root: &Path) -> Option<String> {
    let s = root.to_string_lossy();
    let rest = s.strip_prefix(r"\\?\").unwrap_or(&s);
    if rest.len() >= 2 && rest.as_bytes()[1] == b':' && rest.as_bytes()[0].is_ascii_alphabetic() {
        return Some(format!(r"\\.\{}:", &rest[..1]));
    }
    if rest.len() >= 8 && rest[..7].eq_ignore_ascii_case("Volume{") {
        let end = rest.find('}')?;
        return Some(format!(r"\\?\{}", &rest[..=end]));
    }
    None
}

fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    let mut done = 0usize;
    while done < buf.len() {
        let n = file.seek_read(&mut buf[done..], offset + done as u64)?;
        if n == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        }
        done += n;
    }
    Ok(())
}

/// Reads and merges every file record of the volume.
fn read_records(
    file: &File,
    root: &Path,
    request: &ScanRequest<'_>,
    stats: &mut ScanStats,
) -> Result<Table, ScanError> {
    let io = |e: std::io::Error| ScanError::RootUnreadable {
        path: root.to_path_buf(),
        source: e,
    };
    let mut boot = vec![0u8; 4096];
    read_exact_at(file, &mut boot, 0).map_err(io)?;
    let geo = parse::parse_boot_sector(&boot).map_err(|e| unsupported(root, e.to_string()))?;
    let record_size = u64::from(geo.record_size);
    let sector = u64::from(geo.bytes_per_sector);
    let cluster = geo.bytes_per_cluster;

    let first_len =
        usize::try_from(record_size.max(sector)).map_err(|_| unsupported(root, "geometry"))?;
    let mut first = vec![0u8; first_len];
    read_exact_at(file, &mut first, geo.mft_lcn * cluster).map_err(io)?;
    let rs = usize::try_from(record_size).map_err(|_| unsupported(root, "geometry"))?;
    let (runs, mft_size) = parse::data_runs(&mut first[..rs])
        .map_err(|e| unsupported(root, format!("cannot read the $MFT record: {e}")))?;
    let total =
        usize::try_from(mft_size / record_size).map_err(|_| unsupported(root, "MFT size"))?;
    let mut table = Table::new(total);

    let mut buf = vec![0u8; usize::try_from(CHUNK).unwrap_or(4 << 20)];
    let mut mft_offset: u64 = 0; // bytes of the MFT already processed
    for run in runs {
        let lcn = run
            .lcn
            .ok_or_else(|| unsupported(root, "sparse run in the $MFT"))?;
        let mut disk_pos = lcn * cluster;
        let mut remaining = (run.clusters * cluster).min(mft_size.saturating_sub(mft_offset));
        while remaining > 0 {
            request.cancel.check()?;
            // Whole sectors and whole records.
            let len = remaining.min(CHUNK).div_ceil(sector) * sector;
            let len_usize = usize::try_from(len).map_err(|_| unsupported(root, "chunk"))?;
            read_exact_at(file, &mut buf[..len_usize], disk_pos).map_err(io)?;
            let usable = usize::try_from(remaining.min(len)).unwrap_or(0) / rs * rs;
            for (i, raw) in buf[..usable].chunks_exact_mut(rs).enumerate() {
                let number = (mft_offset + (i * rs) as u64) / record_size;
                match parse::parse_record(raw) {
                    Ok(Some(parsed)) => table.merge(number, parsed),
                    Ok(None) => {}
                    Err(e) => stats.push_error(format!("MFT record {number}: {e}")),
                }
            }
            request
                .progress
                .fetch_add((usable / rs) as u64, Ordering::Relaxed);
            mft_offset += usable as u64;
            disk_pos += len;
            remaining = remaining.saturating_sub(len);
            if usable == 0 {
                break;
            }
        }
    }
    Ok(table)
}

/// Names grouped by parent folder (indexes into `Table::names`), for every name of a user
/// record whose parent is an existing folder with the same sequence number.
struct Children {
    by_parent: Vec<u32>,
    /// (parent record, first index into `by_parent`), sorted.
    starts: Vec<(u64, usize)>,
}

impl Children {
    fn build(t: &Table) -> Self {
        let mut by_parent: Vec<u32> = Vec::with_capacity(t.names.len());
        for (i, name) in t.names.iter().enumerate() {
            let child = u64::from(name.child);
            if child < FIRST_USER_RECORD || !t.in_use(child) {
                continue;
            }
            let parent = name.parent & RECORD_MASK;
            let parent_seq = u16::try_from(name.parent >> 48).unwrap_or(0);
            let valid = parent != child
                && t.in_use(parent)
                && t.is_dir(parent)
                && t.sequence(parent) == parent_seq;
            if let (true, Ok(i)) = (valid, u32::try_from(i)) {
                by_parent.push(i);
            }
        }
        by_parent.sort_unstable_by_key(|&i| (t.names[i as usize].parent & RECORD_MASK, i));
        let mut starts = Vec::new();
        for (pos, &i) in by_parent.iter().enumerate() {
            let parent = t.names[i as usize].parent & RECORD_MASK;
            if starts.last().is_none_or(|(p, _)| *p != parent) {
                starts.push((parent, pos));
            }
        }
        Self { by_parent, starts }
    }

    fn of(&self, parent: u64) -> &[u32] {
        let Ok(k) = self.starts.binary_search_by_key(&parent, |s| s.0) else {
            return &[];
        };
        let start = self.starts[k].1;
        let end = self.starts.get(k + 1).map_or(self.by_parent.len(), |s| s.1);
        &self.by_parent[start..end]
    }
}

fn build_tree(
    t: &Table,
    request: &ScanRequest<'_>,
    mut stats: ScanStats,
) -> Result<ScanTree, ScanError> {
    let children = Children::build(t);
    // Find the requested folder below the root, component by component.
    let mut start = ROOT_RECORD;
    for component in request.relative {
        let found = children.of(start).iter().find(|&&i| {
            let child = u64::from(t.names[i as usize].child);
            t.is_dir(child) && names::eq_ignore_case(&names::encode_wide(t.units(i)), component)
        });
        start = found
            .map(|&i| u64::from(t.names[i as usize].child))
            .ok_or_else(|| ScanError::RootNotADirectory(request.root.to_path_buf()))?;
    }
    let cursor = request
        .policy
        .cursor_for(request.relative)
        .ok_or_else(|| ScanError::RootExcluded(request.root.to_path_buf()))?;

    let mut builder = ScanTreeBuilder::new(t.meta(start, true));
    stats.dirs = 1;
    let mut queue: VecDeque<(NodeId, u64, RuleCursor)> = VecDeque::new();
    queue.push_back((ScanTree::ROOT, start, cursor));
    while let Some((node, dir, cursor)) = queue.pop_front() {
        request.cancel.check()?;
        let mut found = Vec::new();
        let mut descend = Vec::new();
        for &i in children.of(dir) {
            let child = u64::from(t.names[i as usize].child);
            let is_dir = t.is_dir(child);
            let name = names::encode_wide(t.units(i));
            match request.policy.child(&cursor, &name, is_dir) {
                ChildCheck::Excluded => stats.excluded += 1,
                ChildCheck::Included(next) => {
                    if is_dir {
                        stats.dirs += 1;
                    } else {
                        stats.files += 1;
                    }
                    descend.push(is_dir.then_some((child, next)));
                    found.push(ScanChild {
                        name,
                        is_dir,
                        meta: t.meta(child, is_dir || t.is_primary_name(i)),
                    });
                }
            }
        }
        let range = builder.add_listing(node, found);
        for (id, d) in range.zip(descend) {
            if let Some((child, next)) = d {
                queue.push_back((id, child, next));
            }
        }
    }
    Ok(builder.finish(stats))
}

impl Enumerator for MftEnumerator {
    fn scan(&self, request: &ScanRequest<'_>) -> Result<ScanTree, ScanError> {
        let started = Instant::now();
        let root = request.root;
        let device = device_path(root)
            .ok_or_else(|| unsupported(root, "not a drive letter or volume path"))?;
        let file = File::open(&device).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                unsupported(root, "reading the MFT requires administrator rights")
            } else {
                ScanError::RootUnreadable {
                    path: root.to_path_buf(),
                    source: e,
                }
            }
        })?;
        let mut stats = ScanStats::default();
        let table = read_records(&file, root, request, &mut stats)?;
        let read_done = started.elapsed();
        let mut tree = build_tree(&table, request, stats)?;
        tree.stats.elapsed = started.elapsed();
        tracing::info!(
            records = table.len(),
            names = table.names.len(),
            entries = tree.len(),
            read_and_parse_ms = read_done.as_millis(),
            build_ms = (tree.stats.elapsed - read_done).as_millis(),
            "MFT enumeration finished"
        );
        Ok(tree)
    }
}

#[cfg(test)]
mod tests {
    use super::parse::tests::RecordBuilder;
    use super::parse::{NAMESPACE_POSIX, NAMESPACE_WIN32, NAMESPACE_WIN32_AND_DOS};
    use super::*;
    use prism_core::cancel::CancelToken;
    use prism_core::exclusion::ExclusionPolicy;
    use std::sync::atomic::AtomicU64;

    fn parsed(mut raw: Vec<u8>) -> ParsedRecord {
        parse::parse_record(&mut raw).unwrap().unwrap()
    }

    /// A small volume: root(5) with a folder, a file with two hard links, a metadata file,
    /// a file whose names spill into an extension record, and the recycle bin.
    fn volume() -> Table {
        let mut records = Table::new(40);
        let root_ref = 5 | (7 << 48); // the builder gives every record sequence 7
        let put = |records: &mut Table, n: u64, raw: Vec<u8>| records.merge(n, parsed(raw));
        put(
            &mut records,
            5,
            RecordBuilder::new(true)
                .standard(1, 2, 0x16)
                .name(root_ref, NAMESPACE_WIN32_AND_DOS, ".")
                .build(),
        );
        put(
            &mut records,
            0,
            RecordBuilder::new(false)
                .standard(1, 2, 6)
                .name(root_ref, NAMESPACE_WIN32_AND_DOS, "$MFT")
                .build(),
        );
        let docs_ref = 30 | (7 << 48);
        put(
            &mut records,
            30,
            RecordBuilder::new(true)
                .standard(10, 20, 0)
                .name(root_ref, NAMESPACE_WIN32, "Docs")
                .name(root_ref, NAMESPACE_DOS, "DOCS")
                .build(),
        );
        put(
            &mut records,
            31,
            RecordBuilder::new(false)
                .standard(10, 30, 0x20)
                .name(docs_ref, NAMESPACE_WIN32_AND_DOS, "a.txt")
                .name(root_ref, NAMESPACE_POSIX, "link-to-a.txt")
                .nonresident_data("", 5000, 8192, &[0x11, 0x02, 0x40, 0x00])
                .build(),
        );
        // Extension record 33 carries the data and a second name of base record 32.
        put(
            &mut records,
            33,
            RecordBuilder::new(false)
                .extension_of(32 | (7 << 48))
                .name(docs_ref, NAMESPACE_WIN32, "big-ext.bin")
                .nonresident_data("", 1 << 20, 1 << 20, &[0x11, 0x10, 0x50, 0x00])
                .build(),
        );
        put(
            &mut records,
            32,
            RecordBuilder::new(false)
                .standard(10, 40, 0x20)
                .attribute_list(&[32 | (7 << 48), 33 | (7 << 48)])
                .name(docs_ref, NAMESPACE_WIN32, "big.bin")
                .build(),
        );
        put(
            &mut records,
            34,
            RecordBuilder::new(true)
                .standard(1, 1, 0x16)
                .name(root_ref, NAMESPACE_WIN32, "$RECYCLE.BIN")
                .build(),
        );
        records
    }

    fn tree(records: &Table, relative: &[String], policy: &ExclusionPolicy) -> ScanTree {
        build_tree(
            records,
            &ScanRequest {
                root: Path::new(r"X:\"),
                relative,
                policy,
                cancel: &CancelToken::new(),
                progress: &AtomicU64::new(0),
            },
            ScanStats::default(),
        )
        .unwrap()
    }

    fn by_path(t: &ScanTree) -> std::collections::BTreeMap<String, (bool, EntryMeta)> {
        let mut out = std::collections::BTreeMap::new();
        let mut stack = vec![(ScanTree::ROOT, String::new())];
        while let Some((n, path)) = stack.pop() {
            for c in t.children(n) {
                let node = t.node(c);
                let p = if path.is_empty() {
                    node.name.to_string()
                } else {
                    format!("{path}\\{}", node.name)
                };
                out.insert(p.clone(), (node.is_dir, node.meta));
                stack.push((c, p));
            }
        }
        out
    }

    #[test]
    fn builds_the_tree_with_links_extensions_and_exclusions() {
        let records = volume();
        let t = tree(&records, &[], &ExclusionPolicy::with_defaults());
        let all = by_path(&t);
        let paths: Vec<&str> = all.keys().map(String::as_str).collect();
        // Metadata files and the excluded recycle bin are hidden; DOS names are skipped.
        assert_eq!(
            paths,
            vec![
                "Docs",
                r"Docs\a.txt",
                r"Docs\big-ext.bin",
                r"Docs\big.bin",
                "link-to-a.txt"
            ]
        );
        assert_eq!(t.stats.excluded, 1);
        // Both hard-link names: same size, same reference, allocation counted once.
        let a = all[r"Docs\a.txt"].1;
        let link = all["link-to-a.txt"].1;
        assert_eq!((a.size, link.size), (5000, 5000));
        assert_eq!(a.file_ref, link.file_ref);
        assert_eq!(a.file_ref, Some(31 | (7 << 48)));
        assert_eq!(a.alloc_size.unwrap() + link.alloc_size.unwrap(), 8192);
        // Data from the extension record is merged into its base.
        assert_eq!(all[r"Docs\big.bin"].1.size, 1 << 20);
        assert_eq!(all[r"Docs\big-ext.bin"].1.size, 1 << 20);
        // Attributes: folders get the directory bit, empty attributes become NORMAL.
        assert_eq!(all["Docs"].1.attrs, ATTR_DIRECTORY);
        assert_eq!(all["Docs"].1.modified, Some(FileTime(20)));
        assert_eq!(t.root().meta.attrs, 0x16);
    }

    #[test]
    fn partial_scans_start_at_the_requested_folder() {
        let records = volume();
        let t = tree(
            &records,
            &["docs".to_owned()],
            &ExclusionPolicy::with_defaults(),
        );
        let all = by_path(&t);
        assert_eq!(all.len(), 3);
        assert!(all.contains_key("a.txt"));
        assert_eq!(t.root().meta.file_ref, Some(30 | (7 << 48)));
    }

    #[test]
    fn stale_names_of_reused_parent_records_are_dropped() {
        let mut records = volume();
        // Record 35 claims to live in folder 30 with an old sequence number.
        records.merge(
            35,
            parsed(
                RecordBuilder::new(false)
                    .standard(1, 1, 0x20)
                    .name(30 | (6 << 48), NAMESPACE_WIN32, "orphan")
                    .build(),
            ),
        );
        let all = by_path(&tree(&records, &[], &ExclusionPolicy::default()));
        assert!(!all.keys().any(|k| k.contains("orphan")));
    }

    #[test]
    fn device_paths() {
        assert_eq!(device_path(Path::new(r"C:\")).as_deref(), Some(r"\\.\C:"));
        assert_eq!(
            device_path(Path::new(r"\\?\F:\sub")).as_deref(),
            Some(r"\\.\F:")
        );
        assert_eq!(
            device_path(Path::new(r"\\?\Volume{1234-abcd}\")).as_deref(),
            Some(r"\\?\Volume{1234-abcd}")
        );
        assert_eq!(device_path(Path::new(r"\\server\share")), None);
    }
}
