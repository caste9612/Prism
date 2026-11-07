//! File names as stored in the catalog, and case-insensitive comparison.
//!
//! Windows file names are sequences of UTF-16 code units and may contain unpaired surrogates,
//! which cannot be represented in a Rust `String` (or SQLite `TEXT`). The catalog stores names
//! with a small reversible escape so that every name round-trips exactly and two different
//! names never collide (the catalog enforces unique names per folder):
//!
//! - an unpaired surrogate `0xD8xx` is written as `U+FFFF` followed by four uppercase hex
//!   digits (`\u{FFFF}D8xx`);
//! - a literal `U+FFFF` (a non-character, practically never used) is written as
//!   `\u{FFFF}FFFF`.
//!
//! Every other name is stored unchanged.

use std::ffi::{OsStr, OsString};

/// Escape marker: a Unicode non-character that does not appear in real file names.
const ESCAPE: char = '\u{FFFF}';

/// Converts an OS file name to its stored form (see the module documentation).
#[must_use]
pub fn encode(name: &OsStr) -> String {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        encode_units(name.encode_wide(), name.len())
    }
    #[cfg(not(windows))]
    {
        name.to_string_lossy().into_owned()
    }
}

/// Converts a name given as UTF-16 code units (as stored by NTFS) to its stored form.
#[must_use]
pub fn encode_wide(units: &[u16]) -> String {
    encode_units(units.iter().copied(), units.len())
}

fn encode_units(units: impl Iterator<Item = u16>, capacity: usize) -> String {
    let mut out = String::with_capacity(capacity);
    for unit in char::decode_utf16(units) {
        match unit {
            Ok(ESCAPE) => push_escaped(&mut out, 0xFFFF),
            Ok(c) => out.push(c),
            Err(e) => push_escaped(&mut out, e.unpaired_surrogate()),
        }
    }
    out
}

fn push_escaped(out: &mut String, unit: u16) {
    use std::fmt::Write as _;
    out.push(ESCAPE);
    // Writing to a String cannot fail.
    let _ = write!(out, "{unit:04X}");
}

/// Converts a stored name back to the exact OS file name.
#[must_use]
pub fn decode(stored: &str) -> OsString {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        if !stored.contains(ESCAPE) {
            return OsString::from(stored);
        }
        let mut units: Vec<u16> = Vec::with_capacity(stored.len());
        let mut chars = stored.chars();
        while let Some(c) = chars.next() {
            if c == ESCAPE {
                let hex: String = chars.by_ref().take(4).collect();
                match u16::from_str_radix(&hex, 16) {
                    Ok(unit) if hex.len() == 4 => units.push(unit),
                    // Not produced by `encode`; keep the text as it is.
                    _ => {
                        let mut buf = [0u16; 2];
                        units.extend_from_slice(c.encode_utf16(&mut buf));
                        units.extend(hex.encode_utf16());
                    }
                }
            } else {
                let mut buf = [0u16; 2];
                units.extend_from_slice(c.encode_utf16(&mut buf));
            }
        }
        OsString::from_wide(&units)
    }
    #[cfg(not(windows))]
    {
        OsString::from(stored)
    }
}

/// Upper-cases a single character the way NTFS does for name comparisons: a one-to-one
/// mapping (characters whose upper case expands, like `ß`, are left unchanged).
fn fold_char(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// Case-folded form of a name, for case-insensitive lookups and comparisons.
#[must_use]
pub fn fold(name: &str) -> String {
    name.chars().map(fold_char).collect()
}

/// Compares two names without distinguishing upper and lower case (Windows semantics).
#[must_use]
pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    (a.len() == b.len() && a.eq_ignore_ascii_case(b))
        || a.chars().map(fold_char).eq(b.chars().map(fold_char))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_are_unchanged() {
        for n in [
            "report.pdf",
            "Fotografie 2024",
            "日本語.txt",
            "emoji 😀.png",
            "",
        ] {
            assert_eq!(encode(OsStr::new(n)), n);
            assert_eq!(decode(n), OsString::from(n));
        }
    }

    #[cfg(windows)]
    #[test]
    fn unpaired_surrogates_roundtrip_and_do_not_collide() {
        use std::os::windows::ffi::OsStringExt;
        let a = OsString::from_wide(&[0x61, 0xD800, 0x62]);
        let b = OsString::from_wide(&[0x61, 0xDC00, 0x62]);
        let ea = encode(&a);
        let eb = encode(&b);
        assert_ne!(ea, eb);
        assert_eq!(ea, "a\u{FFFF}D800b");
        assert_eq!(decode(&ea), a);
        assert_eq!(decode(&eb), b);
    }

    #[cfg(windows)]
    #[test]
    fn wide_and_os_encodings_agree() {
        use std::os::windows::ffi::OsStringExt;
        let units = [0x61, 0xD800, 0x62, 0xFFFF, 0x65E5];
        assert_eq!(encode_wide(&units), encode(&OsString::from_wide(&units)));
    }

    #[cfg(windows)]
    #[test]
    fn literal_escape_char_roundtrips() {
        let n = OsString::from("x\u{FFFF}D800");
        let e = encode(&n);
        assert_eq!(e, "x\u{FFFF}FFFFD800");
        assert_eq!(decode(&e), n);
    }

    #[test]
    fn case_insensitive_comparison() {
        assert!(eq_ignore_case("$RECYCLE.BIN", "$Recycle.Bin"));
        assert!(eq_ignore_case("ÀÉÎ", "àéî"));
        assert!(!eq_ignore_case("straße", "STRASSE"));
        assert!(!eq_ignore_case("a", "b"));
        assert_eq!(fold("Système"), "SYSTÈME");
    }
}
