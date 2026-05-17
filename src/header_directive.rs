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
#[derive(Debug)]
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
#[derive(Debug)]
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
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ValueTemplate {
    pub parts: Vec<TemplatePart>,
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
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
#[allow(dead_code)]
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
#[allow(dead_code)] // wired in T3 (htaccess integration)
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
            // Apache anchors echo regexes whole-string.
            let anchored = format!("^(?:{})$", pat_tok);
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
                // Apache anchors echo regexes; we wrap input as ^(?:...)$.
                assert!(name_regex.is_match("X-Forwarded-For"));
                assert!(name_regex.is_match("X-Forwarded-Proto"));
                assert!(!name_regex.is_match("Y-Forwarded-For"));
            }
            _ => panic!("expected Echo action"),
        }
    }

    #[test]
    fn parse_malformed_missing_action() {
        let err = parse_header_line("set", "test:1").unwrap_err();
        assert!(err.to_lowercase().contains("missing"));
    }
}
