use regex::Regex;
use crate::htaccess::HtaccessConfig;

fn is_absolute_subst(s: &str) -> bool {
    s.starts_with('/')
        || s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("ftp://")
        || s.starts_with("mailto:")
        || s.starts_with("//")
}

#[derive(Debug)]
pub enum RewriteResult {
    /// No rule matched — continue to normal routing
    None,
    /// Internal rewrite — serve this new path instead
    Rewrite(String),
    /// External redirect. `location: None` produces a response
    /// without a `Location` header — used by `RedirectMatch`
    /// status-only forms (204, 410, 4xx).
    Redirect { status: u16, location: Option<String> },
}

/// Apply all Redirect directives and then RewriteRules from the config.
/// `path` is the request path (no query string). `query` is the query string (may be empty).
pub fn apply_rewrites(path: &str, query: &str, cfg: &HtaccessConfig) -> RewriteResult {
    // 1. RedirectMatch directives (regex match, more specific than prefix).
    for rule in &cfg.redirect_matches {
        if let Some(caps) = rule.pattern.captures(path) {
            let location = rule.to.as_ref().map(|template| {
                let mut subst = template.clone();
                for i in 1..caps.len() {
                    if let Some(m) = caps.get(i) {
                        subst = subst.replace(&format!("${}", i), m.as_str());
                    }
                }
                subst
            });
            return RewriteResult::Redirect {
                status: rule.status,
                location,
            };
        }
    }

    // 2. Redirect directives (prefix match)
    for rule in &cfg.redirects {
        if path.starts_with(&rule.from) {
            let location = if path.len() > rule.from.len() {
                format!("{}{}", rule.to, &path[rule.from.len()..])
            } else {
                rule.to.clone()
            };
            return RewriteResult::Redirect { status: rule.status, location: Some(location) };
        }
    }

    // 2. RewriteRules (only if engine is on)
    if !cfg.rewrite_engine {
        return RewriteResult::None;
    }

    let basename = path.rsplit('/').next().unwrap_or("");

    for rule in &cfg.rewrite_rules {
        // Skip rules scoped to filenames that don't match this request.
        if !rule.file_scope.iter().all(|re| re.is_match(basename)) {
            continue;
        }
        let nc = rule.flags.iter().any(|f| f == "NC");
        let re = if nc {
            Regex::new(&format!("(?i){}", rule.pattern))
        } else {
            Regex::new(&rule.pattern)
        };
        let re = match re {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[rewrite] invalid pattern '{}': {}", rule.pattern, e);
                continue;
            }
        };

        let match_path = path.trim_start_matches('/');
        if let Some(caps) = re.captures(match_path) {
            // Check RewriteConds (all must match)
            if !eval_conds(&rule.conds, path) {
                continue;
            }

            // Apply substitution — replace capture groups $1, $2, ...
            let mut subst = rule.substitution.clone();
            for i in 1..caps.len() {
                if let Some(m) = caps.get(i) {
                    subst = subst.replace(&format!("${}", i), m.as_str());
                }
            }

            // QSA flag: append original query string
            let qsa = rule.flags.iter().any(|f| f == "QSA");
            if qsa && !query.is_empty() {
                if subst.contains('?') {
                    subst.push('&');
                } else {
                    subst.push('?');
                }
                subst.push_str(query);
            }

            // [F] flag — return 403 with no Location.
            if rule.flags.iter().any(|f| f == "F") {
                return RewriteResult::Redirect { status: 403, location: None };
            }
            // [G] flag — return 410 with no Location.
            if rule.flags.iter().any(|f| f == "G") {
                return RewriteResult::Redirect { status: 410, location: None };
            }

            // Apply RewriteBase to relative substitutions.
            if let Some(base) = &cfg.rewrite_base {
                if !is_absolute_subst(&subst) {
                    let sep = if base.ends_with('/') || subst.starts_with('/') {
                        ""
                    } else {
                        "/"
                    };
                    subst = format!("{}{}{}", base, sep, subst);
                }
            }

            // R flag: treat as redirect
            let r_flag = rule.flags.iter().find(|f| f.starts_with("R"));
            if let Some(r) = r_flag {
                let status = r.trim_start_matches("R=").parse::<u16>().unwrap_or(302);
                return RewriteResult::Redirect { status, location: Some(subst) };
            }

            // [L] flag: stop after this rule (always stop for now since we return)
            return RewriteResult::Rewrite(subst);
        }
    }

    RewriteResult::None
}

