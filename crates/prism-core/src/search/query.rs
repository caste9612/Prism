//! The search language (F07-AC5/AC6): free words matched as substrings of the name (all of
//! them), and filters written `key:value`, all combined with AND.
//!
//! ```text
//! report size:>1GB ext:pdf,docx type:image modified:<30d disk:"Backup Foto" path:Documenti
//! is:dir online:no regex:^IMG_\d+ d:
//! ```
//!
//! `:` cannot appear in a Windows file name, so every `word:` is a filter: an unknown one is an
//! error, never a silent search for that text (F07-AC6). A single letter (`d:`) is the drive
//! with that letter. Errors carry a code, the offending value and its position, for the
//! interface to show and translate.

use crate::model::FileTime;
use crate::names;
use crate::space::FileCategory;

/// A parsed search.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// Case-folded words that must all appear in the name.
    pub terms: Vec<String>,
    pub filters: Vec<Filter>,
}

impl Query {
    /// Whether nothing at all was asked.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.filters.is_empty()
    }
}

/// Files or folders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
}

/// One filter of F07-AC5.
#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    /// Size in bytes, bounds included.
    Size {
        min: Option<u64>,
        max: Option<u64>,
    },
    /// Extensions, lowercase, without the dot.
    Ext(Vec<String>),
    Type(Vec<FileCategory>),
    /// Modification time: `after` included, `before` excluded.
    Modified {
        after: Option<FileTime>,
        before: Option<FileTime>,
    },
    /// A disk by name, label or letter (`D:`), resolved by the engine.
    Disk(String),
    /// Case-folded text that the folder path must contain.
    Path(String),
    Is(Kind),
    Online(bool),
    /// A regular expression on the name (case-insensitive).
    Regex(String),
}

/// A mistake in the search text (F07-AC6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    /// `unclosed_quote`, `unknown_filter`, `empty_value`, `bad_size`, `bad_date`, `bad_type`,
    /// `bad_is`, `bad_online`, `bad_regex`.
    pub code: &'static str,
    /// The filter name or the value at fault.
    pub value: String,
    /// Byte range in the input.
    pub start: usize,
    pub end: usize,
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} `{}` at {}..{}",
            self.code, self.value, self.start, self.end
        )
    }
}

impl std::error::Error for QueryError {}

/// The moment the search runs, for relative dates (`modified:<30d`).
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// Milliseconds since 1970 (UTC).
    pub now_ms: i64,
    /// Offset of the user's time zone from UTC, in minutes (`+60` for Italy in winter): dates
    /// typed by the user are local days.
    pub utc_offset_min: i32,
}

struct Token<'a> {
    key: Option<&'a str>,
    value: String,
    start: usize,
    end: usize,
    /// Start of the value (after `key:`).
    value_start: usize,
}

fn tokenize(input: &str) -> Result<Vec<Token<'_>>, QueryError> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        // An optional `key:` made of ASCII letters or `_`.
        let mut j = i;
        while j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_') {
            j += 1;
        }
        let key = (j > i && j < bytes.len() && bytes[j] == b':').then(|| &input[i..j]);
        let value_start = if key.is_some() { j + 1 } else { i };
        // The value: quoted, or up to the next whitespace (quotes may appear inside).
        let mut value = String::new();
        let mut k = value_start;
        if k < bytes.len() && bytes[k] == b'"' {
            let close = input[k + 1..].find('"').ok_or_else(|| QueryError {
                code: "unclosed_quote",
                value: input[k..].to_owned(),
                start: k,
                end: input.len(),
            })?;
            value.push_str(&input[k + 1..k + 1 + close]);
            k = k + close + 2;
        } else {
            while k < bytes.len() && !bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            value.push_str(&input[value_start..k]);
        }
        out.push(Token {
            key,
            value,
            start,
            end: k,
            value_start,
        });
        i = k;
    }
    Ok(out)
}

