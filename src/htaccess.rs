use std::collections::HashMap;
use std::path::Path;
use regex::Regex;

/// Merged configuration from all applicable .htaccess files for a request path.
#[derive(Debug, Clone)]
pub struct HtaccessConfig {
    pub directory_index: Vec<String>,
    pub show_indexes: bool,
    pub auth_required: bool,
    pub auth_name: Option<String>,
    pub auth_user_file: Option<String>,
    pub error_documents: HashMap<u16, String>,
    pub add_types: Vec<AddTypeEntry>,
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
    /// File-pattern scope from enclosing `<FilesMatch>` / `<Files>`
    /// containers. Empty = unscoped.
    pub file_scope: Vec<Regex>,
}

#[derive(Debug, Clone)]
pub struct AddTypeEntry {
    pub file_scope: Vec<Regex>,
    pub ext: String,
    pub mime: String,
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

enum ContainerKind {
    IfModule,
    FilesMatch,
    Files,
    Directory,
    If,
}

fn container_open_kind(tok: &str) -> Option<ContainerKind> {
    let lower = tok.to_ascii_lowercase();
    // Open tokens look like `<IfModule` (no closing `>` because the
    // module name is the second token). For the no-arg form
    // `<IfModule>` we still match — `starts_with` handles both.
    //
    // Order matters: `<ifmodule` is checked BEFORE `<if` so plain
    // `<If` doesn't shadow `<IfModule`.
    if lower.starts_with("<ifmodule") {
        Some(ContainerKind::IfModule)
    } else if lower.starts_with("<filesmatch") {
        Some(ContainerKind::FilesMatch)
    } else if lower.starts_with("<files") {
        Some(ContainerKind::Files)
    } else if lower.starts_with("<directory") {
        Some(ContainerKind::Directory)
    } else if lower.starts_with("<if") {
        Some(ContainerKind::If)
    } else {
        None
    }
}

fn is_container_close(tok: &str) -> bool {
    matches!(
        tok.to_ascii_lowercase().as_str(),
        "</ifmodule>" | "</filesmatch>" | "</files>" | "</directory>" | "</if>"
    )
}

/// Standard Apache directives that webrunner doesn't implement but
/// commonly appear in real-world `.htaccess` files. Demoting these
/// from `warn` to `debug` keeps the default log clean; users can
/// pass `--log-level debug` to see what was skipped.
fn is_known_unsupported(tok: &str) -> bool {
    matches!(
        tok.to_ascii_lowercase().as_str(),
        "directoryslash"
            | "php_flag"
            | "php_value"
            | "fileetag"
            | "redirectmatch"
            | "addencoding"
            | "addcharset"
            | "addoutputfilterbytype"
            | "setenv"
            | "setenvif"
            | "setenvifnocase"
            | "requestheader"
            | "expiresactive"
            | "expiresdefault"
            | "expiresbytype"
    )
}

/// Extract the quoted-or-bare pattern argument from a container open
/// tag like `<FilesMatch "\.php$">` or `<Files *.css>`. Returns the
/// pattern text without the surrounding quotes or trailing `>`.
///
/// `tokens` is the already-split tokens for the line; `tokens[0]` is
/// the open tag itself (`<FilesMatch` or `<Files`).
///
/// Examples:
/// - `<FilesMatch "\.php$">` → `Some("\\.php$")`
/// - `<Files *.css>` → `Some("*.css")`
/// - `<FilesMatch>` (no arg) → `None`
fn extract_container_pattern(tokens: &[&str]) -> Option<String> {
    if tokens.len() < 2 {
        return None;
    }
    let joined = tokens[1..].join(" ");
    let trimmed = joined.trim_end_matches('>').trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed);
    if unquoted.is_empty() {
        None
    } else {
        Some(unquoted.to_string())
    }
}

