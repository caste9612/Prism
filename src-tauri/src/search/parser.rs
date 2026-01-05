//! Query parser for advanced search syntax
//!
//! Supports filters like:
//! - size:>1MB, size:<100KB, size:1GB-5GB
//! - ext:pdf, ext:jpg,png,gif
//! - date:>2024-01-01, date:<2024-12-31
//! - path:Documents, path:/Users/
//! - type:image, type:video, type:document

/// Parsed search query with extracted filters
#[derive(Debug, Clone, Default)]
pub struct ParsedQuery {
    /// The main search text (for FTS)
    pub text: String,
    /// Extracted filters
    pub filters: Vec<QueryFilter>,
}

/// Individual filter from the query
#[derive(Debug, Clone)]
pub enum QueryFilter {
    /// File size filter (in bytes)
    Size(SizeFilter),
    /// File extension filter
    Extension(Vec<String>),
    /// Date filter (Unix timestamp)
    Date(DateFilter),
    /// Path contains filter
    PathContains(String),
    /// File type category
    FileType(FileTypeCategory),
}

#[derive(Debug, Clone)]
pub enum SizeFilter {
    GreaterThan(u64),
    LessThan(u64),
    Between(u64, u64),
    Equals(u64),
}

#[derive(Debug, Clone)]
pub enum DateFilter {
    After(i64),
    Before(i64),
    Between(i64, i64),
}

#[derive(Debug, Clone)]
pub enum FileTypeCategory {
    Image,
    Video,
    Audio,
    Document,
    Archive,
    Code,
}

impl FileTypeCategory {
    pub fn extensions(&self) -> Vec<&'static str> {
        match self {
            FileTypeCategory::Image => vec!["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff"],
            FileTypeCategory::Video => vec!["mp4", "mkv", "avi", "mov", "wmv", "webm", "flv", "m4v"],
            FileTypeCategory::Audio => vec!["mp3", "wav", "flac", "ogg", "aac", "wma", "m4a"],
            FileTypeCategory::Document => vec!["pdf", "doc", "docx", "txt", "rtf", "odt", "xls", "xlsx", "ppt", "pptx"],
            FileTypeCategory::Archive => vec!["zip", "rar", "7z", "tar", "gz", "bz2", "xz"],
            FileTypeCategory::Code => vec!["js", "ts", "py", "rs", "go", "java", "c", "cpp", "h", "hpp", "cs", "rb", "php"],
        }
    }
}

/// Parse a search query string into structured components
pub fn parse_query(input: &str) -> ParsedQuery {
    let mut query = ParsedQuery::default();
    let mut text_parts = Vec::new();

    // Simple tokenization - split by spaces, respecting quotes
    let tokens = tokenize(input);

    for token in tokens {
        if let Some(filter) = parse_filter(&token) {
            query.filters.push(filter);
        } else {
            text_parts.push(token);
        }
    }

    query.text = text_parts.join(" ");
    query
}

/// Tokenize input, respecting quoted strings
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Try to parse a token as a filter
fn parse_filter(token: &str) -> Option<QueryFilter> {
    let parts: Vec<&str> = token.splitn(2, ':').collect();
    if parts.len() != 2 {
        return None;
    }

    let key = parts[0].to_lowercase();
    let value = parts[1];

    match key.as_str() {
        "size" => parse_size_filter(value).map(QueryFilter::Size),
        "ext" | "extension" => Some(QueryFilter::Extension(
            value.split(',').map(|s| s.trim().to_lowercase()).collect()
        )),
        "date" | "modified" => parse_date_filter(value).map(QueryFilter::Date),
        "path" | "in" => Some(QueryFilter::PathContains(value.to_string())),
        "type" => parse_type_filter(value).map(QueryFilter::FileType),
        _ => None,
    }
}

/// Parse size filter (e.g., ">1MB", "<100KB", "1GB-5GB")
fn parse_size_filter(value: &str) -> Option<SizeFilter> {
    if value.contains('-') {
        // Range: "1GB-5GB"
        let parts: Vec<&str> = value.split('-').collect();
        if parts.len() == 2 {
            let min = parse_size(parts[0])?;
            let max = parse_size(parts[1])?;
            return Some(SizeFilter::Between(min, max));
        }
    } else if value.starts_with('>') {
        let size = parse_size(&value[1..])?;
        return Some(SizeFilter::GreaterThan(size));
    } else if value.starts_with('<') {
        let size = parse_size(&value[1..])?;
        return Some(SizeFilter::LessThan(size));
    } else {
        let size = parse_size(value)?;
        return Some(SizeFilter::Equals(size));
    }
    None
}

