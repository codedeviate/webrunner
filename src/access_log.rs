use axum::extract::{ConnectInfo, State};
use axum::http::{header, Request};
use axum::middleware::Next;
use axum::response::Response;
use std::fs::OpenOptions;
use std::io::{stdout, Write};
use std::net::{IpAddr, SocketAddr};
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
    pub fn write_line(&self, line: &str) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = writeln!(w, "{}", line);
            let _ = w.flush();
        }
    }
}

/// Typed marker placed in response extensions by the request handler
/// when a request passed webrunner-managed `.htpasswd` auth. The
/// access-log middleware reads it to populate the `%u` field.
#[derive(Clone, Debug)]
pub struct AuthUser(pub String);

/// Format a Combined Log Format line.
///
/// Spec: `%h - %u [%t] "%r" %>s %b "%{Referer}i" "%{User-Agent}i"`.
/// `%l` (ident) is always `-`. `%b` is `-` when `body_size == 0`.
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

pub async fn middleware(
    State(state): State<crate::handler::AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let Some(log) = state.access_log.clone() else {
        return next.run(req).await;
    };

    let method = req.method().to_string();
    let uri = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();
    let version = format!("{:?}", req.version());
    let referer = req
        .headers()
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let user_agent = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let response = next.run(req).await;

    let auth_user = response
        .extensions()
        .get::<AuthUser>()
        .map(|u| u.0.clone());

    let status = response.status().as_u16();
    let body_size = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let line = format_combined(
        peer.ip(),
        auth_user.as_deref(),
        &method,
        &uri,
        &version,
        status,
        body_size,
        referer.as_deref(),
        user_agent.as_deref(),
    );
    log.write_line(&line);

    response
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

    #[test]
    fn auth_user_round_trips_through_response_extensions() {
        use axum::body::Body;
        use axum::http::Response;

        let mut response: Response<Body> = Response::new(Body::empty());
        response.extensions_mut().insert(AuthUser("alice".to_string()));

        let read = response.extensions().get::<AuthUser>().map(|u| u.0.as_str());
        assert_eq!(read, Some("alice"));
    }
}
