//! The JPEG preview embedded in camera RAW files (F11-AC4): decoding the raw sensor data is
//! neither needed nor practical for a perceptual print.
//!
//! TIFF-based formats (CR2, NEF, ARW, DNG, ORF, RW2, PEF, SRW...) list their previews in the
//! image directories: `JPEGInterchangeFormat` and its length, or a strip compressed as JPEG,
//! also in sub-directories. Other containers (CR3, RAF...) are scanned for JPEG streams. Of the
//! candidates found, the smallest one that is still large enough is used (decoding a
//! full-size preview costs much more and the print does not need it).

use std::collections::HashSet;

/// Previews at least this large on their shorter side are enough for a print.
const ENOUGH: u32 = 400;
/// JPEG streams looked at when scanning a container that is not TIFF.
const MAX_SCANNED: usize = 16;

/// A JPEG stream inside a RAW file, with the size its header declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preview<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

struct Tiff<'a> {
    data: &'a [u8],
    little: bool,
}

impl Tiff<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.data.get(at..at + 2)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.data.get(at..at + 4)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }

    /// The value of an entry as a number (SHORT or LONG, first value).
    fn value(&self, entry: usize) -> Option<u32> {
        match self.u16(entry + 2)? {
            3 => self.u16(entry + 8).map(u32::from),
            4 | 13 => self.u32(entry + 8),
            _ => None,
        }
    }

    /// The values of an entry that is a list of LONG offsets (SubIFDs).
    fn offsets(&self, entry: usize) -> Vec<u32> {
        let (Some(kind), Some(count)) = (self.u16(entry + 2), self.u32(entry + 4)) else {
            return Vec::new();
        };
        if kind != 4 && kind != 13 {
            return Vec::new();
        }
        let count = count.min(16) as usize;
        let base = if count == 1 {
            Some(entry + 8)
        } else {
            self.u32(entry + 8).map(|o| o as usize)
        };
        base.map(|b| (0..count).filter_map(|i| self.u32(b + 4 * i)).collect())
            .unwrap_or_default()
    }

    /// JPEG candidates of every directory reachable from `first`.
    fn candidates(&self, first: u32) -> Vec<(usize, usize)> {
        let mut found = Vec::new();
        let mut queue = vec![first];
        let mut seen = HashSet::new();
        while let Some(ifd) = queue.pop() {
            let ifd = ifd as usize;
            if ifd == 0 || seen.len() > 64 || !seen.insert(ifd) {
                continue;
            }
            let Some(count) = self.u16(ifd) else { continue };
            let (mut jpeg_at, mut jpeg_len) = (None, None);
            let (mut strip_at, mut strip_len, mut compression) = (None, None, None);
            for i in 0..usize::from(count.min(512)) {
                let entry = ifd + 2 + 12 * i;
                match self.u16(entry) {
                    Some(0x0201) => jpeg_at = self.value(entry),
                    Some(0x0202) => jpeg_len = self.value(entry),
                    Some(0x0111) => strip_at = self.value(entry),
                    Some(0x0117) => strip_len = self.value(entry),
                    Some(0x0103) => compression = self.value(entry),
                    Some(0x014A) => queue.extend(self.offsets(entry)),
                    Some(0x8769) => queue.extend(self.value(entry)),
                    _ => {}
                }
            }
            if let (Some(a), Some(l)) = (jpeg_at, jpeg_len) {
                found.push((a as usize, l as usize));
            }
            if let (Some(a), Some(l), Some(6 | 7)) = (strip_at, strip_len, compression) {
                found.push((a as usize, l as usize));
            }
            if let Some(next) = self.u32(ifd + 2 + 12 * usize::from(count)) {
                queue.push(next);
            }
        }
        found
    }
}

/// The size a JPEG stream declares in its frame header, without decoding it.
#[must_use]
pub fn jpeg_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut at = 2;
    loop {
        while *data.get(at)? != 0xFF {
            at += 1;
        }
        while *data.get(at)? == 0xFF {
            at += 1;
        }
        let marker = *data.get(at)?;
        at += 1;
        if matches!(marker, 0xD8 | 0x01 | 0xD0..=0xD7) {
            continue;
        }
        let len = usize::from(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]));
        // Start-of-frame markers (not DHT C4, JPG C8, DAC CC).
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = u16::from_be_bytes([*data.get(at + 3)?, *data.get(at + 4)?]);
            let w = u16::from_be_bytes([*data.get(at + 5)?, *data.get(at + 6)?]);
            return Some((u32::from(w), u32::from(h)));
        }
        if marker == 0xDA || len < 2 {
            return None;
        }
        at += len;
    }
}

fn tiff_candidates(data: &[u8]) -> Vec<(usize, usize)> {
    let little = match data.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Vec::new(),
    };
    let tiff = Tiff { data, little };
    // 42 is TIFF; Olympus (ORF) and Panasonic (RW2) use their own magic numbers.
    match tiff.u16(2) {
        Some(42 | 0x4F52 | 0x5352 | 0x55) => {}
        _ => return Vec::new(),
    }
    tiff.u32(4)
        .map(|first| tiff.candidates(first))
        .unwrap_or_default()
}

fn scanned_candidates(data: &[u8]) -> Vec<(usize, usize)> {
    memchr::memmem::find_iter(data, &[0xFF, 0xD8, 0xFF])
        .take(MAX_SCANNED)
        .map(|at| (at, data.len() - at))
        .collect()
}

/// The preview to print: the smallest one large enough, or else the largest.
#[must_use]
pub fn preview(data: &[u8]) -> Option<Preview<'_>> {
    let mut candidates = tiff_candidates(data);
    if candidates.is_empty() {
        candidates = scanned_candidates(data);
    }
    let mut found: Vec<Preview<'_>> = candidates
        .into_iter()
        .filter_map(|(at, len)| {
            let slice = data.get(at..at.checked_add(len)?)?;
            let (width, height) = jpeg_size(slice)?;
            (width > 0 && height > 0).then_some(Preview {
                data: slice,
                width,
                height,
            })
        })
        .collect();
    found.sort_by_key(|p| u64::from(p.width) * u64::from(p.height));
    found
        .iter()
        .find(|p| p.width.min(p.height) >= ENOUGH)
        .or_else(|| found.last())
        .copied()
}

/// The largest size among the previews: the closest to the photo's own (F11-AC3).
#[must_use]
pub fn largest_size(data: &[u8]) -> Option<(u32, u32)> {
    let mut candidates = tiff_candidates(data);
    if candidates.is_empty() {
        candidates = scanned_candidates(data);
    }
    candidates
        .into_iter()
        .filter_map(|(at, len)| jpeg_size(data.get(at..at.checked_add(len)?)?))
        .max_by_key(|(w, h)| u64::from(*w) * u64::from(*h))
}
