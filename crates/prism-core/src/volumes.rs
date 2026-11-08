//! Connected volumes and their stable identity (ADR-0004, ADR-0016).
//!
//! The platform ([`VolumeSource`], implemented in `prism-win`) reports the volumes that are
//! connected now; [`match_volumes`] decides, for each one, whether it is a volume of the
//! catalog (even if its drive letter changed, F02-AC3), a new one, or ambiguous (the user must
//! choose). The drive letter is never an identity key.

use std::collections::HashSet;

use crate::model::{Volume, VolumeId, VolumeInfo, VolumeKind, VolumeStatus};
use crate::names;
use crate::store::{self, Store, StoreError};

/// A volume connected now, as reported by the platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedVolume {
    /// System information and identity keys; `last_mount` is the preferred mount (a drive
    /// letter when there is one).
    pub info: VolumeInfo,
    /// Every path where the volume is mounted (drive letters like `D:\`, mount folders).
    pub mounts: Vec<String>,
    /// Physical disks holding the volume. Runtime only: disk numbers change across reboots.
    /// Used to run at most one heavy job per physical disk (ADR-0009).
    pub disks: Vec<u32>,
}

/// Errors of a [`VolumeSource`].
#[derive(Debug, thiserror::Error)]
pub enum VolumeError {
    #[error("cannot list the volumes: {0}")]
    Platform(String),
}

/// Lists the volumes connected now.
pub trait VolumeSource: Send + Sync {
    /// # Errors
    /// [`VolumeError`] if the volumes cannot be listed at all. Volumes that cannot be read
    /// (unformatted, locked by BitLocker, empty card readers) are simply left out.
    fn detect(&self) -> Result<Vec<DetectedVolume>, VolumeError>;
}

/// Outcome of matching a connected volume with the catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VolumeMatch {
    /// The volume is this catalog volume.
    Known(VolumeId),
    /// Never seen: ask the user whether to index it (F02-AC1).
    New,
    /// It could be one of these catalog volumes (cloned disks with the same serial, or the
    /// only candidate has a different capacity): the user decides (F02-AC3).
    Ambiguous(Vec<VolumeId>),
}

/// Canonical form of a UNC path for comparisons: `\\server\share`, case-folded, without
/// trailing separators; `\\?\UNC\server\share` is accepted too.
#[must_use]
pub fn normalize_unc(path: &str) -> String {
    let p = path.replace('/', "\\");
    let p = match p.get(..8) {
        Some(prefix) if prefix.eq_ignore_ascii_case(r"\\?\UNC\") => format!(r"\\{}", &p[8..]),
        _ => p,
    };
    names::fold(p.trim_end_matches('\\'))
}

fn same_text(a: Option<&str>, b: Option<&str>) -> bool {
    matches!((a, b), (Some(x), Some(y)) if names::eq_ignore_case(x, y))
}

/// Catalog volumes that share the identity key of `d`.
fn candidates<'a>(d: &VolumeInfo, known: &'a [Volume]) -> Vec<&'a Volume> {
    if d.kind == VolumeKind::Network {
        let Some(unc) = d.unc_path.as_deref().map(normalize_unc) else {
            return Vec::new();
        };
        return known
            .iter()
            .filter(|v| {
                v.info.kind == VolumeKind::Network
                    && v.info.unc_path.as_deref().map(normalize_unc).as_ref() == Some(&unc)
            })
            .collect();
    }
    if let Some(serial) = d.fs_serial {
        return known
            .iter()
            .filter(|v| {
                v.info.kind != VolumeKind::Network
                    && v.info.fs_serial == Some(serial)
                    && same_text(v.info.fs_type.as_deref(), d.fs_type.as_deref())
            })
            .collect();
    }
    // Without a serial number, only the volume GUID (valid on this machine) can match.
    known
        .iter()
        .filter(|v| {
            v.info.kind != VolumeKind::Network
                && same_text(v.info.volume_guid.as_deref(), d.volume_guid.as_deref())
        })
        .collect()
}

fn decide(d: &VolumeInfo, cands: &[&Volume]) -> VolumeMatch {
    if cands.is_empty() {
        return VolumeMatch::New;
    }
    if d.kind == VolumeKind::Network {
        return match cands {
            [one] => VolumeMatch::Known(one.id),
            _ => VolumeMatch::Ambiguous(cands.iter().map(|v| v.id).collect()),
        };
    }
    // Same serial and same volume GUID: the same volume on this machine, even if resized.
    let same_guid: Vec<&&Volume> = cands
        .iter()
        .filter(|v| same_text(v.info.volume_guid.as_deref(), d.volume_guid.as_deref()))
        .collect();
    if let [one] = same_guid.as_slice() {
        return VolumeMatch::Known(one.id);
    }
    let same_capacity: Vec<&&Volume> = cands
        .iter()
        .filter(|v| d.capacity.is_none() || v.info.capacity == d.capacity)
        .collect();
    match same_capacity.as_slice() {
        [one] => VolumeMatch::Known(one.id),
        [] => VolumeMatch::Ambiguous(cands.iter().map(|v| v.id).collect()),
        many => VolumeMatch::Ambiguous(many.iter().map(|v| v.id).collect()),
    }
}

