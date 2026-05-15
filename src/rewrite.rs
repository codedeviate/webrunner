use regex::Regex;
use crate::htaccess::HtaccessConfig;

#[derive(Debug)]
pub enum RewriteResult {
    /// No rule matched — continue to normal routing
    None,
    /// Internal rewrite — serve this new path instead
    Rewrite(String),
    /// External redirect — send this status + Location header
    Redirect { status: u16, location: String },
}

/// Apply all Redirect directives and then RewriteRules from the config.
/// `path` is the request path (no query string). `query` is the query string (may be empty).
pub fn apply_rewrites(path: &str, query: &str, cfg: &HtaccessConfig) -> RewriteResult {
    // 1. Redirect directives (prefix match)
    for rule in &cfg.redirects {
        if path.starts_with(&rule.from) {
            let location = if path.len() > rule.from.len() {
                format!("{}{}", rule.to, &path[rule.from.len()..])
            } else {
                rule.to.clone()
            };
            return RewriteResult::Redirect { status: rule.status, location };
        }
    }

    // 2. RewriteRules (only if engine is on)
    if !cfg.rewrite_engine {
        return RewriteResult::None;
    }

    for rule in &cfg.rewrite_rules {
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

        // Strip leading slash for matching (Apache matches against the path without leading slash)
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

            // R flag: treat as redirect
            let r_flag = rule.flags.iter().find(|f| f.starts_with("R"));
            if let Some(r) = r_flag {
                let status = r.trim_start_matches("R=").parse::<u16>().unwrap_or(302);
                return RewriteResult::Redirect { status, location: subst };
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
    use crate::htaccess::{HtaccessConfig, RedirectRule, RewriteRule, RewriteCond};

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
        });
        let result = apply_rewrites("/page", "lang=en", &cfg);
        assert!(matches!(result, RewriteResult::Rewrite(ref p) if p == "/index.php?lang=en"));
    }
}
