// Static file serving and directory listing

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Find the first existing index file from `candidates` in `dir`.
pub fn resolve_index(dir: &Path, candidates: &[String]) -> Option<PathBuf> {
    for name in candidates {
        let path = dir.join(name);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Build a quoted ETag from file size and last-modified timestamp.
pub fn build_etag(size: u64, modified_secs: u64) -> String {
    format!("\"{:x}-{:x}\"", size, modified_secs)
}

/// Format an HTTP date from a SystemTime (RFC 7231 format).
pub fn http_date(time: SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    let secs = time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = ["Sun","Mon","Tue","Wed","Thu","Fri","Sat"];
    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    let (wday, day, month, year, hour, min, sec) = epoch_to_date(secs);
    format!("{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        days[wday], day, months[month], year, hour, min, sec)
}

fn epoch_to_date(secs: u64) -> (usize, u32, usize, u32, u32, u32, u32) {
    let sec = (secs % 60) as u32;
    let min = ((secs / 60) % 60) as u32;
    let hour = ((secs / 3600) % 24) as u32;
    let days_total = secs / 86400;
    let wday = ((days_total + 4) % 7) as usize;
    let mut year = 1970u32;
    let mut remaining = days_total;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        year += 1;
    }
    let month_days: [u64; 12] = [31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    for &d in &month_days {
        if remaining < d { break; }
        remaining -= d;
        month += 1;
    }
    let day = (remaining + 1) as u32;
    (wday, day, month, year, hour, min, sec)
}

fn is_leap(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Render an HTML directory listing for `dir` at request path `req_path`.
pub fn format_directory_listing(dir: &Path, req_path: &str) -> std::io::Result<String> {
    let mut entries: Vec<(String, bool, u64, String)> = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue; // hide dotfiles
        }
        let meta = entry.metadata()?;
        let is_dir = meta.is_dir();
        let size = if is_dir { 0 } else { meta.len() };
        let modified = meta.modified()
            .map(http_date)
            .unwrap_or_else(|_| "-".to_string());
        entries.push((name, is_dir, size, modified));
    }

    entries.sort_by(|a, b| {
        match (a.1, b.1) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.cmp(&b.0),
        }
    });

    let title = format!("Index of {}", req_path);
    let mut rows = String::new();
    if req_path != "/" {
        rows.push_str("<tr><td><a href=\"../\">../</a></td><td>-</td><td>-</td></tr>\n");
    }
    for (name, is_dir, size, modified) in &entries {
        let href = if *is_dir { format!("{}/", name) } else { name.clone() };
        let display = if *is_dir { format!("{}/", name) } else { name.clone() };
        let size_str = if *is_dir { "-".to_string() } else { format!("{}", size) };
        rows.push_str(&format!(
            "<tr><td><a href=\"{}\">{}</a></td><td>{}</td><td>{}</td></tr>\n",
            href, display, size_str, modified
        ));
    }

    Ok(format!(r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>{title}</title>
<style>body{{font-family:monospace;padding:1em}}table{{border-collapse:collapse}}
td{{padding:2px 12px}}th{{text-align:left;padding:2px 12px;border-bottom:1px solid #ccc}}</style>
</head><body>
<h1>{title}</h1>
<table><tr><th>Name</th><th>Size</th><th>Last Modified</th></tr>
{rows}</table>
</body></html>"#, title = title, rows = rows))
}

/// Parse a Range header value like "bytes=0-1023" into (start, end_inclusive).
pub fn parse_range(header: &str, file_size: u64) -> Option<(u64, u64)> {
    let header = header.trim();
    let bytes = header.strip_prefix("bytes=")?;
    let (start_str, end_str) = bytes.split_once('-')?;

    let (start, end): (u64, u64) = if start_str.is_empty() {
        // Suffix form: bytes=-N means last N bytes
        let suffix: u64 = end_str.parse().ok()?;
        let start = file_size.saturating_sub(suffix);
        (start, file_size - 1)
    } else if end_str.is_empty() {
        // Open-ended form: bytes=N- means from N to end
        let start: u64 = start_str.parse().ok()?;
        (start, file_size - 1)
    } else {
        // Standard form: bytes=N-M
        let start: u64 = start_str.parse().ok()?;
        let end: u64 = end_str.parse().ok()?;
        (start, end)
    };

    if start > end || end >= file_size {
        return None;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_resolve_index_finds_html() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("index.html"), b"hello").unwrap();
        let candidates = vec!["index.html".to_string(), "index.php".to_string()];
        let result = resolve_index(tmp.path(), &candidates);
        assert_eq!(result, Some(tmp.path().join("index.html")));
    }

    #[test]
    fn test_resolve_index_prefers_first_match() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("index.html"), b"a").unwrap();
        fs::write(tmp.path().join("index.php"), b"b").unwrap();
        let candidates = vec!["index.php".to_string(), "index.html".to_string()];
        let result = resolve_index(tmp.path(), &candidates);
        assert_eq!(result, Some(tmp.path().join("index.php")));
    }

    #[test]
    fn test_resolve_index_returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        let candidates = vec!["index.html".to_string()];
        let result = resolve_index(tmp.path(), &candidates);
        assert_eq!(result, None);
    }

    #[test]
    fn test_build_etag() {
        let tag1 = build_etag(1000, 12345);
        let tag2 = build_etag(1000, 12345);
        let tag3 = build_etag(2000, 12345);
        assert_eq!(tag1, tag2);
        assert_ne!(tag1, tag3);
        assert!(tag1.starts_with('"') && tag1.ends_with('"'));
    }

    #[test]
    fn test_format_directory_listing_contains_filename() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), b"world").unwrap();
        let html = format_directory_listing(tmp.path(), "/").unwrap();
        assert!(html.contains("hello.txt"));
    }

    #[test]
    fn test_parse_range_basic() {
        assert_eq!(parse_range("bytes=0-499", 1000), Some((0, 499)));
        assert_eq!(parse_range("bytes=500-999", 1000), Some((500, 999)));
    }

    #[test]
    fn test_parse_range_open_end() {
        assert_eq!(parse_range("bytes=100-", 200), Some((100, 199)));
    }

    #[test]
    fn test_parse_range_suffix() {
        assert_eq!(parse_range("bytes=-100", 200), Some((100, 199)));
    }

    #[test]
    fn test_parse_range_invalid() {
        assert_eq!(parse_range("bytes=900-999", 100), None); // out of bounds
        assert_eq!(parse_range("invalid", 100), None);
    }
}