/// Matches every connected volume with the catalog (ADR-0016). A catalog volume is assigned
/// to at most one connected volume; matches confirmed by the volume GUID are assigned first.
#[must_use]
pub fn match_volumes(detected: &[DetectedVolume], known: &[Volume]) -> Vec<VolumeMatch> {
    let mut result: Vec<Option<VolumeMatch>> = vec![None; detected.len()];
    let mut taken: HashSet<VolumeId> = HashSet::new();
    for strong_pass in [true, false] {
        for (i, d) in detected.iter().enumerate() {
            if result[i].is_some() {
                continue;
            }
            let cands: Vec<&Volume> = candidates(&d.info, known)
                .into_iter()
                .filter(|v| !taken.contains(&v.id))
                .collect();
            let decision = decide(&d.info, &cands);
            let strong = matches!(decision, VolumeMatch::Known(id) if known
                .iter()
                .any(|v| v.id == id
                    && same_text(v.info.volume_guid.as_deref(), d.info.volume_guid.as_deref())));
            if strong_pass && !strong {
                continue;
            }
            if let VolumeMatch::Known(id) = decision {
                taken.insert(id);
            }
            result[i] = Some(decision);
        }
    }
    result
        .into_iter()
        .map(|m| m.unwrap_or(VolumeMatch::New))
        .collect()
}

