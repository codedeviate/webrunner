//! Apache `Header` directive (mod_headers subset) for `.htaccess`.
//!
//! Parses `Header [always|onsuccess] <action> <name> [args...]` rules,
//! resolves value placeholders (`%t`, `%D`, `%l`, `%s`, `%H`, `%m`,
//! `%U`), and applies them to response headers in
//! `handle_request`.

#[allow(unused_imports)] // wired in later tasks
use axum::body::Body;
#[allow(unused_imports)] // HeaderMap, HeaderValue, Response, Body wired in later tasks
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Response};
use regex::Regex;
use std::time::Instant;

/// One `Header` rule from a `.htaccess` file.
#[allow(dead_code)] // fields read in T3+
#[derive(Debug, Clone)]
pub struct HeaderRule {
    pub condition: HeaderCondition,
    pub action: HeaderAction,
    /// `"<file>:<line>"` for warn messages emitted at apply time.
    pub source_loc: String,
}

#[allow(dead_code)] // variants matched in T4+
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderCondition {
    /// Apply on every response.
    Always,
    /// Apply only when `200 <= status < 300`. Apache default.
    OnSuccess,
}

#[allow(dead_code)] // variants matched in T4+
#[derive(Debug, Clone)]
pub enum HeaderAction {
    Set        { name: HeaderName, value: ValueTemplate },
    SetIfEmpty { name: HeaderName, value: ValueTemplate },
    Add        { name: HeaderName, value: ValueTemplate },
    Append     { name: HeaderName, value: ValueTemplate },
    Merge      { name: HeaderName, value: ValueTemplate },
    Unset      { name: HeaderName },
    Echo       { name_regex: Regex },
    Edit {
        name: HeaderName,
        value_regex: Regex,
        replacement: String,
        /// `true` for `edit*` (replace in all values), `false` for `edit` (last value only).
        all: bool,
    },
}

/// Compiled `Header` value with placeholder slots resolved at apply time.
#[allow(dead_code)] // fields read in T4+
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ValueTemplate {
    pub parts: Vec<TemplatePart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplatePart {
    Literal(String),
    /// `%t` — Unix timestamp in microseconds at request start.
    RequestTime,
    /// `%D` — request duration in microseconds.
    Duration,
    /// `%l` — response Content-Length.
    BodySize,
    /// `%s` — response status code.
    Status,
    /// `%H` — protocol, e.g. `HTTP/1.1`.
    Protocol,
    /// `%m` — request method, e.g. `GET`.
    Method,
    /// `%U` — request URL path (no query).
    Url,
}

/// Per-request context passed to `apply_rules`.
#[allow(dead_code)] // constructed in T6
pub struct RequestContext<'a> {
    pub method: &'a Method,
    pub url_path: &'a str,
    /// e.g. `"HTTP/1.1"` (already formatted, not the axum Version enum).
    pub protocol: &'a str,
    pub start_time: Instant,
    /// Wall-clock at request start in Unix microseconds. Used for `%t`.
    pub request_unix_micros: u64,
}

