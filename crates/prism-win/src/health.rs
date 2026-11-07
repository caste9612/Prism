//! SMART data of the physical disks (F17), read with the storage IOCTLs of Windows:
//!
//! - identity and bus of the disk: `IOCTL_STORAGE_QUERY_PROPERTY` / `StorageDeviceProperty`;
//! - NVMe: the health information log page, through `StorageDeviceProtocolSpecificProperty`;
//! - ATA/SATA: `SMART_RCV_DRIVE_DATA` (READ ATTRIBUTES, READ THRESHOLDS) and
//!   `SMART_SEND_DRIVE_COMMAND` (RETURN STATUS).
//!
//! Only commands that read are sent. The SMART IOCTLs of ATA disks are defined with read and
//! write access, so their handle is opened that way, but nothing is ever written (REVIEW D44).
//! Disks behind USB bridges usually do not pass these commands: they are reported as not
//! available (F17-AC4). The interpretation is `prism_core::health`.

use prism_core::health::{self, HealthReading};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows_sys::Win32::System::Ioctl::{
    IOCTL_STORAGE_QUERY_PROPERTY, NVMeDataTypeLogPage, ProtocolTypeNvme, READ_ATTRIBUTES,
    READ_THRESHOLDS, RETURN_SMART_STATUS, SMART_CMD, SMART_CYL_HI, SMART_CYL_LOW,
    SMART_RCV_DRIVE_DATA, SMART_SEND_DRIVE_COMMAND, StorageDeviceProperty,
    StorageDeviceProtocolSpecificProperty,
};

use crate::volumes::{Handle, ioctl};

/// How the disk is attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    Nvme,
    Ata,
    Usb,
    /// A virtual disk (VHD/VHDX, Storage Spaces): no SMART of its own.
    Virtual,
    Other(u32),
}

impl Bus {
    fn from_code(code: u32) -> Self {
        // STORAGE_BUS_TYPE: 3 ATA, 11 SATA, 7 USB, 14-16 virtual and Storage Spaces, 17 NVMe.
        match code {
            17 => Self::Nvme,
            3 | 11 => Self::Ata,
            7 => Self::Usb,
            14..=16 => Self::Virtual,
            n => Self::Other(n),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nvme => "nvme",
            Self::Ata => "sata",
            Self::Usb => "usb",
            Self::Virtual => "virtual",
            Self::Other(_) => "other",
        }
    }
}

/// Model, serial number and bus of a disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskIdentity {
    pub model: String,
    pub serial: String,
    pub bus: Bus,
}

impl DiskIdentity {
    /// What identifies the disk across reboots (disk numbers change): model and serial.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}|{}", self.model, self.serial)
    }
}

/// The health of a disk: a reading, or why there is none (F17-AC4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskHealth {
    pub number: u32,
    pub identity: DiskIdentity,
    pub reading: Result<HealthReading, &'static str>,
}

fn bytes_of(words: &[u64], len: usize) -> Vec<u8> {
    words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .take(len)
        .collect()
}

/// A NUL-terminated ASCII string at `offset` of a descriptor (0: absent), trimmed.
fn ascii_at(desc: &[u8], offset: u32) -> String {
    let at = offset as usize;
    if at == 0 || at >= desc.len() {
        return String::new();
    }
    let end = desc[at..]
        .iter()
        .position(|&b| b == 0)
        .map_or(desc.len(), |p| at + p);
    String::from_utf8_lossy(&desc[at..end]).trim().to_owned()
}

/// The identity in a `STORAGE_DEVICE_DESCRIPTOR`.
fn parse_descriptor(desc: &[u8]) -> Option<DiskIdentity> {
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(desc.get(at..at + 4)?.try_into().ok()?))
    };
    let vendor = ascii_at(desc, u32_at(12)?);
    let product = ascii_at(desc, u32_at(16)?);
    let model = if vendor.is_empty() || product.starts_with(&vendor) {
        product
    } else {
        format!("{vendor} {product}")
    };
    Some(DiskIdentity {
        model,
        serial: ascii_at(desc, u32_at(24)?),
        bus: Bus::from_code(u32_at(28)?),
    })
}