/// Records a connected volume in the catalog according to its match: a known volume gets its
/// current information (mount, free space, last seen); a new one is added with status `new`
/// (indexing waits for the user, F02-AC1). Ambiguous volumes are not recorded.
///
/// # Errors
/// Store errors.
pub fn record_detected(
    store: &Store,
    detected: &DetectedVolume,
    matched: &VolumeMatch,
) -> Result<Option<VolumeId>, StoreError> {
    let info = detected.info.clone();
    match matched {
        VolumeMatch::Known(id) => {
            let id = *id;
            store.write(move |w| store::update_volume_info(w, id, &info))?;
            Ok(Some(id))
        }
        VolumeMatch::New => {
            Ok(Some(store.write(move |w| {
                store::insert_volume(w, &info, VolumeStatus::New)
            })?))
        }
        VolumeMatch::Ambiguous(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BusType, VolumeLabels};

    fn ntfs(serial: u64, capacity: u64, guid: &str, mount: &str) -> VolumeInfo {
        VolumeInfo {
            kind: VolumeKind::Fixed,
            fs_type: Some("NTFS".into()),
            fs_serial: Some(serial),
            volume_guid: Some(format!(r"\\?\Volume{{{guid}}}\")),
            unc_path: None,
            os_label: Some("Data".into()),
            capacity: Some(capacity),
            free_space: Some(capacity / 2),
            bus: Some(BusType::Usb),
            last_mount: Some(mount.into()),
        }
    }

    fn network(unc: &str, mount: &str) -> VolumeInfo {
        VolumeInfo {
            kind: VolumeKind::Network,
            unc_path: Some(unc.into()),
            last_mount: Some(mount.into()),
            ..VolumeInfo::dev_folder(mount)
        }
    }

    fn detected(info: VolumeInfo) -> DetectedVolume {
        let mounts = info.last_mount.iter().cloned().collect();
        DetectedVolume {
            info,
            mounts,
            disks: vec![1],
        }
    }

    fn catalog(infos: Vec<VolumeInfo>) -> Vec<Volume> {
        infos
            .into_iter()
            .enumerate()
            .map(|(i, info)| Volume {
                id: VolumeId(i as i64 + 1),
                info,
                labels: VolumeLabels::default(),
                status: VolumeStatus::Indexed,
                first_seen_at: 0,
                last_seen_at: None,
                last_indexed_at: None,
                usn: None,
            })
            .collect()
    }

    // F02-AC3: a known volume is recognized even with another drive letter.
    #[test]
    fn f02_ac3_known_volume_with_a_different_letter() {
        let known = catalog(vec![ntfs(0xAB, 1000, "1", r"E:\")]);
        let now = [detected(ntfs(0xAB, 1000, "1", r"G:\"))];
        assert_eq!(
            match_volumes(&now, &known),
            vec![VolumeMatch::Known(VolumeId(1))]
        );
        // On another PC the GUID differs: serial and capacity are enough.
        let elsewhere = [detected(ntfs(0xAB, 1000, "other-pc", r"F:\"))];
        assert_eq!(
            match_volumes(&elsewhere, &known),
            vec![VolumeMatch::Known(VolumeId(1))]
        );
    }

    #[test]
    fn unknown_serial_is_new_and_letter_alone_never_matches() {
        let known = catalog(vec![ntfs(0xAB, 1000, "1", r"E:\")]);
        let now = [detected(ntfs(0xCD, 1000, "2", r"E:\"))];
        assert_eq!(match_volumes(&now, &known), vec![VolumeMatch::New]);
    }

    // F02-AC3: clones (same serial and capacity) on another machine are ambiguous: ask.
    #[test]
    fn f02_ac3_cloned_disks_are_ambiguous() {
        let known = catalog(vec![
            ntfs(0xAB, 1000, "a", r"E:\"),
            ntfs(0xAB, 1000, "b", r"F:\"),
        ]);
        let now = [detected(ntfs(0xAB, 1000, "c", r"G:\"))];
        assert_eq!(
            match_volumes(&now, &known),
            vec![VolumeMatch::Ambiguous(vec![VolumeId(1), VolumeId(2)])]
        );
        // On the machine where they were seen, the GUID decides.
        let here = [detected(ntfs(0xAB, 1000, "b", r"G:\"))];
        assert_eq!(
            match_volumes(&here, &known),
            vec![VolumeMatch::Known(VolumeId(2))]
        );
    }

    #[test]
    fn two_clones_connected_together_get_different_volumes() {
        let known = catalog(vec![
            ntfs(0xAB, 1000, "a", r"E:\"),
            ntfs(0xAB, 1000, "b", r"F:\"),
        ]);
        // The second one is confirmed by its GUID and is assigned first.
        let now = [
            detected(ntfs(0xAB, 1000, "a", r"E:\")),
            detected(ntfs(0xAB, 1000, "b", r"F:\")),
        ];
        assert_eq!(
            match_volumes(&now, &known),
            vec![
                VolumeMatch::Known(VolumeId(1)),
                VolumeMatch::Known(VolumeId(2))
            ]
        );
    }

    #[test]
    fn different_capacity_is_ambiguous_unless_the_guid_confirms() {
        let known = catalog(vec![ntfs(0xAB, 1000, "a", r"E:\")]);
        let resized_here = [detected(ntfs(0xAB, 2000, "a", r"E:\"))];
        assert_eq!(
            match_volumes(&resized_here, &known),
            vec![VolumeMatch::Known(VolumeId(1))]
        );
        let other_disk = [detected(ntfs(0xAB, 2000, "z", r"E:\"))];
        assert_eq!(
            match_volumes(&other_disk, &known),
            vec![VolumeMatch::Ambiguous(vec![VolumeId(1)])]
        );
    }

    #[test]
    fn same_serial_on_another_file_system_does_not_match() {
        let mut fat = ntfs(0xAB, 1000, "a", r"E:\");
        fat.fs_type = Some("exFAT".into());
        let known = catalog(vec![ntfs(0xAB, 1000, "a", r"E:\")]);
        assert_eq!(
            match_volumes(&[detected(fat)], &known),
            vec![VolumeMatch::New]
        );
    }

    // F02-AC5: network drives are identified by their normalized UNC path, not the letter.
    #[test]
    fn network_drives_match_by_unc_path() {
        let known = catalog(vec![network(r"\\NAS\Photos", r"Z:\")]);
        let now = [
            detected(network(r"\\nas\photos\", r"Y:\")),
            detected(network(r"\\nas\backup", r"Z:\")),
        ];
        assert_eq!(
            match_volumes(&now, &known),
            vec![VolumeMatch::Known(VolumeId(1)), VolumeMatch::New]
        );
        assert_eq!(normalize_unc(r"\\?\UNC\Nas\Photos\"), r"\\NAS\PHOTOS");
    }

    #[test]
    fn record_detected_updates_known_and_adds_new() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("c.db")).unwrap();
        let first = detected(ntfs(0xAB, 1000, "a", r"E:\"));
        let id = record_detected(&store, &first, &VolumeMatch::New)
            .unwrap()
            .unwrap();
        let known = store.read(store::volumes).unwrap();
        let moved = detected(ntfs(0xAB, 1000, "a", r"G:\"));
        let m = match_volumes(std::slice::from_ref(&moved), &known);
        assert_eq!(m, vec![VolumeMatch::Known(id)]);
        record_detected(&store, &moved, &m[0]).unwrap();
        let v = store.read(|c| store::volume(c, id)).unwrap().unwrap();
        assert_eq!(v.info.last_mount.as_deref(), Some(r"G:\"));
        assert_eq!(v.info.bus, Some(BusType::Usb));
        assert_eq!(v.info.display_kind(), crate::model::DisplayKind::External);
        assert_eq!(v.status, VolumeStatus::New);
        assert!(v.last_seen_at.is_some());
        assert_eq!(
            record_detected(&store, &moved, &VolumeMatch::Ambiguous(vec![id])).unwrap(),
            None
        );
    }
}
