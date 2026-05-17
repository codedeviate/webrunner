//! Apache `Header` directive (mod_headers subset) for `.htaccess`.
//!
//! Parses `Header [always|onsuccess] <action> <name> [args...]` rules,
//! resolves value placeholders (`%t`, `%D`, `%l`, `%s`, `%H`, `%m`,
//! `%U`), and applies them to response headers in
//! `handle_request`.

#[allow(unused_imports)] // wired in later tasks
use axum::body::Body;
#[allow(unused_imports)] // wired in later tasks
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Response};
use regex::Regex;
use std::time::Instant;

/// One `Header` rule from a `.htaccess` file.
#[allow(dead_code)] // wired in later tasks
#[derive(Debug)]
pub struct HeaderRule {
    pub condition: HeaderCondition,
    pub action: HeaderAction,
    /// `"<file>:<line>"` for warn messages emitted at apply time.
    pub source_loc: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderCondition {
    /// Apply on every response.
    Always,
    /// Apply only when `200 <= status < 300`. Apache default.
    OnSuccess,
}

#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)] // wired in later tasks
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

#[allow(dead_code)] // wired in later tasks
fn flush_literal(literal: &mut String, parts: &mut Vec<TemplatePart>) {
    if !literal.is_empty() {
        parts.push(TemplatePart::Literal(std::mem::take(literal)));
    }
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
}