fn identity(h: &Handle) -> Option<DiskIdentity> {
    // STORAGE_PROPERTY_QUERY: PropertyId, QueryType (standard), AdditionalParameters.
    let mut query = Vec::with_capacity(12);
    query.extend_from_slice(&(StorageDeviceProperty as u32).to_le_bytes());
    query.extend_from_slice(&0u32.to_le_bytes());
    query.extend_from_slice(&[0u8; 4]);
    let mut out = vec![0u64; 128];
    let n = ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &query, &mut out)?;
    parse_descriptor(&bytes_of(&out, n))
}

/// The NVMe health information log page (512 bytes).
fn nvme_log(h: &Handle) -> Option<Vec<u8>> {
    // STORAGE_PROPERTY_QUERY (8 bytes) followed by STORAGE_PROTOCOL_SPECIFIC_DATA (40 bytes)
    // and room for the log page; the answer has the same layout.
    const SPECIFIC: u32 = 40;
    const LOG: u32 = 512;
    let mut query = Vec::with_capacity((8 + SPECIFIC + LOG) as usize);
    query.extend_from_slice(&(StorageDeviceProtocolSpecificProperty as u32).to_le_bytes());
    query.extend_from_slice(&0u32.to_le_bytes());
    for v in [
        ProtocolTypeNvme as u32,
        NVMeDataTypeLogPage as u32,
        2, // log page 02h: SMART / health information
        0,
        SPECIFIC, // data offset, from the start of this structure
        LOG,
        0,
        0,
        0,
        0,
    ] {
        query.extend_from_slice(&v.to_le_bytes());
    }
    query.resize((8 + SPECIFIC + LOG) as usize, 0);
    let mut out = vec![0u64; (8 + SPECIFIC + LOG) as usize / 8];
    let n = ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &query, &mut out)?;
    let bytes = bytes_of(&out, n);
    // STORAGE_PROTOCOL_DATA_DESCRIPTOR: Version, Size, then the protocol data structure.
    let offset = u32::from_le_bytes(bytes.get(8 + 16..8 + 20)?.try_into().ok()?) as usize;
    let length = u32::from_le_bytes(bytes.get(8 + 20..8 + 24)?.try_into().ok()?) as usize;
    let start = 8 + offset;
    Some(bytes.get(start..start + length.min(LOG as usize))?.to_vec())
}

/// SENDCMDINPARAMS for a SMART command (32 bytes without the data buffer).
fn smart_command(number: u32, feature: u32) -> Vec<u8> {
    let mut p = Vec::with_capacity(32);
    p.extend_from_slice(&512u32.to_le_bytes()); // cBufferSize
    // IDEREGS: features, sector count, sector number, cyl low, cyl high, drive/head, command.
    p.extend_from_slice(&[
        feature as u8,
        1,
        1,
        SMART_CYL_LOW as u8,
        SMART_CYL_HI as u8,
        0xA0 | (((number & 1) as u8) << 4),
        SMART_CMD as u8,
        0,
    ]);
    p.push(number as u8); // bDriveNumber
    p.resize(32, 0);
    p
}

/// A SMART data page (attributes or thresholds) of an ATA disk.
fn ata_page(h: &Handle, number: u32, feature: u32) -> Option<Vec<u8>> {
    // SENDCMDOUTPARAMS: cBufferSize (4), DRIVERSTATUS (12), then the 512-byte page.
    let mut out = vec![0u64; (16 + 512) / 8];
    let n = ioctl(
        h,
        SMART_RCV_DRIVE_DATA,
        &smart_command(number, feature),
        &mut out,
    )?;
    let bytes = bytes_of(&out, n);
    (bytes.len() >= 16 + 512 && bytes[4] == 0).then(|| bytes[16..16 + 512].to_vec())
}

