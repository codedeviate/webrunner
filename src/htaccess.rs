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
            apply_htaccess(&mut cfg, &content, &htaccess_path.display().to_string());
        }
    }
    Ok(cfg)
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
}