fn eval_conds(conds: &[crate::htaccess::RewriteCond], path: &str) -> bool {
    for cond in conds {
        let nc = cond.flags.iter().any(|f| f == "NC");
        let test_val = expand_cond_var(&cond.test_string, path);
        let pattern = if nc {
            format!("(?i){}", cond.condition)
        } else {
            cond.condition.clone()
        };
        let matched = if let Ok(re) = Regex::new(&pattern) {
            re.is_match(&test_val)
        } else {
            false
        };
        if !matched {
            return false;
        }
    }
    true
}

fn expand_cond_var(var: &str, path: &str) -> String {
    match var {
        "%{REQUEST_URI}" => path.to_string(),
        "%{REQUEST_FILENAME}" => path.to_string(),
        _ => var.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::htaccess::{HtaccessConfig, RedirectRule, RewriteRule};

    fn cfg_with_redirect(status: u16, from: &str, to: &str) -> HtaccessConfig {
        let mut cfg = HtaccessConfig::default();
        cfg.redirects.push(RedirectRule {
            status,
            from: from.to_string(),
            to: to.to_string(),
        });
        cfg
    }

    #[test]
    fn test_redirect_prefix_match() {
        let cfg = cfg_with_redirect(301, "/old", "https://example.com/new");
        let result = apply_rewrites("/old/page", "", &cfg);
        assert!(matches!(result, RewriteResult::Redirect { status: 301, .. }));
    }

    #[test]
    fn test_redirect_no_match() {
        let cfg = cfg_with_redirect(301, "/old", "https://example.com/new");
        let result = apply_rewrites("/other", "", &cfg);
        assert!(matches!(result, RewriteResult::None));
    }

    #[test]
    fn test_rewrite_rule_basic() {
        let mut cfg = HtaccessConfig::default();
        cfg.rewrite_engine = true;
        cfg.rewrite_rules.push(RewriteRule {
            pattern: r"^foo/(.+)$".to_string(),
            substitution: "/bar/$1".to_string(),
            flags: vec![],
            conds: vec![],
            file_scope: Vec::new(),
        });
        let result = apply_rewrites("/foo/baz", "", &cfg);
        assert!(matches!(result, RewriteResult::Rewrite(ref p) if p == "/bar/baz"));
    }

    #[test]
    fn test_rewrite_rule_r_flag_makes_redirect() {
        let mut cfg = HtaccessConfig::default();
        cfg.rewrite_engine = true;
        cfg.rewrite_rules.push(RewriteRule {
            pattern: r"^go$".to_string(),
            substitution: "https://example.com".to_string(),
            flags: vec!["R=302".to_string()],
            conds: vec![],
            file_scope: Vec::new(),
        });
        let result = apply_rewrites("/go", "", &cfg);
        assert!(matches!(result, RewriteResult::Redirect { status: 302, .. }));
    }

    #[test]
    fn test_rewrite_engine_off_skips_rules() {
        let mut cfg = HtaccessConfig::default();
        cfg.rewrite_engine = false;
        cfg.rewrite_rules.push(RewriteRule {
            pattern: r"^foo$".to_string(),
            substitution: "/bar".to_string(),
            flags: vec![],
            conds: vec![],
            file_scope: Vec::new(),
        });
        let result = apply_rewrites("/foo", "", &cfg);
        assert!(matches!(result, RewriteResult::None));
    }

    #[test]
    fn test_qsa_flag_appends_query() {
        let mut cfg = HtaccessConfig::default();
        cfg.rewrite_engine = true;
        cfg.rewrite_rules.push(RewriteRule {
            pattern: r"^page$".to_string(),
            substitution: "/index.php".to_string(),
            flags: vec!["QSA".to_string()],
            conds: vec![],
            file_scope: Vec::new(),
        });
        let result = apply_rewrites("/page", "lang=en", &cfg);
        assert!(matches!(result, RewriteResult::Rewrite(ref p) if p == "/index.php?lang=en"));
    }

    #[test]
    fn apply_redirectmatch_with_captures() {
        let mut cfg = HtaccessConfig::default();
        cfg.redirect_matches.push(crate::htaccess::RedirectMatchRule {
            status: 301,
            pattern: regex::Regex::new("^/old/(.*)$").unwrap(),
            to: Some("/new/$1".to_string()),
        });
        let result = apply_rewrites("/old/foo/bar", "", &cfg);
        match result {
            RewriteResult::Redirect { status, location } => {
                assert_eq!(status, 301);
                assert_eq!(location.as_deref(), Some("/new/foo/bar"));
            }
            other => panic!("expected Redirect; got {:?}", other),
        }
    }

    #[test]
    fn apply_redirectmatch_status_204_omits_location() {
        let mut cfg = HtaccessConfig::default();
        cfg.redirect_matches.push(crate::htaccess::RedirectMatchRule {
            status: 204,
            pattern: regex::Regex::new("/favicon.ico$").unwrap(),
            to: None,
        });
        let result = apply_rewrites("/favicon.ico", "", &cfg);
        match result {
            RewriteResult::Redirect { status, location } => {
                assert_eq!(status, 204);
                assert!(location.is_none(), "204 should have no Location");
            }
            other => panic!("expected Redirect; got {:?}", other),
        }
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn apply_rewrite_f_flag_returns_403() {
        let mut cfg = HtaccessConfig::default();
        cfg.rewrite_engine = true;
        cfg.rewrite_rules.push(RewriteRule {
            pattern: r"^secret/.*".to_string(),
            substitution: "-".to_string(),
            flags: vec!["F".to_string()],
            conds: vec![],
            file_scope: Vec::new(),
        });
        let result = apply_rewrites("/secret/file.txt", "", &cfg);
        match result {
            RewriteResult::Redirect { status, location } => {
                assert_eq!(status, 403);
                assert!(location.is_none(), "[F] should produce no Location");
            }
            other => panic!("expected Redirect(403); got {:?}", other),
        }
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn apply_rewrite_g_flag_returns_410() {
        let mut cfg = HtaccessConfig::default();
        cfg.rewrite_engine = true;
        cfg.rewrite_rules.push(RewriteRule {
            pattern: r"^gone-page$".to_string(),
            substitution: "-".to_string(),
            flags: vec!["G".to_string()],
            conds: vec![],
            file_scope: Vec::new(),
        });
        let result = apply_rewrites("/gone-page", "", &cfg);
        match result {
            RewriteResult::Redirect { status, location } => {
                assert_eq!(status, 410);
                assert!(location.is_none(), "[G] should produce no Location");
            }
            other => panic!("expected Redirect(410); got {:?}", other),
        }
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn apply_rewritebase_prepends_to_relative_substitution() {
        let mut cfg = HtaccessConfig::default();
        cfg.rewrite_engine = true;
        cfg.rewrite_base = Some("/app".to_string());
        cfg.rewrite_rules.push(RewriteRule {
            pattern: r"^foo$".to_string(),
            substitution: "bar".to_string(),
            flags: vec![],
            conds: vec![],
            file_scope: Vec::new(),
        });
        let result = apply_rewrites("/foo", "", &cfg);
        match result {
            RewriteResult::Rewrite(p) => assert_eq!(p, "/app/bar"),
            other => panic!("expected Rewrite(/app/bar); got {:?}", other),
        }
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn apply_rewritebase_leaves_absolute_substitution_alone() {
        let mut cfg = HtaccessConfig::default();
        cfg.rewrite_engine = true;
        cfg.rewrite_base = Some("/app".to_string());
        cfg.rewrite_rules.push(RewriteRule {
            pattern: r"^foo$".to_string(),
            substitution: "/already-absolute".to_string(),
            flags: vec![],
            conds: vec![],
            file_scope: Vec::new(),
        });
        let result = apply_rewrites("/foo", "", &cfg);
        match result {
            RewriteResult::Rewrite(p) => assert_eq!(p, "/already-absolute"),
            other => panic!("expected Rewrite(/already-absolute); got {:?}", other),
        }
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn apply_rewrites_respects_file_scope() {
        let mut cfg = HtaccessConfig::default();
        cfg.rewrite_engine = true;
        cfg.rewrite_rules.push(RewriteRule {
            pattern: "^.*".to_string(),
            substitution: "/rewritten".to_string(),
            flags: vec!["L".to_string()],
            conds: Vec::new(),
            file_scope: vec![regex::Regex::new("^.*\\.php$").unwrap()],
        });

        // PHP request → rule fires.
        match apply_rewrites("/foo.php", "", &cfg) {
            RewriteResult::Rewrite(p) => assert_eq!(p, "/rewritten"),
            other => panic!("expected Rewrite for /foo.php, got {:?}", other),
        }

        // HTML request → rule does NOT fire (basename doesn't match).
        match apply_rewrites("/foo.html", "", &cfg) {
            RewriteResult::None => {}
            other => panic!("expected None for /foo.html, got {:?}", other),
        }
    }
}