/// Whether the disk reports a SMART threshold exceeded (RETURN STATUS).
fn ata_failing(h: &Handle, number: u32) -> bool {
    let mut out = vec![0u64; (16 + 512) / 8];
    let Some(n) = ioctl(
        h,
        SMART_SEND_DRIVE_COMMAND,
        &smart_command(number, RETURN_SMART_STATUS),
        &mut out,
    ) else {
        return false;
    };
    let bytes = bytes_of(&out, n);
    // The registers come back in the buffer: cylinder 0x2CF4 means "threshold exceeded".
    bytes.len() >= 16 + 7 && bytes[16 + 3] == 0xF4 && bytes[16 + 4] == 0x2C
}

/// The health of physical disk `number`, or `None` when the disk cannot be opened at all.
#[must_use]
pub fn read_disk(number: u32) -> Option<DiskHealth> {
    let path = format!(r"\\.\PhysicalDrive{number}");
    let any = Handle::open(&path, 0, 0)?;
    let identity = identity(&any)?;
    let reading = match identity.bus {
        Bus::Nvme => nvme_log(&any)
            .or_else(|| Handle::open(&path, GENERIC_READ, 0).and_then(|h| nvme_log(&h)))
            .and_then(|log| health::nvme_reading(&log))
            .ok_or("not_supported"),
        Bus::Usb => Err("usb"),
        Bus::Virtual => Err("virtual"),
        _ => match Handle::open(&path, GENERIC_READ | GENERIC_WRITE, 0) {
            None => Err("no_access"),
            Some(h) => match (
                ata_page(&h, number, READ_ATTRIBUTES),
                ata_page(&h, number, READ_THRESHOLDS),
            ) {
                (Some(values), Some(limits)) => Ok(health::ata_reading(
                    &health::ata_attributes(&values, &limits),
                    ata_failing(&h, number),
                )),
                _ => Err("not_supported"),
            },
        },
    };
    Some(DiskHealth {
        number,
        identity,
        reading,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_and_commands() {
        let mut desc = vec![0u8; 128];
        desc[12..16].copy_from_slice(&64u32.to_le_bytes());
        desc[16..20].copy_from_slice(&72u32.to_le_bytes());
        desc[24..28].copy_from_slice(&96u32.to_le_bytes());
        desc[28..32].copy_from_slice(&17u32.to_le_bytes());
        desc[64..68].copy_from_slice(b"ACME");
        desc[72..84].copy_from_slice(b"FastDisk 1TB");
        desc[96..104].copy_from_slice(b"  S3R1AL");
        let id = parse_descriptor(&desc).unwrap();
        assert_eq!(id.model, "ACME FastDisk 1TB");
        assert_eq!(id.serial, "S3R1AL");
        assert_eq!(id.bus, Bus::Nvme);
        assert_eq!(id.key(), "ACME FastDisk 1TB|S3R1AL");

        let cmd = smart_command(1, READ_ATTRIBUTES);
        assert_eq!(cmd.len(), 32);
        assert_eq!(
            (cmd[4], cmd[7], cmd[8], cmd[10], cmd[12]),
            (0xD0, 0x4F, 0xC2, 0xB0, 1)
        );
        assert_eq!(cmd[9], 0xB0, "drive/head register of drive 1");
    }

    // F17-AC1/AC4: every disk of this machine gives a reading or says why it cannot (read
    // only; without administrator rights most disks answer "no_access" or "not_supported").
    #[test]
    fn the_disks_of_this_machine_answer() {
        let mut seen = 0;
        for n in 0..8 {
            if let Some(d) = read_disk(n) {
                seen += 1;
                assert!(!d.identity.model.is_empty() || !d.identity.serial.is_empty());
                if let Ok(r) = &d.reading {
                    assert!(r.status.is_some());
                }
                println!("disk {n}: {:?} {:?}", d.identity, d.reading);
            }
        }
        assert!(seen > 0, "at least the system disk");
    }
}
