use std::collections::HashMap;
use std::path::Path;

/// Merged configuration from all applicable .htaccess files for a request path.
#[derive(Debug, Clone)]
pub struct HtaccessConfig {
    pub directory_index: Vec<String>,
    pub show_indexes: bool,
    pub auth_required: bool,
    pub auth_name: Option<String>,
    pub auth_user_file: Option<String>,
    pub error_documents: HashMap<u16, String>,
    pub add_types: Vec<(String, String)>, // (extension, mime_type)
    pub add_default_charset: Option<String>,
    pub redirects: Vec<RedirectRule>,
    pub rewrite_engine: bool,
    pub rewrite_rules: Vec<RewriteRule>,
    pub header_rules: Vec<crate::header_directive::HeaderRule>,
}

#[derive(Debug, Clone)]
pub struct RedirectRule {
    pub status: u16,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone)]
pub struct RewriteCond {
    pub test_string: String,
    pub condition: String,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RewriteRule {
    pub pattern: String,
    pub substitution: String,
    pub flags: Vec<String>,
    pub conds: Vec<RewriteCond>,
}

impl Default for HtaccessConfig {
    fn default() -> Self {
        Self {
            directory_index: vec!["index.html".to_string(), "index.htm".to_string()],
            show_indexes: true,
            auth_required: false,
            auth_name: None,
            auth_user_file: None,
            error_documents: HashMap::new(),
            add_types: Vec::new(),
            add_default_charset: None,
            redirects: Vec::new(),
            rewrite_engine: false,
            rewrite_rules: Vec::new(),
            header_rules: Vec::new(),
        }
    }
}

/// Walk from `root` down to `dir`, merging all .htaccess files.
/// Deeper files take precedence over shallower ones.
pub fn parse_htaccess_for_path(root: &Path, dir: &Path) -> Result<HtaccessConfig, String> {
    // Collect the chain of directories from root down to dir
    let mut dirs: Vec<&Path> = Vec::new();
    let mut current = dir;
    loop {
        dirs.push(current);
        if current == root {
            break;
        }
        match current.parent() {
            Some(p) if p.starts_with(root) || p == root => current = p,
            _ => break,
        }
    }
    dirs.reverse(); // root first, deepest last

    let mut cfg = HtaccessConfig::default();
    for d in dirs {
        let htaccess_path = d.join(".htaccess");
        if htaccess_path.exists() {
            let content = std::fs::read_to_string(&htaccess_path)
                .map_err(|e| format!("Failed to read {:?}: {}", htaccess_path, e))?;
            let joined = join_continuations(&content);
            apply_htaccess(&mut cfg, &joined, &htaccess_path.display().to_string());
        }
    }
    Ok(cfg)
}

/// Join `\`-terminated continuation lines before tokenization.
///
/// Apache's `.htaccess` permits a line ending in `\` to be joined
/// with the next line. The trailing backslash is stripped and the
/// two lines concatenated with a single space.
///
/// Comment lines (those whose first non-whitespace character is
/// `#`) never continue — Apache strips the trailing backslash but
/// the comment still terminates at the newline.
fn join_continuations(content: &str) -> String {
    let mut out = String::new();
    let mut pending: Option<String> = None;
    for line in content.lines() {
        let is_comment = line.trim_start().starts_with('#');
        if let Some(prev) = pending.take() {
            // Combine `prev` (already stripped of trailing `\`)
            // with the current line. The current line's leading
            // whitespace is removed so continuation indentation
            // doesn't bleed into the joined value.
            let combined = format!("{} {}", prev, line.trim_start());
            if let Some(more) = check_continuation(&combined, false) {
                pending = Some(more);
            } else {
                out.push_str(&combined);
                out.push('\n');
            }
        } else if let Some(stripped) = check_continuation(line, is_comment) {
            pending = Some(stripped);
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if let Some(prev) = pending {
        // Trailing unterminated continuation: emit as-is.
        out.push_str(&prev);
        out.push('\n');
    }
    out
}

fn check_continuation(line: &str, is_comment: bool) -> Option<String> {
    if is_comment {
        return None;
    }
    let trimmed = line.trim_end();
    trimmed.strip_suffix('\\').map(|without_bs| without_bs.trim_end().to_string())
}

fn apply_htaccess(cfg: &mut HtaccessConfig, content: &str, file_path: &str) {
    let mut pending_conds: Vec<RewriteCond> = Vec::new();

    for (line_no, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let tokens: Vec<&str> = line.splitn(10, char::is_whitespace)
            .filter(|s| !s.is_empty())
            .collect();

        if tokens.is_empty() {
            continue;
        }

        match tokens[0].to_ascii_lowercase().as_str() {
            "directoryindex" => {
                if tokens.len() > 1 {
                    cfg.directory_index = tokens[1..].iter().map(|s| s.to_string()).collect();
                }
            }
            "options" => {
                for opt in &tokens[1..] {
                    match opt.to_ascii_lowercase().as_str() {
                        "+indexes" | "indexes" => cfg.show_indexes = true,
                        "-indexes" => cfg.show_indexes = false,
                        _ => {}
                    }
                }
            }
            "authtype" => {
                // Only Basic is supported; presence means auth is configured
            }
            "authname" => {
                if tokens.len() > 1 {
                    let name = tokens[1..].join(" ").trim_matches('"').to_string();
                    cfg.auth_name = Some(name);
                }
            }
            "authuserfile" => {
                if tokens.len() > 1 {
                    cfg.auth_user_file = Some(tokens[1].to_string());
                }
            }
            "require" => {
                if tokens.get(1).map(|s| s.to_ascii_lowercase()) == Some("valid-user".to_string()) {
                    cfg.auth_required = true;
                }
            }
            "errordocument" => {
                if tokens.len() >= 3 {
                    if let Ok(code) = tokens[1].parse::<u16>() {
                        cfg.error_documents.insert(code, tokens[2].to_string());
                    } else {
                        log::warn!("[.htaccess] {}:{}: invalid error code '{}'", file_path, line_no + 1, tokens[1]);
                    }
                }
            }
            "addtype" => {
                if tokens.len() >= 3 {
                    let mime = tokens[1].to_string();
                    // extensions may have leading dot: .foo → foo
                    for ext_raw in &tokens[2..] {
                        let ext = ext_raw.trim_start_matches('.').to_ascii_lowercase();
                        cfg.add_types.push((ext, mime.clone()));
                    }
                }
            }
            "adddefaultcharset" => {
                if tokens.len() > 1 {
                    cfg.add_default_charset = Some(tokens[1].to_string());
                }
            }
            "redirect" => {
                // Redirect [status] from to
                let (status, from, to) = if tokens.len() >= 4 {
                    let s = match tokens[1].to_ascii_lowercase().as_str() {
                        "permanent" => 301,
                        "temp" | "temporary" => 302,
                        "seeother" => 303,
                        "gone" => 410,
                        s => s.parse::<u16>().unwrap_or(302),
                    };
                    (s, tokens[2].to_string(), tokens[3].to_string())
                } else if tokens.len() == 3 {
                    (302, tokens[1].to_string(), tokens[2].to_string())
                } else {
                    log::warn!("[.htaccess] {}:{}: malformed Redirect", file_path, line_no + 1);
                    continue;
                };
                cfg.redirects.push(RedirectRule { status, from, to });
            }
            "rewriteengine" => {
                cfg.rewrite_engine = tokens.get(1)
                    .map(|s| s.eq_ignore_ascii_case("on"))
                    .unwrap_or(false);
            }
            "rewritecond" => {
                if tokens.len() >= 3 {
                    let flags = parse_flags(tokens.get(3).copied().unwrap_or(""));
                    pending_conds.push(RewriteCond {
                        test_string: tokens[1].to_string(),
                        condition: tokens[2].to_string(),
                        flags,
                    });
                }
            }
            "rewriterule" => {
                if tokens.len() >= 3 {
                    let flags = parse_flags(tokens.get(3).copied().unwrap_or(""));
                    cfg.rewrite_rules.push(RewriteRule {
                        pattern: tokens[1].to_string(),
                        substitution: tokens[2].to_string(),
                        flags,
                        conds: std::mem::take(&mut pending_conds),
                    });
                }
            }
            "header" => {
                // Skip the directive keyword itself; tokens[0] is "header" (any case).
                // Re-slice from the original line so quoted values survive.
                let after = line
                    .trim_start()
                    .get(tokens[0].len()..)
                    .unwrap_or("")
                    .trim_start();
                let loc = format!("{}:{}", file_path, line_no + 1);
                match crate::header_directive::parse_header_line(after, &loc) {
                    Ok(rule) => cfg.header_rules.push(rule),
                    Err(reason) => log::warn!(
                        "[.htaccess] {}: malformed Header directive: {}",
                        loc,
                        reason
                    ),
                }
            }
            _ => {
                log::warn!("[.htaccess] {}:{}: unknown directive '{}', skipping", file_path, line_no + 1, tokens[0]);
            }
        }
    }
}

fn parse_flags(raw: &str) -> Vec<String> {
    // raw looks like "[L,NC,R=301]" or ""
    let trimmed = raw.trim_matches(|c| c == '[' || c == ']');
    trimmed.split(',')
        .map(|f| f.trim().to_ascii_uppercase())
        .filter(|f| !f.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_htaccess(dir: &std::path::Path, content: &str) {
        fs::write(dir.join(".htaccess"), content).unwrap();
    }

    #[test]
    fn test_directory_index_parsed() {
        let tmp = TempDir::new().unwrap();
        write_htaccess(tmp.path(), "DirectoryIndex index.php index.html\n");
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert_eq!(cfg.directory_index, vec!["index.php", "index.html"]);
    }

    #[test]
    fn test_options_no_indexes() {
        let tmp = TempDir::new().unwrap();
        write_htaccess(tmp.path(), "Options -Indexes\n");
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert!(!cfg.show_indexes);
    }

    #[test]
    fn test_options_plus_indexes() {
        let tmp = TempDir::new().unwrap();
        write_htaccess(tmp.path(), "Options +Indexes\n");
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert!(cfg.show_indexes);
    }

    #[test]
    fn test_error_document_parsed() {
        let tmp = TempDir::new().unwrap();
        write_htaccess(tmp.path(), "ErrorDocument 404 /404.html\n");
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert_eq!(cfg.error_documents.get(&404), Some(&"/404.html".to_string()));
    }

    #[test]
    fn test_addtype_parsed() {
        let tmp = TempDir::new().unwrap();
        write_htaccess(tmp.path(), "AddType application/x-foo .foo\n");
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert!(cfg.add_types.iter().any(|(e, m)| e == "foo" && m == "application/x-foo"));
    }

    #[test]
    fn test_auth_directives_parsed() {
        let tmp = TempDir::new().unwrap();
        write_htaccess(
            tmp.path(),
            "AuthType Basic\nAuthName \"Restricted\"\nAuthUserFile /etc/passwd\nRequire valid-user\n",
        );
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert!(cfg.auth_required);
        assert_eq!(cfg.auth_user_file.as_deref(), Some("/etc/passwd"));
        assert_eq!(cfg.auth_name.as_deref(), Some("Restricted"));
    }

    #[test]
    fn test_deeper_htaccess_overrides_parent() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        write_htaccess(tmp.path(), "DirectoryIndex parent.html\n");
        write_htaccess(&sub, "DirectoryIndex child.html\n");
        let cfg = parse_htaccess_for_path(tmp.path(), &sub).unwrap();
        assert_eq!(cfg.directory_index, vec!["child.html"]);
    }

    #[test]
    fn test_unknown_directive_skipped() {
        let tmp = TempDir::new().unwrap();
        write_htaccess(tmp.path(), "UnknownDirective foo bar\nDirectoryIndex index.html\n");
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert_eq!(cfg.directory_index, vec!["index.html"]);
    }

    #[test]
    fn test_header_directive_dispatched_to_module() {
        let tmp = TempDir::new().unwrap();
        write_htaccess(tmp.path(), "Header set X-Foo bar\n");
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert_eq!(cfg.header_rules.len(), 1);
    }

    use std::sync::{Mutex, OnceLock};

    #[derive(Clone, Debug)]
    #[allow(dead_code)]
    struct LogEntry {
        level: log::Level,
        msg: String,
    }

    static LOG_CAPTURE: OnceLock<Mutex<Vec<LogEntry>>> = OnceLock::new();
    static LOG_INIT: std::sync::Once = std::sync::Once::new();
    static LOG_TEST_MUTEX: Mutex<()> = Mutex::new(());

    struct CaptureLogger;

    impl log::Log for CaptureLogger {
        fn enabled(&self, _m: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            if let Some(buf) = LOG_CAPTURE.get() {
                if let Ok(mut v) = buf.lock() {
                    v.push(LogEntry {
                        level: record.level(),
                        msg: record.args().to_string(),
                    });
                }
            }
        }
        fn flush(&self) {}
    }

    static CAPTURE_LOGGER: CaptureLogger = CaptureLogger;

    fn install_log_capture() {
        LOG_INIT.call_once(|| {
            let _ = LOG_CAPTURE.set(Mutex::new(Vec::new()));
            let _ = log::set_logger(&CAPTURE_LOGGER);
            log::set_max_level(log::LevelFilter::Debug);
        });
    }

    fn clear_log() {
        install_log_capture();
        if let Some(buf) = LOG_CAPTURE.get() {
            if let Ok(mut v) = buf.lock() {
                v.clear();
            }
        }
    }

    fn captured() -> Vec<LogEntry> {
        LOG_CAPTURE
            .get()
            .and_then(|m| m.lock().ok().map(|v| v.clone()))
            .unwrap_or_default()
    }

    /// Run `f` with the log-capture buffer cleared, holding the
    /// capture mutex so concurrent capture-using tests don't race.
    fn with_log_capture<F: FnOnce()>(f: F) {
        install_log_capture();
        let _guard = LOG_TEST_MUTEX.lock().unwrap();
        clear_log();
        f();
    }

    #[allow(dead_code)]
    fn has_log_at_level(level: log::Level, substring: &str) -> bool {
        captured()
            .iter()
            .any(|e| e.level == level && e.msg.contains(substring))
    }

    #[allow(dead_code)]
    fn count_at_level(level: log::Level) -> usize {
        captured().iter().filter(|e| e.level == level).count()
    }

    #[test]
    fn line_continuation_joins_simple_case() {
        // AddCharset uses backslash continuation in real-world htaccess
        // files. We don't (yet) implement AddCharset, but the parser
        // must not emit bogus `unknown directive` warns for the
        // continuation lines.
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            write_htaccess(
                tmp.path(),
                "AddCharset utf-8 .atom \\\n                 .css\n",
            );
            let _ = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
            // The bogus `.css` token must NOT appear in any log message.
            for e in captured() {
                assert!(
                    !e.msg.contains(".css"),
                    "continuation should have joined; got bogus token in log: {}",
                    e.msg
                );
            }
        });
    }

    #[test]
    fn line_continuation_does_not_join_comments() {
        // Comment lines ending in `\` must NOT continue onto the next
        // line. Apache's parser strips the trailing backslash but the
        // comment still terminates.
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            write_htaccess(
                tmp.path(),
                "# trailing backslash on comment \\\nDirectoryIndex foo.html\n",
            );
            let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
            assert_eq!(cfg.directory_index, vec!["foo.html"]);
        });
    }
}
