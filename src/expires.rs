//! Apache `mod_expires` directive support for `.htaccess`.
//!
//! Implements `ExpiresActive`, `ExpiresDefault`, `ExpiresByType`
//! with the `[access|now] plus N <unit>` time-spec grammar. Emits
//! `Cache-Control: max-age=N` and `Expires: <http-date>` headers
//! based on the response's Content-Type.

#[allow(unused_imports)]
use axum::body::Body;
#[allow(unused_imports)]
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, EXPIRES};
#[allow(unused_imports)]
use axum::http::{HeaderValue, Response};
#[allow(unused_imports)]
use std::time::{Duration, SystemTime};

#[allow(dead_code)] // fields used in T2/T4
#[derive(Debug, Clone, Default)]
pub struct ExpiresConfig {
    pub active: bool,
    pub default: Option<u64>,           // seconds
    pub by_type: Vec<(String, u64)>,    // (mime, seconds)
}

/// Parse `"access plus N <unit>"` (or `"now plus N <unit>"`) into
/// seconds. Apache conventions: month = 30 days, year = 365 days.
///
/// Supported units: `second(s)`, `minute(s)`, `hour(s)`, `day(s)`,
/// `week(s)`, `month(s)`, `year(s)`.
///
/// Returns `Err(reason)` on malformed input (unknown base, missing
/// 'plus', non-numeric duration, unknown unit, or multi-component
/// specs which are deferred).
#[allow(dead_code)] // called from htaccess.rs in T3
pub fn parse_expires_spec(spec: &str) -> Result<u64, String> {
    let mut iter = spec.split_whitespace();
    let base = iter.next().ok_or_else(|| "empty spec".to_string())?;
    if !base.eq_ignore_ascii_case("access") && !base.eq_ignore_ascii_case("now") {
        return Err(format!(
            "unsupported base '{}' (only 'access' / 'now' are implemented)",
            base
        ));
    }
    let plus = iter.next().ok_or_else(|| "missing 'plus' keyword".to_string())?;
    if !plus.eq_ignore_ascii_case("plus") {
        return Err(format!("expected 'plus', got '{}'", plus));
    }
    let n_str = iter.next().ok_or_else(|| "missing duration".to_string())?;
    let n: u64 = n_str
        .parse()
        .map_err(|_| format!("invalid duration '{}'", n_str))?;
    let unit = iter.next().ok_or_else(|| "missing unit".to_string())?;
    if iter.next().is_some() {
        return Err("multi-component specs not supported".to_string());
    }

    let per_unit: u64 = match unit.to_ascii_lowercase().as_str() {
        "second" | "seconds" => 1,
        "minute" | "minutes" => 60,
        "hour" | "hours" => 3_600,
        "day" | "days" => 86_400,
        "week" | "weeks" => 604_800,
        "month" | "months" => 2_592_000,   // 30 days
        "year" | "years" => 31_536_000,    // 365 days
        other => return Err(format!("unknown unit '{}'", other)),
    };

    Ok(n.saturating_mul(per_unit))
}

/// Apply `Cache-Control: max-age=N` and `Expires: <http-date>`
/// headers to the response based on the request's Content-Type
/// and the `ExpiresConfig`.
///
/// Called from `handle_request` BEFORE `header_directive::apply_rules`
/// so user-specified `Header set Cache-Control` overrides our value
/// (Apache semantics).
#[allow(dead_code)] // wired into handle_request in T4
pub fn apply_expires(_response: &mut Response<Body>, _cfg: &ExpiresConfig) {
    // Body filled in by T2.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_expires_spec_access_plus_1_month() {
        assert_eq!(parse_expires_spec("access plus 1 month"), Ok(2_592_000));
    }

    #[test]
    fn parse_expires_spec_now_plus_5_seconds() {
        // `now` is an alias for `access`.
        assert_eq!(parse_expires_spec("now plus 5 seconds"), Ok(5));
    }

    #[test]
    fn parse_expires_spec_year_singular_and_plural() {
        assert_eq!(parse_expires_spec("access plus 1 year"), Ok(31_536_000));
        assert_eq!(parse_expires_spec("access plus 2 years"), Ok(63_072_000));
    }

    #[test]
    fn parse_expires_spec_zero_seconds_immediate_expiry() {
        assert_eq!(parse_expires_spec("access plus 0 seconds"), Ok(0));
    }

    #[test]
    fn parse_expires_spec_modification_base_errs() {
        let err = parse_expires_spec("modification plus 1 day").unwrap_err();
        assert!(err.to_lowercase().contains("unsupported base"), "got: {}", err);
    }

    #[test]
    fn parse_expires_spec_malformed_missing_unit_errs() {
        let err = parse_expires_spec("access plus 5").unwrap_err();
        assert!(err.to_lowercase().contains("missing unit"), "got: {}", err);
    }

    #[test]
    fn parse_expires_spec_unknown_unit_errs() {
        let err = parse_expires_spec("access plus 1 fortnight").unwrap_err();
        assert!(err.to_lowercase().contains("unknown unit"), "got: {}", err);
    }
}