fn apply_htaccess(cfg: &mut HtaccessConfig, content: &str, file_path: &str) {
    let mut pending_conds: Vec<RewriteCond> = Vec::new();
    let mut scope_stack: Vec<Regex> = Vec::new();

    for (line_no, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Use `split_whitespace` so runs of whitespace collapse into
        // a single separator. The earlier `splitn(10, char::is_whitespace)`
        // form split on each individual whitespace char, which broke
        // tokens like `AddType ... <many spaces> ... json` — the
        // 10-split cap was exhausted inside the whitespace run, leaving
        // a junk token with leading whitespace.
        let tokens: Vec<&str> = line.split_whitespace().collect();

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
                    for ext_raw in &tokens[2..] {
                        // Strip trailing semicolons (some real-world
                        // htaccess files mistakenly add them); then
                        // strip a leading dot.
                        let ext = ext_raw
                            .trim_end_matches(';')
                            .trim_start_matches('.')
                            .to_ascii_lowercase();
                        if ext.is_empty() {
                            continue;
                        }
                        if ext_raw.ends_with(';') {
                            log::debug!(
                                "[.htaccess] {}:{}: stripped trailing ';' from AddType extension '{}'",
                                file_path,
                                line_no + 1,
                                ext_raw,
                            );
                        }
                        cfg.add_types.push(AddTypeEntry {
                            file_scope: scope_stack.clone(),
                            ext,
                            mime: mime.clone(),
                        });
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
                        file_scope: scope_stack.clone(),
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
                    Ok(mut rule) => {
                        rule.file_scope = scope_stack.clone();
                        cfg.header_rules.push(rule);
                    }
                    Err(reason) => log::warn!(
                        "[.htaccess] {}: malformed Header directive: {}",
                        loc,
                        reason
                    ),
                }
            }
            t if container_open_kind(t).is_some() => {
                // Safe: just matched.
                let kind = container_open_kind(t).unwrap();
                match kind {
                    ContainerKind::IfModule => {
                        // Silent passthrough; webrunner natively implements
                        // the module semantics.
                    }
                    ContainerKind::FilesMatch => {
                        let raw_pat = extract_container_pattern(&tokens);
                        match raw_pat.as_deref().map(Regex::new) {
                            Some(Ok(re)) => scope_stack.push(re),
                            Some(Err(e)) => log::warn!(
                                "[.htaccess] {}:{}: malformed FilesMatch pattern '{}': {}",
                                file_path,
                                line_no + 1,
                                raw_pat.as_deref().unwrap_or(""),
                                e
                            ),
                            None => log::warn!(
                                "[.htaccess] {}:{}: <FilesMatch> missing pattern",
                                file_path,
                                line_no + 1
                            ),
                        }
                    }
                    ContainerKind::Files => {
                        let raw_pat = extract_container_pattern(&tokens);
                        match raw_pat.as_deref() {
                            Some(pat) => {
                                let re_str = glob_to_regex(pat);
                                match Regex::new(&re_str) {
                                    Ok(re) => scope_stack.push(re),
                                    Err(e) => log::warn!(
                                        "[.htaccess] {}:{}: malformed Files pattern '{}': {}",
                                        file_path,
                                        line_no + 1,
                                        pat,
                                        e
                                    ),
                                }
                            }
                            None => log::warn!(
                                "[.htaccess] {}:{}: <Files> missing pattern",
                                file_path,
                                line_no + 1
                            ),
                        }
                    }
                    ContainerKind::Directory | ContainerKind::If => {
                        // Recognized but not scoping (Directory is invalid
                        // in .htaccess per Apache; If needs an expression
                        // evaluator). Contained rules apply globally.
                        log::debug!(
                            "[.htaccess] {}:{}: container '{}' is recognized but does not yet scope contained rules",
                            file_path,
                            line_no + 1,
                            tokens[0]
                        );
                    }
                }
            }
            t if is_container_close(t) => {
                let lower = t.to_ascii_lowercase();
                match lower.as_str() {
                    "</filesmatch>" | "</files>" => match scope_stack.pop() {
                        Some(_) => {}
                        None => log::warn!(
                            "[.htaccess] {}:{}: unmatched '{}'",
                            file_path,
                            line_no + 1,
                            tokens[0]
                        ),
                    },
                    _ => {
                        // </IfModule>, </Directory>, </If> are no-ops.
                    }
                }
            }
            t if is_known_unsupported(t) => {
                log::debug!(
                    "[.htaccess] {}:{}: directive '{}' is recognized but not yet implemented in webrunner",
                    file_path,
                    line_no + 1,
                    tokens[0]
                );
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

/// Convert an Apache-style glob pattern to a regex string.
///
/// - `*` → `.*`
/// - `?` → `.`
/// - All regex metacharacters (`.`, `+`, `(`, `)`, `^`, `$`, `|`,
///   `{`, `}`, `\`) are escaped.
/// - Result is anchored with `^(?:...)$` so the pattern matches
///   the whole basename, not a substring.
/// - Brace expansion (`{a,b}`) is NOT supported — those characters
///   are escaped to literals.
fn glob_to_regex(pat: &str) -> String {
    let mut out = String::from("^(?:");
    for c in pat.chars() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '.' | '+' | '(' | ')' | '^' | '$' | '|' | '{' | '}' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            other => out.push(other),
        }
    }
    out.push_str(")$");
    out
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
        assert!(cfg.add_types.iter().any(|entry| entry.ext == "foo" && entry.mime == "application/x-foo"));
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

    use std::cell::RefCell;
    use std::sync::Once;

    #[derive(Clone, Debug)]
    struct LogEntry {
        level: log::Level,
        msg: String,
    }

    thread_local! {
        /// Per-thread log capture buffer. `Some(Vec)` when a capture-
        /// using test is active on this thread; `None` otherwise.
        /// Messages from threads with no active capture are dropped
        /// by the logger, preventing cross-test bleed-through.
        static CAPTURE: RefCell<Option<Vec<LogEntry>>> = const { RefCell::new(None) };
    }

    static LOG_INIT: Once = Once::new();

    struct CaptureLogger;

    impl log::Log for CaptureLogger {
        fn enabled(&self, _m: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            CAPTURE.with(|cell| {
                if let Some(buf) = cell.borrow_mut().as_mut() {
                    buf.push(LogEntry {
                        level: record.level(),
                        msg: record.args().to_string(),
                    });
                }
            });
        }
        fn flush(&self) {}
    }

    static CAPTURE_LOGGER: CaptureLogger = CaptureLogger;

    fn install_log_capture() {
        LOG_INIT.call_once(|| {
            let _ = log::set_logger(&CAPTURE_LOGGER);
            log::set_max_level(log::LevelFilter::Debug);
        });
    }

    /// Run `f` with a fresh per-thread log capture. The capture is
    /// thread-local, so multiple capture-using tests run in parallel
    /// without serialization and without cross-thread bleed-through.
    /// On panic in `f`, the unwind propagates after the capture is
    /// reset on the next call (no poisoning is possible because no
    /// mutex is held).
    fn with_log_capture<F: FnOnce()>(f: F) {
        install_log_capture();
        CAPTURE.with(|cell| cell.replace(Some(Vec::new())));
        f();
    }

    fn captured() -> Vec<LogEntry> {
        CAPTURE.with(|cell| cell.borrow().clone().unwrap_or_default())
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

    #[test]
    fn ifmodule_is_passthrough() {
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            write_htaccess(
                tmp.path(),
                "<IfModule mod_headers.c>\nHeader set X-Foo bar\n</IfModule>\n",
            );
            let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
            // Contained Header rule was captured.
            assert_eq!(cfg.header_rules.len(), 1);
            // No warn-level logs emitted.
            assert_eq!(
                count_at_level(log::Level::Warn),
                0,
                "<IfModule> wrap should produce no warn-level logs; got: {:?}",
                captured()
            );
        });
    }

    #[test]
    fn filesmatch_contained_directive_currently_applies_globally() {
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            write_htaccess(
                tmp.path(),
                "<FilesMatch \"\\.php$\">\nHeader set X-Foo bar\n</FilesMatch>\n",
            );
            let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
            assert_eq!(cfg.header_rules.len(), 1);
            // Now that scope_stack is wired, a balanced <FilesMatch> produces
            // no log output at any level (scope push/pop is silent).
            assert_eq!(
                count_at_level(log::Level::Warn),
                0,
                "<FilesMatch> wrap should produce no warn-level logs; got: {:?}",
                captured()
            );
        });
    }

    #[test]
    fn recognized_unsupported_directive_no_warn() {
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            write_htaccess(tmp.path(), "php_flag display_errors off\n");
            let _ = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
            assert_eq!(
                count_at_level(log::Level::Warn),
                0,
                "php_flag should produce no warn-level log; got: {:?}",
                captured()
            );
            assert!(
                has_log_at_level(log::Level::Debug, "php_flag"),
                "expected debug log for php_flag; got: {:?}",
                captured()
            );
        });
    }

    #[test]
    fn unknown_directive_still_warns() {
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            write_htaccess(tmp.path(), "TotallyMadeUpDirective foo\n");
            let _ = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
            assert!(
                has_log_at_level(log::Level::Warn, "TotallyMadeUpDirective"),
                "genuinely unknown directive should still warn; got: {:?}",
                captured()
            );
        });
    }

    #[test]
    fn addtype_strips_trailing_semicolons() {
        // Real-world htaccess files sometimes use semicolons after
        // extensions (`AddType ... xls;`). Apache itself stores them
        // literally and the extension never matches — same as the
        // pre-fix behaviour here. We strip them so the mapping works.
        let tmp = TempDir::new().unwrap();
        write_htaccess(
            tmp.path(),
            "AddType application/vnd.ms-excel xls;\n",
        );
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert!(
            cfg.add_types
                .iter()
                .any(|entry| entry.ext == "xls" && entry.mime == "application/vnd.ms-excel"),
            "expected ('xls', 'application/vnd.ms-excel') in add_types; got {:?}",
            cfg.add_types,
        );
        assert!(
            !cfg.add_types.iter().any(|entry| entry.ext == "xls;"),
            "extension should be stripped of trailing semicolon"
        );
    }

    #[test]
    fn parser_robustness_real_world_fixture() {
        let fixture = r#"
# General Apache settings
Options SymLinksIfOwnerMatch
DirectorySlash Off
RewriteEngine On

<FilesMatch "\.php$">
    <IfModule mod_headers.c>
        Header set Cache-Control "no-cache"
    </IfModule>
</FilesMatch>

RewriteCond %{REQUEST_FILENAME} !-f
RewriteCond %{REQUEST_FILENAME} !-d
RewriteRule ^(.+)\.(\d+)\.(js|css)$ $1.$3 [L]

RewriteCond %{REQUEST_FILENAME} !-f
RewriteRule . index.php [QSA,L]

php_flag    display_errors          off
php_value   max_input_vars          16384

<IfModule mod_headers.c>
    Header set X-UA-Compatible "IE=edge"
    Header set X-Content-Type-Options "nosniff"
    Header setifempty X-FRAME-OPTIONS "deny"
</IfModule>

<IfModule mod_mime.c>
    AddType application/json                            json map topojson
    AddType application/vnd.ms-excel                    xls;
    AddType application/vnd.oasis.opendocument.spreadsheet  ods;
    AddCharset utf-8 .atom \
                     .css \
                     .js
</IfModule>

AddDefaultCharset utf-8

<IfModule mod_alias.c>
    RedirectMatch 204 /favicon.ico$
</IfModule>

<IfModule mod_expires.c>
    ExpiresActive on
    ExpiresDefault "access plus 1 month"
    ExpiresByType text/css "access plus 1 year"
</IfModule>

FileETag None
"#;
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            std::fs::write(tmp.path().join(".htaccess"), fixture).unwrap();
            let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();

            // Supported directives all parsed.
            assert!(cfg.rewrite_engine, "RewriteEngine On");
            assert_eq!(
                cfg.rewrite_rules.len(),
                2,
                "expected 2 RewriteRules; got {:?}",
                cfg.rewrite_rules
            );
            // 4 Header rules: Cache-Control (inside FilesMatch — applies globally for now),
            // X-UA-Compatible, X-Content-Type-Options, X-FRAME-OPTIONS.
            assert_eq!(
                cfg.header_rules.len(),
                4,
                "expected 4 Header rules; got {}",
                cfg.header_rules.len()
            );
            // AddType including the `xls;` typo case.
            assert!(
                cfg.add_types.iter().any(|entry| entry.ext == "xls"),
                "AddType xls; should strip to 'xls'"
            );
            assert!(
                cfg.add_types.iter().any(|entry| entry.ext == "json"),
                "AddType json present"
            );
            assert_eq!(cfg.add_default_charset.as_deref(), Some("utf-8"));

            // The critical assertion: ZERO warn-level entries.
            let warns: Vec<_> = captured()
                .into_iter()
                .filter(|e| e.level == log::Level::Warn)
                .collect();
            assert!(
                warns.is_empty(),
                "expected zero warn-level logs; got {} warns: {:?}",
                warns.len(),
                warns,
            );
        });
    }

    #[test]
    fn glob_to_regex_simple_star() {
        let re_str = glob_to_regex("*.php");
        let re = regex::Regex::new(&re_str).unwrap();
        assert!(re.is_match("index.php"));
        assert!(re.is_match("foo.php"));
        assert!(!re.is_match("index.html"));
        assert!(!re.is_match("foo.php.bak"));
    }

    #[test]
    fn glob_to_regex_question_mark() {
        let re_str = glob_to_regex("foo?.php");
        let re = regex::Regex::new(&re_str).unwrap();
        assert!(re.is_match("foo1.php"));
        assert!(re.is_match("fooa.php"));
        assert!(!re.is_match("foo.php"));
        assert!(!re.is_match("foo12.php"));
    }

    #[test]
    fn glob_to_regex_escapes_regex_metachars() {
        // The dot in a glob is a LITERAL dot, not a regex metachar.
        let re_str = glob_to_regex("a.b");
        let re = regex::Regex::new(&re_str).unwrap();
        assert!(re.is_match("a.b"));
        assert!(!re.is_match("axb"));
        assert!(!re.is_match("aXb"));
    }

    #[test]
    fn scope_stack_filesmatch_pushes_and_pops() {
        // Without scoping support in T3, a Header rule inside
        // <FilesMatch> has no file_scope (T3 wires that). This test
        // asserts the parser correctly opens and closes scope on a
        // balanced pair without errors.
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            write_htaccess(
                tmp.path(),
                "<FilesMatch \"\\.php$\">\nHeader set X-Foo bar\n</FilesMatch>\n",
            );
            let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
            assert_eq!(cfg.header_rules.len(), 1);
            // No warn-level logs: balanced FilesMatch is clean.
            assert_eq!(count_at_level(log::Level::Warn), 0);
        });
    }

    #[test]
    fn unmatched_filesmatch_close_warns() {
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            write_htaccess(tmp.path(), "</FilesMatch>\n");
            let _ = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
            assert!(
                has_log_at_level(log::Level::Warn, "unmatched"),
                "expected unmatched-close warn; got: {:?}",
                captured()
            );
        });
    }

    #[test]
    fn addtype_scoped_under_filesmatch() {
        let tmp = TempDir::new().unwrap();
        write_htaccess(
            tmp.path(),
            "<FilesMatch \"\\.xls$\">\nAddType application/vnd.ms-excel xls\n</FilesMatch>\n",
        );
        let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
        assert_eq!(cfg.add_types.len(), 1);
        let entry = &cfg.add_types[0];
        assert_eq!(entry.ext, "xls");
        assert_eq!(entry.mime, "application/vnd.ms-excel");
        assert_eq!(entry.file_scope.len(), 1);
    }

    #[test]
    fn malformed_filesmatch_pattern_warns_does_not_crash() {
        with_log_capture(|| {
            let tmp = TempDir::new().unwrap();
            // `[invalid` is an unclosed character class — regex compile fails.
            write_htaccess(
                tmp.path(),
                "<FilesMatch \"[invalid\">\nHeader set X-After bar\n</FilesMatch>\n",
            );
            let cfg = parse_htaccess_for_path(tmp.path(), tmp.path()).unwrap();
            // The Header rule after the malformed FilesMatch still parses.
            assert_eq!(cfg.header_rules.len(), 1);
            // A warn mentions the malformed pattern.
            assert!(
                has_log_at_level(log::Level::Warn, "malformed FilesMatch"),
                "expected malformed FilesMatch warn; got: {:?}",
                captured()
            );
        });
    }
}