/// Parses the search text.
///
/// # Errors
/// [`QueryError`] for any malformed part (F07-AC6).
pub fn parse(input: &str, clock: Clock) -> Result<Query, QueryError> {
    let mut q = Query::default();
    for t in tokenize(input)? {
        let err = |code: &'static str, value: &str| QueryError {
            code,
            value: value.to_owned(),
            start: t.value_start,
            end: t.end,
        };
        let Some(key) = t.key else {
            let folded = names::fold(&t.value);
            if !folded.is_empty() {
                q.terms.push(folded);
            }
            continue;
        };
        let key_lower = key.to_ascii_lowercase();
        if key.len() == 1 {
            // `d:` is the drive D:; `d:report` also searches for "report" on it.
            q.filters
                .push(Filter::Disk(format!("{}:", key.to_ascii_uppercase())));
            let folded = names::fold(&t.value);
            if !folded.is_empty() {
                q.terms.push(folded);
            }
            continue;
        }
        if t.value.is_empty() {
            return Err(QueryError {
                code: "empty_value",
                value: key.to_owned(),
                start: t.start,
                end: t.end,
            });
        }
        let v = t.value.as_str();
        let filter = match key_lower.as_str() {
            "size" | "dim" => parse_size(v).ok_or_else(|| err("bad_size", v))?,
            "ext" => Filter::Ext(
                v.split(',')
                    .map(|e| e.trim().trim_start_matches('.').to_lowercase())
                    .filter(|e| !e.is_empty())
                    .collect(),
            ),
            "type" | "tipo" => {
                let mut cats = Vec::new();
                for part in v.split([',', '|']).filter(|p| !p.is_empty()) {
                    cats.push(category(part).ok_or_else(|| err("bad_type", part))?);
                }
                Filter::Type(cats)
            }
            "modified" | "mod" | "modificato" => {
                parse_modified(v, clock).ok_or_else(|| err("bad_date", v))?
            }
            "disk" | "disco" => Filter::Disk(v.to_owned()),
            "path" | "percorso" => Filter::Path(names::fold(v)),
            "is" => Filter::Is(match v.to_ascii_lowercase().as_str() {
                "dir" | "folder" | "cartella" => Kind::Dir,
                "file" => Kind::File,
                _ => return Err(err("bad_is", v)),
            }),
            "online" => Filter::Online(match v.to_lowercase().as_str() {
                "yes" | "true" | "si" | "sì" => true,
                "no" | "false" => false,
                _ => return Err(err("bad_online", v)),
            }),
            "regex" => {
                regex::RegexBuilder::new(v)
                    .size_limit(1 << 20)
                    .build()
                    .map_err(|e| QueryError {
                        code: "bad_regex",
                        value: e.to_string(),
                        start: t.value_start,
                        end: t.end,
                    })?;
                Filter::Regex(v.to_owned())
            }
            _ => {
                return Err(QueryError {
                    code: "unknown_filter",
                    value: key.to_owned(),
                    start: t.start,
                    end: t.value_start - 1,
                });
            }
        };
        q.filters.push(filter);
    }
    Ok(q)
}

fn category(s: &str) -> Option<FileCategory> {
    use FileCategory as C;
    Some(match s.to_lowercase().as_str() {
        "image" | "images" | "immagine" | "immagini" | "photo" | "foto" => C::Images,
        "video" | "videos" => C::Videos,
        "audio" | "music" | "musica" => C::Audio,
        "document" | "documents" | "documento" | "documenti" | "doc" => C::Documents,
        "archive" | "archives" | "archivio" | "archivi" => C::Archives,
        "disk_image" | "disk_images" | "iso" => C::DiskImages,
        "code" | "codice" => C::Code,
        "program" | "programs" | "programma" | "programmi" | "exe" => C::Programs,
        "other" | "altro" => C::Other,
        _ => return None,
    })
}