/// Compile a raw value string into a `ValueTemplate`.
///
/// Scans for `%X` sequences. Unknown `%X` (including `%{...}<suffix>`
/// from future subprojects) parse as literal text with a `log::warn!`.
/// `%%` is a literal `%`.
pub fn compile_value_template(raw: &str) -> ValueTemplate {
    let mut parts: Vec<TemplatePart> = Vec::new();
    let mut literal = String::new();

    let mut iter = raw.chars().peekable();
    while let Some(c) = iter.next() {
        if c != '%' {
            literal.push(c);
            continue;
        }
        // Saw '%'. Peek next.
        let next = match iter.peek().copied() {
            Some(n) => n,
            None => {
                // Trailing '%': emit literal and stop.
                literal.push('%');
                break;
            }
        };
        match next {
            '%' => {
                iter.next();
                literal.push('%');
            }
            't' => {
                iter.next();
                flush_literal(&mut literal, &mut parts);
                parts.push(TemplatePart::RequestTime);
            }
            'D' => {
                iter.next();
                flush_literal(&mut literal, &mut parts);
                parts.push(TemplatePart::Duration);
            }
            'l' => {
                iter.next();
                flush_literal(&mut literal, &mut parts);
                parts.push(TemplatePart::BodySize);
            }
            's' => {
                iter.next();
                flush_literal(&mut literal, &mut parts);
                parts.push(TemplatePart::Status);
            }
            'H' => {
                iter.next();
                flush_literal(&mut literal, &mut parts);
                parts.push(TemplatePart::Protocol);
            }
            'm' => {
                iter.next();
                flush_literal(&mut literal, &mut parts);
                parts.push(TemplatePart::Method);
            }
            'U' => {
                iter.next();
                flush_literal(&mut literal, &mut parts);
                parts.push(TemplatePart::Url);
            }
            '{' => {
                // `%{NAME}X` form — reserved for subprojects #3/#4/#6.
                // Consume up to and including the suffix char, log a warn,
                // and emit the whole raw sequence as a literal.
                iter.next(); // consume '{'
                let mut name = String::new();
                let mut closed = false;
                for c in iter.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c);
                }
                let suffix = if closed { iter.next() } else { None };
                let raw_seq = match suffix {
                    Some(s) => format!("%{{{}}}{}", name, s),
                    None => format!("%{{{}", name),
                };
                log::warn!(
                    "[Header] unsupported placeholder '{}' (deferred subproject); emitting as literal",
                    raw_seq
                );
                literal.push_str(&raw_seq);
            }
            other => {
                // Unknown single-char placeholder. Warn and emit literally.
                iter.next();
                log::warn!(
                    "[Header] unknown placeholder '%{}'; emitting as literal",
                    other
                );
                literal.push('%');
                literal.push(other);
            }
        }
    }
    flush_literal(&mut literal, &mut parts);
    ValueTemplate { parts }
}

fn flush_literal(literal: &mut String, parts: &mut Vec<TemplatePart>) {
    if !literal.is_empty() {
        parts.push(TemplatePart::Literal(std::mem::take(literal)));
    }
}

/// Apply parsed `HeaderRule`s to the response.
///
/// Rules are applied in order. Each rule's condition is checked against
/// the response status:
/// - `Always`: always apply.
/// - `OnSuccess`: apply only when `200 <= status < 300`.
///
/// Apply-time errors (invalid `HeaderName`/`HeaderValue`, regex
/// substitution producing invalid bytes) log a `warn!` with the rule's
/// `source_loc` and skip the rule.
#[allow(dead_code)] // wired into handle_request in T6
pub fn apply_rules(
    response: &mut Response<Body>,
    rules: &[HeaderRule],
    request_headers: &HeaderMap,
    ctx: &RequestContext<'_>,
) {
    for rule in rules {
        if !condition_matches(rule.condition, response.status().as_u16()) {
            continue;
        }
        apply_one(response, rule, request_headers, ctx);
    }
}

#[allow(dead_code)] // called via apply_rules, wired in T6
fn condition_matches(c: HeaderCondition, status: u16) -> bool {
    match c {
        HeaderCondition::Always => true,
        HeaderCondition::OnSuccess => (200..300).contains(&status),
    }
}

