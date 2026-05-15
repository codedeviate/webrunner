use std::fs::OpenOptions;
use std::io::{stdout, Write};
use std::net::IpAddr;
use std::sync::Mutex;

use time::macros::format_description;
use time::OffsetDateTime;

const APACHE_TIME: &[time::format_description::FormatItem<'_>] = format_description!(
    "[day]/[month repr:short]/[year]:[hour]:[minute]:[second] +0000"
);

pub struct AccessLog {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl std::fmt::Debug for AccessLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessLog").finish_non_exhaustive()
    }
}

impl AccessLog {
    /// Open the access log sink. `target == "-"` selects stdout; any
    /// other value is opened as a file in append+create mode.
    #[allow(dead_code)] // wired in Task 4
    pub fn open(target: &str) -> Result<Self, String> {
        let writer: Box<dyn Write + Send> = if target == "-" {
            Box::new(stdout())
        } else {
            let f = OpenOptions::new()
                .append(true)
                .create(true)
                .open(target)
                .map_err(|e| format!("cannot open access log '{}': {}", target, e))?;
            Box::new(f)
        };
        Ok(AccessLog {
            writer: Mutex::new(writer),
        })
    }

    /// Write one log line. Errors are swallowed — a dev tool should not
    /// crash on disk-full or a closed stdout.
    #[allow(dead_code)] // wired in Task 4
    pub fn write_line(&self, line: &str) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = writeln!(w, "{}", line);
            let _ = w.flush();
        }
    }
}

/// Format a Combined Log Format line.
///
/// Spec: `%h - %u [%t] "%r" %>s %b "%{Referer}i" "%{User-Agent}i"`.
/// `%l` (ident) is always `-`. `%b` is `-` when `body_size == 0`.
#[allow(dead_code)] // wired in Task 4
#[allow(clippy::too_many_arguments)] // mirrors Combined Log Format field set
pub fn format_combined(
    remote_ip: IpAddr,
    auth_user: Option<&str>,
    method: &str,
    uri: &str,
    version: &str,
    status: u16,
    body_size: u64,
    referer: Option<&str>,
    user_agent: Option<&str>,
) -> String {
    let time_str = OffsetDateTime::now_utc()
        .format(APACHE_TIME)
        .unwrap_or_else(|_| "-".to_string());
    let size_field = if body_size == 0 {
        "-".to_string()
    } else {
        body_size.to_string()
    };
    format!(
        "{} - {} [{}] \"{} {} {}\" {} {} \"{}\" \"{}\"",
        remote_ip,
        auth_user.unwrap_or("-"),
        time_str,
        method,
        uri,
        version,
        status,
        size_field,
        referer.unwrap_or("-"),
        user_agent.unwrap_or("-"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::Ipv4Addr;

    #[test]
    fn test_open_dash_returns_stdout_sink() {
        let result = AccessLog::open("-");
        assert!(result.is_ok());
    }

    #[test]
    fn test_open_file_creates_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("access.log");
        let path_str = path.to_str().unwrap();

        {
            let log = AccessLog::open(path_str).unwrap();
            log.write_line("first line");
        }
        {
            let log = AccessLog::open(path_str).unwrap();
            log.write_line("second line");
        }

        let mut contents = String::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        assert_eq!(contents, "first line\nsecond line\n");
    }

    #[test]
    fn test_open_invalid_path_errors() {
        let result = AccessLog::open("/nope/no/such/dir/wr-access.log");
        let err = result.unwrap_err();
        assert!(err.contains("/nope/no/such/dir/wr-access.log"));
        assert!(err.contains("cannot open"));
    }

    #[test]
    fn test_format_combined_minimal() {
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let line = format_combined(ip, None, "GET", "/", "HTTP/1.1", 200, 0, None, None);
        assert!(line.starts_with("127.0.0.1 - - ["));
        assert!(line.contains("\"GET / HTTP/1.1\" 200 -"));
        assert!(line.ends_with("\"-\" \"-\""));
    }

    #[test]
    fn test_format_combined_full() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42));
        let line = format_combined(
            ip,
            Some("alice"),
            "POST",
            "/api/data?id=7",
            "HTTP/1.1",
            201,
            1024,
            Some("https://example.com/"),
            Some("Mozilla/5.0"),
        );
        assert!(line.starts_with("192.168.1.42 - alice ["));
        assert!(line.contains("\"POST /api/data?id=7 HTTP/1.1\" 201 1024"));
        assert!(line.ends_with("\"https://example.com/\" \"Mozilla/5.0\""));
    }
}