/// A size such as `1.5GB`, `10MB`, `700KiB`, `123` (bytes).
fn bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let num: f64 = num.replace(',', ".").parse().ok()?;
    let mult: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1e3,
        "m" | "mb" => 1e6,
        "g" | "gb" => 1e9,
        "t" | "tb" => 1e12,
        "kib" => 1024.0,
        "mib" => 1_048_576.0,
        "gib" => 1_073_741_824.0,
        "tib" => 1_099_511_627_776.0,
        _ => return None,
    };
    let v = num * mult;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (0.0..1.8e19).contains(&v).then(|| v.round() as u64)
}

fn parse_size(v: &str) -> Option<Filter> {
    let (min, max) = if let Some((a, b)) = v.split_once("..") {
        (Some(bytes(a)?), Some(bytes(b)?))
    } else if let Some(x) = v.strip_prefix(">=") {
        (Some(bytes(x)?), None)
    } else if let Some(x) = v.strip_prefix("<=") {
        (None, Some(bytes(x)?))
    } else if let Some(x) = v.strip_prefix('>') {
        (Some(bytes(x)?.saturating_add(1)), None)
    } else if let Some(x) = v.strip_prefix('<') {
        (None, Some(bytes(x)?.checked_sub(1)?))
    } else {
        (Some(bytes(v)?), None)
    };
    if let (Some(a), Some(b)) = (min, max)
        && a > b
    {
        return None;
    }
    Some(Filter::Size { min, max })
}

const DAY_MS: i64 = 86_400_000;

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    days_from_civil(
        if m == 12 { y + 1 } else { y },
        if m == 12 { 1 } else { m + 1 },
        1,
    ) - days_from_civil(y, m, 1)
}

/// A typed date as a span of local days: `2024` (the year), `2024-03` (the month),
/// `2024-03-15` or `15/03/2024` (the day). Returns [start, end) in UTC milliseconds.
fn date_span(s: &str, offset_ms: i64) -> Option<(i64, i64)> {
    let parts: Vec<&str> = if s.contains('/') {
        let mut p: Vec<&str> = s.split('/').collect();
        p.reverse(); // dd/mm/yyyy -> yyyy, mm, dd
        p
    } else {
        s.split('-').collect()
    };
    let nums: Vec<i64> = parts
        .iter()
        .map(|p| p.parse::<i64>().ok())
        .collect::<Option<_>>()?;
    let (start, end) = match nums.as_slice() {
        [y] if (1601..=9999).contains(y) => {
            (days_from_civil(*y, 1, 1), days_from_civil(y + 1, 1, 1))
        }
        [y, m] if (1601..=9999).contains(y) && (1..=12).contains(m) => {
            let s = days_from_civil(*y, *m, 1);
            (s, s + days_in_month(*y, *m))
        }
        [y, m, d]
            if (1601..=9999).contains(y)
                && (1..=12).contains(m)
                && (1..=days_in_month(*y, *m)).contains(d) =>
        {
            let s = days_from_civil(*y, *m, *d);
            (s, s + 1)
        }
        _ => return None,
    };
    Some((start * DAY_MS - offset_ms, end * DAY_MS - offset_ms))
}

/// A duration such as `30d`, `2w`, `6m`, `1y`, `12h`, in milliseconds.
fn duration_ms(s: &str) -> Option<i64> {
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let (num, unit) = s.split_at(split);
    let n: i64 = num.parse().ok()?;
    let unit_ms = match unit.to_ascii_lowercase().as_str() {
        "h" => 3_600_000,
        "d" | "g" => DAY_MS,
        "w" => 7 * DAY_MS,
        "m" => 30 * DAY_MS,
        "y" | "a" => 365 * DAY_MS,
        _ => return None,
    };
    n.checked_mul(unit_ms)
}

