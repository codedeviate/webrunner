// CGI dispatcher

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub struct CgiOutput {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Returns true if `ext` is a CGI-handled file extension.
///
/// `pl` and `php` are always treated as CGI UNLESS listed in `disabled`
/// (from `--no-cgi`). `extra` is the per-process opt-in set (from `--cgi`).
/// Both `extra` and `disabled` entries MUST already be lowercase;
/// `CliConfig::validate` enforces this. `extra` and `disabled` MUST NOT
/// share any entry; `CliConfig::validate` enforces that too.
pub fn is_cgi_ext(ext: &str, extra: &[String], disabled: &[String]) -> bool {
    let e = ext.to_lowercase();
    if disabled.iter().any(|x| x == &e) {
        return false;
    }
    matches!(e.as_str(), "pl" | "php") || extra.iter().any(|x| x == &e)
}

pub fn interpreter_for(ext: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match ext.to_lowercase().as_str() {
        "pl" => Some(("perl", vec![])),
        "php" => Some(("php", vec![])),
        "ts" | "js" => Some(("bun", vec!["run"])),
        _ => None,
    }
}

pub fn parse_cgi_output(raw: &[u8]) -> CgiOutput {
    // Try to find the blank line separator: \r\n\r\n first, then \n\n
    let (header_bytes, body_bytes) = if let Some(pos) = find_subsequence(raw, b"\r\n\r\n") {
        (&raw[..pos], &raw[pos + 4..])
    } else if let Some(pos) = find_subsequence(raw, b"\n\n") {
        (&raw[..pos], &raw[pos + 2..])
    } else {
        eprintln!("[CGI ERROR] No blank line found in CGI output");
        return CgiOutput {
            status: 500,
            headers: vec![],
            body: raw.to_vec(),
        };
    };

    let header_str = String::from_utf8_lossy(header_bytes);
    let mut status: u16 = 200;
    let mut headers: Vec<(String, String)> = Vec::new();

    for line in header_str.lines() {
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_lowercase();
            let value = line[colon_pos + 1..].trim().to_string();
            if key == "status" {
                // Parse "404 Not Found" → 404
                if let Some(code_str) = value.split_whitespace().next() {
                    if let Ok(code) = code_str.parse::<u16>() {
                        status = code;
                    }
                }
            } else {
                headers.push((key, value));
            }
        }
    }

    CgiOutput {
        status,
        headers,
        body: body_bytes.to_vec(),
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

pub fn build_cgi_env(
    method: &str,
    path: &str,
    query: &str,
    script_path: &str,
    script_name: &str,
    server_name: &str,
    server_port: &str,
    remote_addr: &str,
    content_type: &str,
    content_length: &str,
    http_headers: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = vec![
        ("REQUEST_METHOD".to_string(), method.to_string()),
        ("QUERY_STRING".to_string(), query.to_string()),
        ("CONTENT_TYPE".to_string(), content_type.to_string()),
        ("CONTENT_LENGTH".to_string(), content_length.to_string()),
        ("PATH_INFO".to_string(), "".to_string()),
        ("PATH_TRANSLATED".to_string(), "".to_string()),
        ("SCRIPT_FILENAME".to_string(), script_path.to_string()),
        ("SCRIPT_NAME".to_string(), script_name.to_string()),
        ("SERVER_NAME".to_string(), server_name.to_string()),
        ("SERVER_PORT".to_string(), server_port.to_string()),
        ("SERVER_PROTOCOL".to_string(), "HTTP/1.1".to_string()),
        ("SERVER_SOFTWARE".to_string(), format!("webrunner/{}", env!("CARGO_PKG_VERSION"))),
        ("GATEWAY_INTERFACE".to_string(), "CGI/1.1".to_string()),
        ("REMOTE_ADDR".to_string(), remote_addr.to_string()),
        ("REDIRECT_STATUS".to_string(), "200".to_string()),
    ];

    for (key, value) in http_headers {
        let env_key = format!("HTTP_{}", key.to_uppercase().replace('-', "_"));
        env.push((env_key, value.clone()));
    }

    env
}

/// Run a CGI script and capture its response.
///
/// `timeout_secs` bounds the script's wall-clock runtime: when it elapses,
/// the child is SIGKILL'd (via `kill_on_drop`) and `504 Gateway Timeout`
/// is returned. A value of `0` disables the timeout entirely — the script
/// is allowed to run to completion.
pub async fn run_cgi(
    script_path: &Path,
    ext: &str,
    env_vars: Vec<(String, String)>,
    stdin_body: Vec<u8>,
    timeout_secs: u64,
) -> CgiOutput {
    let (interpreter, args) = match interpreter_for(ext) {
        Some(v) => v,
        None => {
            eprintln!("[CGI ERROR] No interpreter for extension: {}", ext);
            return CgiOutput {
                status: 500,
                headers: vec![],
                body: b"Internal Server Error".to_vec(),
            };
        }
    };

    let script_dir = script_path.parent().unwrap_or(Path::new("."));

    let mut cmd = Command::new(interpreter);
    for arg in &args {
        cmd.arg(arg);
    }
    cmd.arg(script_path);
    cmd.current_dir(script_dir);
    cmd.env_clear();
    cmd.kill_on_drop(true);
    // Preserve PATH so interpreters can be located
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    for (key, value) in &env_vars {
        cmd.env(key, value);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let run = async move {
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[CGI ERROR] Failed to spawn {}: {}", interpreter, e);
                return CgiOutput {
                    status: 500,
                    headers: vec![],
                    body: b"Internal Server Error".to_vec(),
                };
            }
        };

        // Write stdin if needed
        if !stdin_body.is_empty() {
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(&stdin_body).await;
            }
        } else {
            drop(child.stdin.take());
        }

        let mut stdout_handle = child.stdout.take().expect("stdout piped");
        let mut stderr_handle = child.stderr.take().expect("stderr piped");

        let (stdout_bytes, stderr_bytes) = tokio::join!(
            async {
                let mut buf = Vec::new();
                let _ = stdout_handle.read_to_end(&mut buf).await;
                buf
            },
            async {
                let mut buf = Vec::new();
                let _ = stderr_handle.read_to_end(&mut buf).await;
                buf
            }
        );

        let _ = child.wait().await;

        if !stderr_bytes.is_empty() {
            let stderr_str = String::from_utf8_lossy(&stderr_bytes);
            for line in stderr_str.lines() {
                eprintln!("[CGI ERROR] {}", line);
            }
        }

        parse_cgi_output(&stdout_bytes)
    };

    if timeout_secs == 0 {
        run.await
    } else {
        match tokio::time::timeout(Duration::from_secs(timeout_secs), run).await {
            Ok(output) => output,
            Err(_) => {
                eprintln!(
                    "[CGI ERROR] Script timed out after {} seconds: {}",
                    timeout_secs,
                    script_path.display(),
                );
                CgiOutput {
                    status: 504,
                    headers: vec![],
                    body: b"Gateway Timeout".to_vec(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_cgi_ext_defaults() {
        let empty: Vec<String> = Vec::new();
        assert!(is_cgi_ext("pl", &empty, &empty));
        assert!(is_cgi_ext("php", &empty, &empty));
        assert!(is_cgi_ext("PHP", &empty, &empty));
        assert!(!is_cgi_ext("js", &empty, &empty));
        assert!(!is_cgi_ext("ts", &empty, &empty));
        assert!(!is_cgi_ext("html", &empty, &empty));
        assert!(!is_cgi_ext("css", &empty, &empty));
    }

    #[test]
    fn test_is_cgi_ext_with_extras() {
        let empty: Vec<String> = Vec::new();
        let extras = vec!["js".to_string(), "ts".to_string()];
        assert!(is_cgi_ext("js", &extras, &empty));
        assert!(is_cgi_ext("ts", &extras, &empty));
        assert!(is_cgi_ext("JS", &extras, &empty));
        assert!(is_cgi_ext("pl", &extras, &empty));
        assert!(!is_cgi_ext("html", &extras, &empty));
        // Contract: extras must already be lowercased by the caller.
        // An un-normalised entry will NOT match (validate() prevents this in production).
        let upper_extras = vec!["JS".to_string()];
        assert!(!is_cgi_ext("js", &upper_extras, &empty));
    }

    #[test]
    fn test_is_cgi_ext_disabled_overrides_default() {
        let empty: Vec<String> = Vec::new();
        let disabled = vec!["pl".to_string()];
        assert!(!is_cgi_ext("pl", &empty, &disabled));
    }

    #[test]
    fn test_is_cgi_ext_disabled_one_doesnt_affect_other() {
        let empty: Vec<String> = Vec::new();
        let disabled = vec!["pl".to_string()];
        assert!(is_cgi_ext("php", &empty, &disabled));
    }

    #[test]
    fn test_is_cgi_ext_disabled_case_insensitive_input() {
        let empty: Vec<String> = Vec::new();
        let disabled = vec!["pl".to_string()];
        assert!(!is_cgi_ext("PL", &empty, &disabled));
    }

    #[test]
    fn test_interpreter_for() {
        assert_eq!(interpreter_for("pl"), Some(("perl", vec![])));
        assert_eq!(interpreter_for("php"), Some(("php", vec![])));
        assert_eq!(interpreter_for("ts"), Some(("bun", vec!["run"])));
        assert_eq!(interpreter_for("js"), Some(("bun", vec!["run"])));
        assert_eq!(interpreter_for("html"), None);
    }

    #[test]
    fn test_parse_cgi_output_basic() {
        let output = b"Content-Type: text/html\r\n\r\n<html>Hello</html>";
        let parsed = parse_cgi_output(output);
        assert_eq!(parsed.status, 200);
        assert!(parsed.headers.iter().any(|(k, v)| k == "content-type" && v == "text/html"));
        assert_eq!(parsed.body, b"<html>Hello</html>");
    }

    #[test]
    fn test_parse_cgi_output_with_status() {
        let output = b"Status: 404 Not Found\r\nContent-Type: text/plain\r\n\r\nNot found";
        let parsed = parse_cgi_output(output);
        assert_eq!(parsed.status, 404);
        assert_eq!(parsed.body, b"Not found");
    }

    #[test]
    fn test_parse_cgi_output_lf_only() {
        let output = b"Content-Type: text/plain\n\nHello";
        let parsed = parse_cgi_output(output);
        assert_eq!(parsed.status, 200);
        assert_eq!(parsed.body, b"Hello");
    }

    #[test]
    fn test_parse_cgi_output_malformed_no_blank_line() {
        let output = b"Content-Type: text/plain";
        let parsed = parse_cgi_output(output);
        assert_eq!(parsed.status, 500);
    }
}