#[allow(dead_code)] // called via apply_rules, wired in T6
fn apply_one(
    response: &mut Response<Body>,
    rule: &HeaderRule,
    request_headers: &HeaderMap,
    ctx: &RequestContext<'_>,
) {
    match &rule.action {
        HeaderAction::Set { name, value } => {
            let raw = resolve_template(value, response, ctx);
            match HeaderValue::from_str(&raw) {
                Ok(hv) => {
                    response.headers_mut().insert(name.clone(), hv);
                }
                Err(e) => log::warn!(
                    "[Header] {}: invalid value for {}: {}",
                    rule.source_loc, name, e
                ),
            }
        }
        HeaderAction::SetIfEmpty { name, value } => {
            if response.headers().get(name).is_some() {
                return;
            }
            let raw = resolve_template(value, response, ctx);
            match HeaderValue::from_str(&raw) {
                Ok(hv) => {
                    response.headers_mut().insert(name.clone(), hv);
                }
                Err(e) => log::warn!(
                    "[Header] {}: invalid value for {}: {}",
                    rule.source_loc, name, e
                ),
            }
        }
        HeaderAction::Add { name, value } => {
            let raw = resolve_template(value, response, ctx);
            match HeaderValue::from_str(&raw) {
                Ok(hv) => {
                    response.headers_mut().append(name.clone(), hv);
                }
                Err(e) => log::warn!(
                    "[Header] {}: invalid value for {}: {}",
                    rule.source_loc, name, e
                ),
            }
        }
        HeaderAction::Append { name, value } => {
            let raw = resolve_template(value, response, ctx);
            let combined = match response.headers().get(name) {
                Some(existing) => {
                    let existing_str = existing.to_str().unwrap_or("");
                    format!("{}, {}", existing_str, raw)
                }
                None => raw,
            };
            match HeaderValue::from_str(&combined) {
                Ok(hv) => {
                    response.headers_mut().insert(name.clone(), hv);
                }
                Err(e) => log::warn!(
                    "[Header] {}: invalid value for {}: {}",
                    rule.source_loc, name, e
                ),
            }
        }
        HeaderAction::Merge { name, value } => {
            let raw = resolve_template(value, response, ctx);
            let already_present = response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|existing| {
                    existing
                        .split(',')
                        .map(|part| part.trim())
                        .any(|part| part == raw)
                })
                .unwrap_or(false);
            if already_present {
                return;
            }
            let combined = match response.headers().get(name) {
                Some(existing) => format!("{}, {}", existing.to_str().unwrap_or(""), raw),
                None => raw,
            };
            match HeaderValue::from_str(&combined) {
                Ok(hv) => {
                    response.headers_mut().insert(name.clone(), hv);
                }
                Err(e) => log::warn!(
                    "[Header] {}: invalid value for {}: {}",
                    rule.source_loc, name, e
                ),
            }
        }
        HeaderAction::Unset { name } => {
            // `HeaderMap::remove` returns the FIRST value and drops the rest.
            // To remove ALL values we loop until none remain.
            while response.headers_mut().remove(name).is_some() {}
        }
        HeaderAction::Echo { name_regex } => {
            for (name, value) in request_headers.iter() {
                if name_regex.is_match(name.as_str()) {
                    response.headers_mut().append(name.clone(), value.clone());
                }
            }
        }
        HeaderAction::Edit { name, value_regex, replacement, all } => {
            // Collect existing values (cloned), then remove them all,
            // then re-append after substitution. Apache's `edit` (no
            // star) substitutes only the LAST value; `edit*` substitutes
            // all values.
            let existing: Vec<HeaderValue> = response
                .headers()
                .get_all(name)
                .iter()
                .cloned()
                .collect();
            // Drain all existing values.
            while response.headers_mut().remove(name).is_some() {}
            let last_idx = existing.len().saturating_sub(1);
            for (idx, hv) in existing.into_iter().enumerate() {
                let s = hv.to_str().unwrap_or("").to_string();
                let new_s = if *all || idx == last_idx {
                    value_regex.replace_all(&s, replacement.as_str()).to_string()
                } else {
                    s
                };
                match HeaderValue::from_str(&new_s) {
                    Ok(new_hv) => {
                        response.headers_mut().append(name.clone(), new_hv);
                    }
                    Err(e) => log::warn!(
                        "[Header] {}: edit produced invalid value for {}: {}",
                        rule.source_loc, name, e
                    ),
                }
            }
        }
    }
}