fn parse_modified(v: &str, clock: Clock) -> Option<Filter> {
    let offset = i64::from(clock.utc_offset_min) * 60_000;
    let ft = |ms: i64| Some(FileTime::from_unix_millis(ms));
    let (after, before) = if let Some((a, b)) = v.split_once("..") {
        (ft(date_span(a, offset)?.0), ft(date_span(b, offset)?.1))
    } else if let Some((op, rest)) = ["<=", ">=", "<", ">"]
        .iter()
        .find_map(|op| v.strip_prefix(op).map(|r| (*op, r)))
    {
        if let Some(d) = duration_ms(rest) {
            // Relative: `<30d` = less than 30 days ago (recent), `>30d` = older.
            let at = clock.now_ms.checked_sub(d)?;
            match op {
                "<" | "<=" => (ft(at), None),
                _ => (None, ft(at)),
            }
        } else {
            let (start, end) = date_span(rest, offset)?;
            match op {
                ">" => (ft(end), None),
                ">=" => (ft(start), None),
                "<" => (None, ft(start)),
                _ => (None, ft(end)),
            }
        }
    } else {
        let (start, end) = date_span(v, offset)?;
        (ft(start), ft(end))
    };
    Some(Filter::Modified { after, before })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOCK: Clock = Clock {
        now_ms: 1_794_000_000_000, // 2026-11-06 21:20 UTC
        utc_offset_min: 60,
    };

    fn ok(s: &str) -> Query {
        parse(s, CLOCK).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    fn fails(s: &str) -> QueryError {
        parse(s, CLOCK).expect_err(s)
    }

    fn day_ms(y: i64, m: i64, d: i64) -> i64 {
        days_from_civil(y, m, d) * DAY_MS - 3_600_000
    }

    #[test]
    fn f07_ac2_words_are_folded_and_all_required() {
        let q = ok("  Report  \"Città Vecchia\" 2024 ");
        assert_eq!(
            q.terms,
            ["REPORT", "CITTÀ VECCHIA", "2024"].map(names::fold)
        );
        assert!(q.filters.is_empty());
        assert!(ok("   ").is_empty());
    }

    #[test]
    fn f07_ac5_sizes() {
        let size = |s: &str| ok(s).filters[0].clone();
        assert_eq!(
            size("size:>1GB"),
            Filter::Size {
                min: Some(1_000_000_001),
                max: None
            }
        );
        assert_eq!(
            size("size:>=1GB"),
            Filter::Size {
                min: Some(1_000_000_000),
                max: None
            }
        );
        assert_eq!(
            size("size:<10MB"),
            Filter::Size {
                min: None,
                max: Some(9_999_999)
            }
        );
        assert_eq!(
            size("size:10MB..1GB"),
            Filter::Size {
                min: Some(10_000_000),
                max: Some(1_000_000_000)
            }
        );
        assert_eq!(
            size("size:1,5GB"),
            Filter::Size {
                min: Some(1_500_000_000),
                max: None
            }
        );
        assert_eq!(
            size("size:2MiB"),
            Filter::Size {
                min: Some(2_097_152),
                max: None
            }
        );
        assert_eq!(
            size("SIZE:700"),
            Filter::Size {
                min: Some(700),
                max: None
            }
        );
        for bad in ["size:>big", "size:1XB", "size:2GB..1GB", "size:<0"] {
            let e = fails(bad);
            assert_eq!(e.code, "bad_size", "{bad}");
            assert_eq!(e.start, 5, "{bad}");
        }
    }

    #[test]
    fn f07_ac5_extensions_types_kinds_online() {
        let q = ok("ext:PDF,.docx, type:image|video is:dir online:no");
        assert_eq!(
            q.filters,
            [
                Filter::Ext(vec!["pdf".into(), "docx".into()]),
                Filter::Type(vec![FileCategory::Images, FileCategory::Videos]),
                Filter::Is(Kind::Dir),
                Filter::Online(false),
            ]
        );
        assert_eq!(
            ok("tipo:documenti").filters,
            [Filter::Type(vec![FileCategory::Documents])]
        );
        assert_eq!(fails("type:spreadsheet").code, "bad_type");
        assert_eq!(fails("is:link").code, "bad_is");
        assert_eq!(fails("online:maybe").code, "bad_online");
    }

    #[test]
    fn f07_ac5_dates_absolute_and_relative() {
        let m = |s: &str| ok(s).filters[0].clone();
        let at = |ms: i64| Some(FileTime::from_unix_millis(ms));
        // `>` a day = after that local day; `<` = before it.
        assert_eq!(
            m("modified:>2024-01-01"),
            Filter::Modified {
                after: at(day_ms(2024, 1, 2)),
                before: None
            }
        );
        assert_eq!(
            m("modified:>=2024-01-01"),
            Filter::Modified {
                after: at(day_ms(2024, 1, 1)),
                before: None
            }
        );
        assert_eq!(
            m("modified:<2024-01-01"),
            Filter::Modified {
                after: None,
                before: at(day_ms(2024, 1, 1))
            }
        );
        assert_eq!(
            m("modified:2024"),
            Filter::Modified {
                after: at(day_ms(2024, 1, 1)),
                before: at(day_ms(2025, 1, 1))
            }
        );
        assert_eq!(
            m("modified:2024-02"),
            Filter::Modified {
                after: at(day_ms(2024, 2, 1)),
                before: at(day_ms(2024, 3, 1))
            }
        );
        assert_eq!(
            m("modified:15/03/2024"),
            Filter::Modified {
                after: at(day_ms(2024, 3, 15)),
                before: at(day_ms(2024, 3, 16))
            }
        );
        assert_eq!(
            m("modified:2024-01-01..2024-06-30"),
            Filter::Modified {
                after: at(day_ms(2024, 1, 1)),
                before: at(day_ms(2024, 7, 1))
            }
        );
        // Relative: `<30d` recent, `>1y` old.
        assert_eq!(
            m("modified:<30d"),
            Filter::Modified {
                after: at(CLOCK.now_ms - 30 * DAY_MS),
                before: None
            }
        );
        assert_eq!(
            m("modified:>1y"),
            Filter::Modified {
                after: None,
                before: at(CLOCK.now_ms - 365 * DAY_MS)
            }
        );
        for bad in [
            "modified:yesterday",
            "modified:2024-13-01",
            "modified:2023-02-29",
            "modified:>3x",
        ] {
            assert_eq!(fails(bad).code, "bad_date", "{bad}");
        }
    }

    #[test]
    fn f07_ac5_disk_path_regex_and_drive_letters() {
        let q = ok("disk:\"Backup Foto\" path:Documenti regex:^IMG_\\d+ d: e:report");
        assert_eq!(
            q.filters,
            [
                Filter::Disk("Backup Foto".into()),
                Filter::Path(names::fold("Documenti")),
                Filter::Regex("^IMG_\\d+".into()),
                Filter::Disk("D:".into()),
                Filter::Disk("E:".into()),
            ]
        );
        assert_eq!(q.terms, [names::fold("report")]);
    }

    // F07-AC6: mistakes are reported with their position, never ignored.
    #[test]
    fn f07_ac6_errors_point_at_the_mistake() {
        let e = fails("report colour:red");
        assert_eq!(
            (e.code, e.value.as_str(), e.start, e.end),
            ("unknown_filter", "colour", 7, 13)
        );
        let e = fails("disk:\"Backup Foto");
        assert_eq!((e.code, e.start), ("unclosed_quote", 5));
        let e = fails("ext: report");
        assert_eq!((e.code, e.value.as_str()), ("empty_value", "ext"));
        let e = fails("regex:(unclosed");
        assert_eq!(e.code, "bad_regex");
        assert_eq!(e.start, 6);
    }

    #[test]
    fn calendar() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 12), 31);
    }
}