/// Parse size string (e.g., "1MB", "500KB", "2GB")
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();

    let (num_str, multiplier) = if s.ends_with("TB") {
        (&s[..s.len()-2], 1024u64 * 1024 * 1024 * 1024)
    } else if s.ends_with("GB") {
        (&s[..s.len()-2], 1024u64 * 1024 * 1024)
    } else if s.ends_with("MB") {
        (&s[..s.len()-2], 1024u64 * 1024)
    } else if s.ends_with("KB") {
        (&s[..s.len()-2], 1024u64)
    } else if s.ends_with("B") {
        (&s[..s.len()-1], 1u64)
    } else {
        // Assume bytes if no suffix
        (s.as_str(), 1u64)
    };

    num_str.trim().parse::<f64>().ok().map(|n| (n * multiplier as f64) as u64)
}

/// Parse date filter (e.g., ">2024-01-01", "<2024-12-31")
fn parse_date_filter(value: &str) -> Option<DateFilter> {
    if value.contains('-') && !value.starts_with('>') && !value.starts_with('<') {
        // Could be a date range or a date
        // For now, just try to parse as a date
        if let Some(ts) = parse_date(value) {
            return Some(DateFilter::After(ts));
        }
    }

    if value.starts_with('>') {
        let ts = parse_date(&value[1..])?;
        return Some(DateFilter::After(ts));
    } else if value.starts_with('<') {
        let ts = parse_date(&value[1..])?;
        return Some(DateFilter::Before(ts));
    } else {
        let ts = parse_date(value)?;
        // Exact date means that day
        return Some(DateFilter::After(ts));
    }
}

/// Parse date string to Unix timestamp
fn parse_date(s: &str) -> Option<i64> {
    // Simple YYYY-MM-DD parsing
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() == 3 {
        let year: i32 = parts[0].parse().ok()?;
        let month: u32 = parts[1].parse().ok()?;
        let day: u32 = parts[2].parse().ok()?;

        // Simple calculation (not accounting for leap years perfectly)
        use chrono::{NaiveDate, TimeZone, Utc};
        let date = NaiveDate::from_ymd_opt(year, month, day)?;
        let datetime = date.and_hms_opt(0, 0, 0)?;
        return Some(Utc.from_utc_datetime(&datetime).timestamp());
    }
    None
}

/// Parse file type category
fn parse_type_filter(value: &str) -> Option<FileTypeCategory> {
    match value.to_lowercase().as_str() {
        "image" | "images" | "img" | "photo" | "photos" => Some(FileTypeCategory::Image),
        "video" | "videos" | "movie" | "movies" => Some(FileTypeCategory::Video),
        "audio" | "music" | "sound" => Some(FileTypeCategory::Audio),
        "document" | "documents" | "doc" | "docs" => Some(FileTypeCategory::Document),
        "archive" | "archives" | "zip" | "compressed" => Some(FileTypeCategory::Archive),
        "code" | "source" | "programming" => Some(FileTypeCategory::Code),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_query() {
        let query = parse_query("hello world");
        assert_eq!(query.text, "hello world");
        assert!(query.filters.is_empty());
    }

    #[test]
    fn test_parse_size_filter() {
        let query = parse_query("document size:>1MB");
        assert_eq!(query.text, "document");
        assert_eq!(query.filters.len(), 1);

        if let QueryFilter::Size(SizeFilter::GreaterThan(size)) = &query.filters[0] {
            assert_eq!(*size, 1024 * 1024);
        } else {
            panic!("Expected size filter");
        }
    }

    #[test]
    fn test_parse_extension_filter() {
        let query = parse_query("ext:pdf,docx report");
        assert_eq!(query.text, "report");
        assert_eq!(query.filters.len(), 1);

        if let QueryFilter::Extension(exts) = &query.filters[0] {
            assert_eq!(exts, &vec!["pdf", "docx"]);
        } else {
            panic!("Expected extension filter");
        }
    }

    #[test]
    fn test_parse_type_filter() {
        let query = parse_query("type:image vacation");
        assert_eq!(query.text, "vacation");
        assert_eq!(query.filters.len(), 1);
    }

    #[test]
    fn test_parse_size_units() {
        assert_eq!(parse_size("1KB"), Some(1024));
        assert_eq!(parse_size("1MB"), Some(1024 * 1024));
        assert_eq!(parse_size("1GB"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size("1.5GB"), Some((1.5 * 1024.0 * 1024.0 * 1024.0) as u64));
    }
}