#[allow(dead_code)] // called via apply_one, wired in T6
fn resolve_template(
    template: &ValueTemplate,
    response: &Response<Body>,
    ctx: &RequestContext<'_>,
) -> String {
    let mut out = String::new();
    for part in &template.parts {
        match part {
            TemplatePart::Literal(s) => out.push_str(s),
            TemplatePart::RequestTime => out.push_str(&ctx.request_unix_micros.to_string()),
            TemplatePart::Duration => {
                let micros = ctx.start_time.elapsed().as_micros();
                out.push_str(&(micros as u64).to_string());
            }
            TemplatePart::BodySize => {
                let size = response
                    .headers()
                    .get(axum::http::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("0");
                out.push_str(size);
            }
            TemplatePart::Status => out.push_str(&response.status().as_u16().to_string()),
            TemplatePart::Protocol => out.push_str(ctx.protocol),
            TemplatePart::Method => out.push_str(ctx.method.as_str()),
            TemplatePart::Url => out.push_str(ctx.url_path),
        }
    }
    out
}

/// Parse the portion of a `.htaccess` line that follows the `Header`
/// keyword (the keyword itself is stripped by the caller in `htaccess.rs`).
///
/// Grammar:
/// ```text
/// rest = [condition] action name [args...]
/// condition = "always" | "onsuccess"
/// action = "set" | "setifempty" | "add" | "append" | "merge"
///        | "unset" | "echo" | "edit" | "edit*"
/// ```
///
/// Quoted values (`"..."`) preserve internal whitespace. Backslash
/// escapes inside quotes are `\"` and `\\`.
///
/// Returns `Err(reason)` on malformed input; the caller logs the warn
/// with the source location.
pub fn parse_header_line(rest: &str, source_loc: &str) -> Result<HeaderRule, String> {
    let mut tokens = tokenize_header_line(rest)?;
    if tokens.is_empty() {
        return Err("missing action".to_string());
    }

    // Optional condition.
    let condition = match tokens[0].to_ascii_lowercase().as_str() {
        "always" => {
            tokens.remove(0);
            HeaderCondition::Always
        }
        "onsuccess" => {
            tokens.remove(0);
            HeaderCondition::OnSuccess
        }
        _ => HeaderCondition::OnSuccess,
    };

    if tokens.is_empty() {
        return Err("missing action".to_string());
    }
    let action_word = tokens.remove(0).to_ascii_lowercase();

    // `edit*` action — handle before the lowercase match.
    if action_word == "edit*" {
        let action = parse_edit_action(&tokens, true)?;
        return Ok(HeaderRule {
            condition,
            action,
            source_loc: source_loc.to_string(),
        });
    }

    let action = match action_word.as_str() {
        "set" => parse_value_action(&mut tokens, |name, value| HeaderAction::Set { name, value })?,
        "setifempty" => parse_value_action(&mut tokens, |name, value| HeaderAction::SetIfEmpty { name, value })?,
        "add" => parse_value_action(&mut tokens, |name, value| HeaderAction::Add { name, value })?,
        "append" => parse_value_action(&mut tokens, |name, value| HeaderAction::Append { name, value })?,
        "merge" => parse_value_action(&mut tokens, |name, value| HeaderAction::Merge { name, value })?,
        "unset" => {
            let name_tok = tokens.first().ok_or_else(|| "unset: missing header name".to_string())?;
            let name = HeaderName::from_bytes(name_tok.as_bytes())
                .map_err(|e| format!("unset: invalid header name '{}': {}", name_tok, e))?;
            if tokens.len() > 1 {
                return Err("unset: unexpected extra arguments".to_string());
            }
            HeaderAction::Unset { name }
        }
        "echo" => {
            let pat_tok = tokens.first().ok_or_else(|| "echo: missing header-name regex".to_string())?;
            // Apache anchors echo regexes whole-string. Header names are
            // case-insensitive per RFC 9110 §5.1, so wrap as `(?i:...)`.
            let anchored = format!("^(?i:{})$", pat_tok);
            let regex = Regex::new(&anchored)
                .map_err(|e| format!("echo: invalid regex '{}': {}", pat_tok, e))?;
            if tokens.len() > 1 {
                return Err("echo: unexpected extra arguments".to_string());
            }
            HeaderAction::Echo { name_regex: regex }
        }
        "edit" => parse_edit_action(&tokens, false)?,
        other => return Err(format!("unknown action '{}'", other)),
    };

    Ok(HeaderRule {
        condition,
        action,
        source_loc: source_loc.to_string(),
    })
}

fn parse_value_action<F>(tokens: &mut Vec<String>, ctor: F) -> Result<HeaderAction, String>
where
    F: FnOnce(HeaderName, ValueTemplate) -> HeaderAction,
{
    let name_tok = if tokens.is_empty() {
        return Err("missing header name".to_string());
    } else {
        tokens.remove(0)
    };
    let value_tok = if tokens.is_empty() {
        return Err(format!("missing value for header '{}'", name_tok));
    } else {
        tokens.remove(0)
    };
    if !tokens.is_empty() {
        return Err("unexpected extra arguments".to_string());
    }
    let name = HeaderName::from_bytes(name_tok.as_bytes())
        .map_err(|e| format!("invalid header name '{}': {}", name_tok, e))?;
    let value = compile_value_template(&value_tok);
    Ok(ctor(name, value))
}

fn parse_edit_action(tokens: &[String], all: bool) -> Result<HeaderAction, String> {
    let name_tok = tokens.first().ok_or_else(|| "edit: missing header name".to_string())?.clone();
    let regex_tok = tokens.get(1).ok_or_else(|| "edit: missing regex".to_string())?.clone();
    let replacement_tok = tokens.get(2).ok_or_else(|| "edit: missing replacement".to_string())?.clone();
    if tokens.len() > 3 {
        return Err("edit: unexpected extra arguments".to_string());
    }
    let name = HeaderName::from_bytes(name_tok.as_bytes())
        .map_err(|e| format!("edit: invalid header name '{}': {}", name_tok, e))?;
    let value_regex = Regex::new(&regex_tok)
        .map_err(|e| format!("edit: invalid regex '{}': {}", regex_tok, e))?;
    Ok(HeaderAction::Edit {
        name,
        value_regex,
        replacement: replacement_tok,
        all,
    })
}

/// Split a Header-directive line into tokens, preserving quoted spans.
///
/// Outside quotes: whitespace separates tokens. Inside double quotes:
/// whitespace is preserved; `\"` and `\\` are escapes. Single quotes
/// have no special meaning. Unclosed quotes return Err.
fn tokenize_header_line(s: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut iter = s.chars().peekable();

    while let Some(c) = iter.next() {
        if in_quote {
            match c {
                '\\' => match iter.next() {
                    Some(esc) => cur.push(esc),
                    None => return Err("trailing backslash inside quoted string".to_string()),
                },
                '"' => {
                    in_quote = false;
                    out.push(std::mem::take(&mut cur));
                }
                other => cur.push(other),
            }
        } else if c == '"' {
            in_quote = true;
            // Apache fuses bare-quote-adjacent input (`foo"bar baz"` -> `foobar baz`):
            // we do NOT push the current accumulator here.
        } else if c.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }

    if in_quote {
        return Err("unclosed quoted string".to_string());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_literal_only() {
        let t = compile_value_template("bar");
        assert_eq!(t.parts, vec![TemplatePart::Literal("bar".to_string())]);
    }

    #[test]
    fn template_percent_escape() {
        let t = compile_value_template("50%% off");
        assert_eq!(t.parts, vec![TemplatePart::Literal("50% off".to_string())]);
    }

    #[test]
    fn template_all_basic_placeholders() {
        let t = compile_value_template("%m %U %s %H %D %l %t");
        assert_eq!(
            t.parts,
            vec![
                TemplatePart::Method,
                TemplatePart::Literal(" ".to_string()),
                TemplatePart::Url,
                TemplatePart::Literal(" ".to_string()),
                TemplatePart::Status,
                TemplatePart::Literal(" ".to_string()),
                TemplatePart::Protocol,
                TemplatePart::Literal(" ".to_string()),
                TemplatePart::Duration,
                TemplatePart::Literal(" ".to_string()),
                TemplatePart::BodySize,
                TemplatePart::Literal(" ".to_string()),
                TemplatePart::RequestTime,
            ]
        );
    }

    #[test]
    fn template_unknown_placeholder_falls_back_to_literal() {
        let t = compile_value_template("hi %q there");
        assert_eq!(
            t.parts,
            vec![TemplatePart::Literal("hi %q there".to_string())],
        );

        let u = compile_value_template("ip=%{X-Forwarded-For}i!");
        assert_eq!(
            u.parts,
            vec![TemplatePart::Literal("ip=%{X-Forwarded-For}i!".to_string())],
        );
    }

    #[test]
    fn parse_set_simple() {
        let r = parse_header_line("set X-Foo bar", "test:1").unwrap();
        assert!(matches!(r.condition, HeaderCondition::OnSuccess));
        match r.action {
            HeaderAction::Set { name, value } => {
                assert_eq!(name.as_str(), "x-foo");
                assert_eq!(value.parts, vec![TemplatePart::Literal("bar".to_string())]);
            }
            _ => panic!("expected Set action"),
        }
    }

    #[test]
    fn parse_set_quoted_value() {
        let r = parse_header_line(r#"set X-Foo "a b c""#, "test:1").unwrap();
        match r.action {
            HeaderAction::Set { value, .. } => {
                assert_eq!(value.parts, vec![TemplatePart::Literal("a b c".to_string())]);
            }
            _ => panic!("expected Set action"),
        }
    }

    #[test]
    fn parse_set_with_always() {
        let r = parse_header_line("always set X-Foo bar", "test:1").unwrap();
        assert!(matches!(r.condition, HeaderCondition::Always));
        assert!(matches!(r.action, HeaderAction::Set { .. }));
    }

    #[test]
    fn parse_unset_no_value() {
        let r = parse_header_line("unset X-Powered-By", "test:1").unwrap();
        match r.action {
            HeaderAction::Unset { name } => assert_eq!(name.as_str(), "x-powered-by"),
            _ => panic!("expected Unset action"),
        }
    }

    #[test]
    fn parse_edit() {
        let r = parse_header_line("edit Location ^/foo /bar", "test:1").unwrap();
        match r.action {
            HeaderAction::Edit { name, value_regex, replacement, all } => {
                assert_eq!(name.as_str(), "location");
                assert_eq!(value_regex.as_str(), "^/foo");
                assert_eq!(replacement, "/bar");
                assert!(!all);
            }
            _ => panic!("expected Edit action"),
        }

        let r_star = parse_header_line("edit* X-Foo a b", "test:1").unwrap();
        assert!(matches!(r_star.action, HeaderAction::Edit { all: true, .. }));
    }

    #[test]
    fn parse_echo_regex() {
        let r = parse_header_line(r#"echo "X-Forwarded-.*""#, "test:1").unwrap();
        match r.action {
            HeaderAction::Echo { name_regex } => {
                // Apache anchors echo regexes; we wrap input as ^(?i:...)$ so
                // matching against lowercase axum header names is correct.
                assert!(name_regex.is_match("x-forwarded-for"));
                assert!(name_regex.is_match("x-forwarded-proto"));
                assert!(name_regex.is_match("X-Forwarded-For")); // also matches mixed-case
                assert!(!name_regex.is_match("y-forwarded-for"));
            }
            _ => panic!("expected Echo action"),
        }
    }

    #[test]
    fn parse_malformed_missing_action() {
        let err = parse_header_line("set", "test:1").unwrap_err();
        assert!(err.to_lowercase().contains("missing"));
    }

    use axum::body::Body;
    use axum::http::{Method, Response, StatusCode};
    use std::time::Instant;

    fn empty_request_ctx() -> (Method, String, String, Instant, u64) {
        (
            Method::GET,
            "/test".to_string(),
            "HTTP/1.1".to_string(),
            Instant::now(),
            1_700_000_000_000_000u64,
        )
    }

    fn make_ctx<'a>(m: &'a Method, u: &'a str, p: &'a str, start: Instant, utc: u64) -> RequestContext<'a> {
        RequestContext { method: m, url_path: u, protocol: p, start_time: start, request_unix_micros: utc }
    }

    fn rule(action: HeaderAction, condition: HeaderCondition) -> HeaderRule {
        HeaderRule { condition, action, source_loc: "test:1".to_string() }
    }

    fn make_response(status: StatusCode) -> Response<Body> {
        Response::builder().status(status).body(Body::empty()).unwrap()
    }

    #[test]
    fn apply_set_overrides_existing() {
        let mut resp = make_response(StatusCode::OK);
        resp.headers_mut().insert("x-foo", HeaderValue::from_static("old"));
        let action = HeaderAction::Set {
            name: HeaderName::from_static("x-foo"),
            value: compile_value_template("new"),
        };
        let rules = vec![rule(action, HeaderCondition::OnSuccess)];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);
        assert_eq!(resp.headers().get("x-foo").unwrap(), "new");
    }

    #[test]
    fn apply_set_if_empty_skips_when_present() {
        let mut resp = make_response(StatusCode::OK);
        resp.headers_mut().insert("x-foo", HeaderValue::from_static("kept"));
        let rules = vec![rule(
            HeaderAction::SetIfEmpty {
                name: HeaderName::from_static("x-foo"),
                value: compile_value_template("new"),
            },
            HeaderCondition::OnSuccess,
        )];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);
        assert_eq!(resp.headers().get("x-foo").unwrap(), "kept");
    }

    #[test]
    fn apply_add_appends_multi_value() {
        let mut resp = make_response(StatusCode::OK);
        let rules = vec![
            rule(
                HeaderAction::Add {
                    name: HeaderName::from_static("set-cookie"),
                    value: compile_value_template("a=1"),
                },
                HeaderCondition::OnSuccess,
            ),
            rule(
                HeaderAction::Add {
                    name: HeaderName::from_static("set-cookie"),
                    value: compile_value_template("b=2"),
                },
                HeaderCondition::OnSuccess,
            ),
        ];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);
        let values: Vec<&str> = resp.headers().get_all("set-cookie").iter().map(|v| v.to_str().unwrap()).collect();
        assert_eq!(values, vec!["a=1", "b=2"]);
    }

    #[test]
    fn apply_append_concatenates_with_comma() {
        let mut resp = make_response(StatusCode::OK);
        resp.headers_mut().insert("x-foo", HeaderValue::from_static("first"));
        let rules = vec![rule(
            HeaderAction::Append {
                name: HeaderName::from_static("x-foo"),
                value: compile_value_template("second"),
            },
            HeaderCondition::OnSuccess,
        )];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);
        assert_eq!(resp.headers().get("x-foo").unwrap(), "first, second");
    }

    #[test]
    fn apply_merge_skips_duplicate() {
        let mut resp = make_response(StatusCode::OK);
        resp.headers_mut().insert("x-foo", HeaderValue::from_static("a, b"));
        let rules = vec![rule(
            HeaderAction::Merge {
                name: HeaderName::from_static("x-foo"),
                value: compile_value_template("b"),
            },
            HeaderCondition::OnSuccess,
        )];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);
        // 'b' already present; merge is a no-op.
        assert_eq!(resp.headers().get("x-foo").unwrap(), "a, b");
    }

    #[test]
    fn apply_unset_removes_all_values() {
        let mut resp = make_response(StatusCode::OK);
        resp.headers_mut().append("set-cookie", HeaderValue::from_static("a=1"));
        resp.headers_mut().append("set-cookie", HeaderValue::from_static("b=2"));
        let rules = vec![rule(
            HeaderAction::Unset { name: HeaderName::from_static("set-cookie") },
            HeaderCondition::OnSuccess,
        )];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);
        assert!(resp.headers().get_all("set-cookie").iter().next().is_none());
    }

    #[test]
    fn apply_onsuccess_skips_on_404_and_always_applies_on_404() {
        // OnSuccess rule on a 404: skipped.
        let mut resp = make_response(StatusCode::NOT_FOUND);
        let rules = vec![rule(
            HeaderAction::Set {
                name: HeaderName::from_static("x-onsuccess"),
                value: compile_value_template("v"),
            },
            HeaderCondition::OnSuccess,
        )];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);
        assert!(resp.headers().get("x-onsuccess").is_none());

        // Always rule on a 404: applied.
        let mut resp2 = make_response(StatusCode::NOT_FOUND);
        let rules2 = vec![rule(
            HeaderAction::Set {
                name: HeaderName::from_static("x-always"),
                value: compile_value_template("v"),
            },
            HeaderCondition::Always,
        )];
        apply_rules(&mut resp2, &rules2, &req, &ctx);
        assert_eq!(resp2.headers().get("x-always").unwrap(), "v");
    }

    #[test]
    fn resolve_method_and_url_and_protocol() {
        let mut resp = make_response(StatusCode::OK);
        let rules = vec![rule(
            HeaderAction::Set {
                name: HeaderName::from_static("x-info"),
                value: compile_value_template("%m %U %H"),
            },
            HeaderCondition::OnSuccess,
        )];
        let req = HeaderMap::new();
        let m = Method::POST;
        let u = "/api/x";
        let p = "HTTP/1.1";
        let ctx = make_ctx(&m, u, p, Instant::now(), 0);
        apply_rules(&mut resp, &rules, &req, &ctx);
        assert_eq!(resp.headers().get("x-info").unwrap(), "POST /api/x HTTP/1.1");
    }

    #[test]
    fn resolve_status_from_response() {
        let mut resp = make_response(StatusCode::CREATED);
        let rules = vec![rule(
            HeaderAction::Set {
                name: HeaderName::from_static("x-status"),
                value: compile_value_template("%s"),
            },
            HeaderCondition::Always,
        )];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);
        assert_eq!(resp.headers().get("x-status").unwrap(), "201");
    }

    #[test]
    fn apply_echo_copies_matching_request_headers() {
        let mut resp = make_response(StatusCode::OK);
        let mut req = HeaderMap::new();
        req.insert("x-forwarded-for", HeaderValue::from_static("1.2.3.4"));
        req.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        req.insert("y-other", HeaderValue::from_static("nope"));

        let rules = vec![rule(
            HeaderAction::Echo {
                name_regex: Regex::new("^(?i:X-Forwarded-.*)$").unwrap(),
            },
            HeaderCondition::OnSuccess,
        )];
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);

        assert_eq!(resp.headers().get("x-forwarded-for").unwrap(), "1.2.3.4");
        assert_eq!(resp.headers().get("x-forwarded-proto").unwrap(), "https");
        assert!(resp.headers().get("y-other").is_none());
    }

    #[test]
    fn apply_edit_replaces_last_value() {
        let mut resp = make_response(StatusCode::OK);
        resp.headers_mut().append("x-foo", HeaderValue::from_static("alpha"));
        resp.headers_mut().append("x-foo", HeaderValue::from_static("beta"));

        let rules = vec![rule(
            HeaderAction::Edit {
                name: HeaderName::from_static("x-foo"),
                value_regex: Regex::new("be").unwrap(),
                replacement: "BE".to_string(),
                all: false,
            },
            HeaderCondition::OnSuccess,
        )];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);

        let values: Vec<&str> = resp.headers().get_all("x-foo").iter().map(|v| v.to_str().unwrap()).collect();
        // First value untouched; second value substituted.
        assert_eq!(values, vec!["alpha", "BEta"]);
    }

    #[test]
    fn apply_edit_star_replaces_all_values() {
        let mut resp = make_response(StatusCode::OK);
        resp.headers_mut().append("x-foo", HeaderValue::from_static("ab"));
        resp.headers_mut().append("x-foo", HeaderValue::from_static("ac"));

        let rules = vec![rule(
            HeaderAction::Edit {
                name: HeaderName::from_static("x-foo"),
                value_regex: Regex::new("a").unwrap(),
                replacement: "A".to_string(),
                all: true,
            },
            HeaderCondition::OnSuccess,
        )];
        let req = HeaderMap::new();
        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);

        let values: Vec<&str> = resp.headers().get_all("x-foo").iter().map(|v| v.to_str().unwrap()).collect();
        assert_eq!(values, vec!["Ab", "Ac"]);
    }

    #[test]
    fn apply_echo_matches_allcaps_abbreviation_in_header_name() {
        // Regression: previous title-casing implementation produced
        // `X-Xss-Protection` from the lowercase axum name, which fails
        // to match a user regex `X-XSS-Protection`. The case-insensitive
        // regex wrap (`(?i:...)`) fixes this.
        let mut resp = make_response(StatusCode::OK);
        let mut req = HeaderMap::new();
        req.insert("x-xss-protection", HeaderValue::from_static("1; mode=block"));

        // Parse via parse_header_line so the same (?i:...) wrapping the
        // production code path uses is exercised.
        let r = parse_header_line(r#"echo "X-XSS-Protection""#, "test:1").unwrap();
        let rules = vec![r];

        let (m, u, p, s, utc) = empty_request_ctx();
        let ctx = make_ctx(&m, &u, &p, s, utc);
        apply_rules(&mut resp, &rules, &req, &ctx);

        assert_eq!(
            resp.headers().get("x-xss-protection").unwrap(),
            "1; mode=block"
        );
    }

    #[test]
    fn resolve_duration_is_decimal_microseconds() {
        let start = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut resp = make_response(StatusCode::OK);
        let rules = vec![rule(
            HeaderAction::Set {
                name: HeaderName::from_static("x-d"),
                value: compile_value_template("%D"),
            },
            HeaderCondition::Always,
        )];
        let req = HeaderMap::new();
        let m = Method::GET;
        let u = "/";
        let p = "HTTP/1.1";
        let ctx = make_ctx(&m, u, p, start, 0);
        apply_rules(&mut resp, &rules, &req, &ctx);
        let raw = resp.headers().get("x-d").unwrap().to_str().unwrap();
        let micros: u64 = raw.parse().expect("not a number");
        assert!(micros >= 10_000, "duration {} < 10ms", micros);
    }
}
