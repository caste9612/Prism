//! Catalog model: identifiers, entry metadata and volumes (docs/ARCHITECTURE.md §3).

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Identifier of a volume in the catalog (`volumes.id`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VolumeId(pub i64);

/// Identifier of a file or folder in the catalog (`entries.id`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(pub i64);

impl fmt::Display for VolumeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A Windows `FILETIME`: 100-nanosecond intervals since 1601-01-01 UTC.
///
/// Entry timestamps are stored with this full precision so that the walker and the MFT reader
/// produce identical values, and so that hash validity (ADR-0011) is not weakened by rounding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileTime(pub i64);

impl FileTime {
    /// Number of 100 ns intervals between 1601-01-01 and 1970-01-01.
    pub const UNIX_EPOCH_OFFSET: i64 = 116_444_736_000_000_000;

    /// Converts to milliseconds since the Unix epoch (may be negative for very old dates).
    #[must_use]
    pub fn to_unix_millis(self) -> i64 {
        (self.0 - Self::UNIX_EPOCH_OFFSET).div_euclid(10_000)
    }

    /// Builds a `FileTime` from milliseconds since the Unix epoch.
    #[must_use]
    pub fn from_unix_millis(ms: i64) -> Self {
        Self(
            ms.saturating_mul(10_000)
                .saturating_add(Self::UNIX_EPOCH_OFFSET),
        )
    }
}

/// Milliseconds since the Unix epoch, used for application timestamps (first seen, indexed...).
#[must_use]
pub fn unix_millis_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Windows `FILE_ATTRIBUTE_DIRECTORY`.
pub const ATTR_DIRECTORY: u32 = 0x10;
/// Windows `FILE_ATTRIBUTE_REPARSE_POINT`.
pub const ATTR_REPARSE_POINT: u32 = 0x400;

/// Metadata of a file or folder as stored in the catalog (F04-AC3).
///
/// All fields come from the enumeration; an update overwrites them with the latest scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct EntryMeta {
    /// Logical size in bytes (0 for folders).
    pub size: u64,
    /// Space allocated on disk, when the enumerator knows it (MFT); `None` for the walker.
    pub alloc_size: Option<u64>,
    pub created: Option<FileTime>,
    pub modified: Option<FileTime>,
    /// Windows file attributes (`FILE_ATTRIBUTE_*`).
    pub attrs: u32,
    /// NTFS file reference (record number and sequence), when known. It identifies a file
    /// across renames and moves (F05-AC4).
    pub file_ref: Option<u64>,
}

/// Kind of volume, as shown in the catalog (F01).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VolumeKind {
    Fixed,
    Removable,
    Network,
}

impl VolumeKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Removable => "removable",
            Self::Network => "network",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fixed" => Some(Self::Fixed),
            "removable" => Some(Self::Removable),
            "network" => Some(Self::Network),
            _ => None,
        }
    }
}

/// Indexing state of a volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VolumeStatus {
    /// Known but never indexed (e.g. waiting for the user's answer, F02-AC1).
    New,
    /// An indexing or update is writing to the catalog: readers may see partial data.
    Indexing,
    /// The catalog matches the last completed indexing or update.
    Indexed,
    /// The user chose "always ignore" (F02-AC4).
    Ignored,
    /// An indexing or update did not complete (cancelled, crash, error): the data may be
    /// partially updated and a new indexing is required (F04-AC6).
    ReindexNeeded,
}

impl VolumeStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Indexing => "indexing",
            Self::Indexed => "indexed",
            Self::Ignored => "ignored",
            Self::ReindexNeeded => "reindex_needed",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "new" => Some(Self::New),
            "indexing" => Some(Self::Indexing),
            "indexed" => Some(Self::Indexed),
            "ignored" => Some(Self::Ignored),
            "reindex_needed" => Some(Self::ReindexNeeded),
            _ => None,
        }
    }
}

/// User labels of a volume (F03).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VolumeLabels {
    pub name: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub color: Option<String>,
}

/// Identity and system information of a volume, as detected (ADR-0004).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeInfo {
    pub kind: VolumeKind,
    pub fs_type: Option<String>,
    pub fs_serial: Option<u64>,
    pub volume_guid: Option<String>,
    pub unc_path: Option<String>,
    pub os_label: Option<String>,
    pub capacity: Option<u64>,
    pub free_space: Option<u64>,
    /// Last drive letter or path where the volume was seen. Never an identity key.
    pub last_mount: Option<String>,
}

impl VolumeInfo {
    /// A development "volume" rooted at an arbitrary folder (used by `prism-cli` until real
    /// volume detection arrives in T2).
    #[must_use]
    pub fn dev_folder(path: &str) -> Self {
        Self {
            kind: VolumeKind::Fixed,
            fs_type: None,
            fs_serial: None,
            volume_guid: None,
            unc_path: None,
            os_label: None,
            capacity: None,
            free_space: None,
            last_mount: Some(path.to_owned()),
        }
    }
}

/// A volume of the catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub id: VolumeId,
    pub info: VolumeInfo,
    pub labels: VolumeLabels,
    pub status: VolumeStatus,
    pub first_seen_at: i64,
    pub last_seen_at: Option<i64>,
    pub last_indexed_at: Option<i64>,
}

/// A catalog entry with its position in the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRecord {
    pub id: EntryId,
    pub volume: VolumeId,
    /// `None` only for the root of a volume.
    pub parent: Option<EntryId>,
    /// Stored name (see [`crate::names`]); empty for the root of a volume.
    pub name: String,
    pub is_dir: bool,
    pub meta: EntryMeta,
}

/// Aggregates of a folder over its whole subtree (F04-AC4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DirStats {
    /// Sum of the logical sizes of all files below the folder.
    pub total_size: u64,
    /// Sum of the allocated sizes of all files below the folder (logical size when unknown).
    pub total_alloc: u64,
    pub file_count: u64,
    pub dir_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_unix_roundtrip() {
        let ft = FileTime::from_unix_millis(1_700_000_000_123);
        assert_eq!(ft.to_unix_millis(), 1_700_000_000_123);
        assert_eq!(FileTime(FileTime::UNIX_EPOCH_OFFSET).to_unix_millis(), 0);
    }

    #[test]
    fn enums_roundtrip() {
        for k in [
            VolumeKind::Fixed,
            VolumeKind::Removable,
            VolumeKind::Network,
        ] {
            assert_eq!(VolumeKind::parse(k.as_str()), Some(k));
        }
        for s in [
            VolumeStatus::New,
            VolumeStatus::Indexing,
            VolumeStatus::Indexed,
            VolumeStatus::Ignored,
            VolumeStatus::ReindexNeeded,
        ] {
            assert_eq!(VolumeStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(VolumeStatus::parse("bogus"), None);
    }
}
