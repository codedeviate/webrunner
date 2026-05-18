// Request pipeline orchestrator

use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{Request, Response, header, HeaderMap},
};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, UNIX_EPOCH};

use crate::cli::CliConfig;
use crate::htaccess::parse_htaccess_for_path;
use crate::auth::{parse_htpasswd_file, check_credentials};
use crate::rewrite::{apply_rewrites, RewriteResult};
use crate::cgi::{is_cgi_ext, build_cgi_env, run_cgi};
use crate::static_files::{resolve_index, build_etag, http_date, format_directory_listing, parse_range, parse_imf_fixdate};
use crate::mime::mime_for_ext_owned;

#[derive(Clone)]
pub struct AppState {
    pub root: PathBuf,
    pub config: Arc<CliConfig>,
    pub access_log: Option<Arc<crate::access_log::AccessLog>>,
}

/// Effective auth settings for a request: either the LAST matching
/// `AuthScope`, or the flat (unscoped) fields when no scope matches.
#[derive(Debug, Clone)]
struct EffectiveAuth {
    auth_required: bool,
    auth_name: Option<String>,
    auth_user_file: Option<String>,
}

fn resolve_auth_scope(
    htaccess: &crate::htaccess::HtaccessConfig,
    filename: &str,
) -> EffectiveAuth {
    // Walk scopes in declaration order; LAST match wins (Apache merge order).
    let mut chosen: Option<&crate::htaccess::AuthScope> = None;
    for scope in &htaccess.auth_scopes {
        if scope.file_scope.iter().all(|re| re.is_match(filename)) {
            chosen = Some(scope);
        }
    }
    if let Some(s) = chosen {
        return EffectiveAuth {
            auth_required: s.auth_required,
            auth_name: s.auth_name.clone(),
            auth_user_file: s.auth_user_file.clone(),
        };
    }
    EffectiveAuth {
        auth_required: htaccess.auth_required,
        auth_name: htaccess.auth_name.clone(),
        auth_user_file: htaccess.auth_user_file.clone(),
    }
}

pub async fn handle_request(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
) -> Response<Body> {
    let request_start = Instant::now();
    let request_unix_micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);
    let (parts, body) = req.into_parts();
    let method = parts.method.to_string();
    let uri = parts.uri.clone();
    let path_str = uri.path();
    let query = uri.query().unwrap_or("");
    let req_headers = parts.headers.clone();

    // Collect request body bytes (for CGI POST)
    use http_body_util::BodyExt;
    let body_bytes: Vec<u8> = body.collect().await
        .map(|c| c.to_bytes().to_vec())
        .unwrap_or_default();

    // Resolve filesystem path (prevent path traversal)
    let rel_path = path_str.trim_start_matches('/');
    let fs_path = match safe_join(&state.root, rel_path) {
        Some(p) => p,
        None => return error_response(400, "Bad Request"),
    };

    // Determine the directory for .htaccess loading
    let htaccess_dir = if fs_path.is_dir() {
        fs_path.clone()
    } else {
        fs_path.parent().unwrap_or(&state.root).to_path_buf()
    };

    // Load .htaccess configuration
    let htaccess = match parse_htaccess_for_path(&state.root, &htaccess_dir) {
        Ok(cfg) => cfg,
        Err(e) => {
            log::error!("[.htaccess] error: {}", e);
            crate::htaccess::HtaccessConfig::default()
        }
    };

    let mut authenticated_user: Option<String> = None;
    // Redirect/Rewrite
    let mut response = match apply_rewrites(path_str, query, &htaccess) {
        RewriteResult::Redirect { status, location } => {
            let mut builder = Response::builder().status(status);
            if let Some(loc) = location {
                builder = builder.header(header::LOCATION, loc);
            }
            builder.body(Body::empty()).unwrap()
        }
        RewriteResult::Rewrite(new_path) => {
            let new_rel = new_path.trim_start_matches('/');
            match safe_join(&state.root, new_rel) {
                Some(new_fs) => {
                    serve_path(
                        &state, &new_fs, &new_path, query, &method, &req_headers,
                        &htaccess, body_bytes, peer_addr.ip(),
                    )
                    .await
                }
                None => error_response(400, "Bad Request"),
            }
        }
        RewriteResult::None => {
            serve_path(
                &state, &fs_path, path_str, query, &method, &req_headers,
                &htaccess, body_bytes, peer_addr.ip(),
            )
            .await
        }
    };

    // Compute the basename used for scope matching. Uses the ORIGINAL
    // request URL (path_str), so post-rewrite paths are NOT visible
    // here. For directory listings, 404s, and other paths with an
    // empty URL leaf, fall back to the resolved fs_path's leaf.
    let filename: String = {
        let url_basename = path_str.rsplit('/').next().unwrap_or("");
        if url_basename.is_empty() {
            // Directory request — use the fs_path leaf when available.
            fs_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        } else {
            url_basename.to_string()
        }
    };

    // Scope-aware auth check. Runs AFTER serve_path so the file is
    // resolved; the LAST matching AuthScope (or the flat fields)
    // applies.
    //
    // Note on Apache divergence: Apache evaluates auth before the
    // filesystem handler reads the file body. webrunner runs the
    // handler first and the auth check overwrites the response on
    // failure — observable behaviour matches Apache (a failed auth
    // returns 401, never reveals 404/file bytes), but internally
    // there's a wasted read on auth-fail paths. Acceptable for a
    // dev server.
    //
    // Note on filename source: the basename used for scope matching
    // comes from the ORIGINAL request URL, not the post-rewrite
    // path. For rewrites that preserve the extension (the common
    // case) this is identical; for rewrites that change the
    // extension, scope matches against what the user requested,
    // not what was served.
    let auth = resolve_auth_scope(&htaccess, &filename);
    if auth.auth_required {
        if let Some(user_file) = &auth.auth_user_file {
            match check_basic_auth(&req_headers, user_file) {
                Some(user) => authenticated_user = Some(user),
                None => {
                    let realm = auth.auth_name.as_deref().unwrap_or("Restricted");
                    return auth_challenge_response(realm);
                }
            }
        } else {
            log::warn!("[auth] auth_required but no AuthUserFile configured, denying access");
            return error_response(403, "Forbidden");
        }
    }

    crate::expires::apply_expires(&mut response, &htaccess.expires);

    let protocol = format!("{:?}", parts.version);
    let header_ctx = crate::header_directive::RequestContext {
        method: &parts.method,
        url_path: path_str,
        protocol: &protocol,
        start_time: request_start,
        request_unix_micros,
        filename: &filename,
    };
    crate::header_directive::apply_rules(
        &mut response,
        &htaccess.header_rules,
        &req_headers,
        &header_ctx,
    );

    if let Some(user) = authenticated_user {
        response
            .extensions_mut()
            .insert(crate::access_log::AuthUser(user));
    }
    response
}

#[allow(clippy::too_many_arguments)]
async fn serve_path(
    state: &AppState,
    fs_path: &Path,
    req_path: &str,
    query: &str,
    method: &str,
    req_headers: &HeaderMap,
    htaccess: &crate::htaccess::HtaccessConfig,
    body_bytes: Vec<u8>,
    peer_ip: IpAddr,
) -> Response<Body> {
    // Directory handling
    if fs_path.is_dir() {
        if let Some(index_path) = resolve_index(fs_path, &htaccess.directory_index) {
            let ext = index_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if is_cgi_ext(ext, &state.config.cgi, &state.config.no_cgi) {
                return run_cgi_handler(state, &index_path, ext, req_path, query, method, req_headers, body_bytes, peer_ip).await;
            } else {
                return serve_static_file(state, &index_path, req_path, req_headers, htaccess).await;
            }
        }

        let show = htaccess.show_indexes && !state.config.no_index;
        if show {
            match format_directory_listing(fs_path, req_path) {
                Ok(html) => {
                    return Response::builder()
                        .status(200)
                        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                        .header(header::CONTENT_LENGTH, html.len())
                        .body(Body::from(html))
                        .unwrap();
                }
                Err(e) => {
                    log::error!("[static] directory listing error: {}", e);
                    return error_response(500, "Internal Server Error");
                }
            }
        } else {
            return error_response(403, "Forbidden");
        }
    }

    // CGI script
    let ext = fs_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if is_cgi_ext(ext, &state.config.cgi, &state.config.no_cgi) {
        return run_cgi_handler(state, fs_path, ext, req_path, query, method, req_headers, body_bytes, peer_ip).await;
    }

    // Static file
    if !fs_path.exists() {
        return error_response(404, "Not Found");
    }

    serve_static_file(state, fs_path, req_path, req_headers, htaccess).await
}

async fn serve_static_file(
    state: &AppState,
    path: &Path,
    _req_path: &str,
    req_headers: &HeaderMap,
    htaccess: &crate::htaccess::HtaccessConfig,
) -> Response<Body> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return error_response(404, "Not Found"),
    };

    let file_size = meta.len();
    let modified = meta.modified().unwrap_or(UNIX_EPOCH);
    let modified_secs = modified.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let last_modified = http_date(modified);

    // Compute content_type up front — needed for compression eligibility AND response.
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let content_type_raw = mime_for_ext_owned(ext, &htaccess.add_types, filename);
    let content_type = if let Some(charset) = &htaccess.add_default_charset {
        if !content_type_raw.contains("charset") {
            format!("{}; charset={}", content_type_raw, charset)
        } else {
            content_type_raw
        }
    } else {
        content_type_raw
    };

    let base_etag = build_etag(file_size, modified_secs);

    // Decide compression eligibility BEFORE finalising the ETag.
    let want_compress = state.config.compression == crate::compression::Compression::On
        && crate::compression::is_compressible(&content_type)
        && file_size >= crate::compression::MIN_COMPRESS_SIZE
        && !req_headers.contains_key(header::RANGE);
    let chosen_encoding = if want_compress {
        req_headers
            .get(header::ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .and_then(crate::compression::pick_encoding)
    } else {
        None
    };

    // ETag is base + suffix when compression will be applied.
    let etag = match chosen_encoding {
        Some(enc) => format!(
            "{}{}\"",
            &base_etag[..base_etag.len() - 1],
            enc.etag_suffix(),
        ),
        None => base_etag.clone(),
    };

    // ETag / If-None-Match check (takes precedence over If-Modified-Since)
    if let Some(inm) = req_headers.get(header::IF_NONE_MATCH) {
        if inm.to_str().unwrap_or("") == etag {
            return Response::builder().status(304).body(Body::empty()).unwrap();
        }
        // INM present but did not match — fall through to serve normally.
        // Per RFC 7232 §6, do NOT consult If-Modified-Since when INM is present.
    } else if let Some(ims) = req_headers.get(header::IF_MODIFIED_SINCE) {
        if let Some(client_time) = parse_imf_fixdate(ims.to_str().unwrap_or("")) {
            // Compare at second precision: HTTP dates are second-precision and the
            // Last-Modified header we sent was truncated by http_date(). Comparing
            // raw nanosecond-precision SystemTimes would mis-fire same-second cases.
            let client_secs = client_time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            if modified_secs <= client_secs {
                return Response::builder().status(304).body(Body::empty()).unwrap();
            }
        }
    }

    // Range request — uses the un-suffixed ETag, skips compression by design.
    if let Some(range_header) = req_headers.get(header::RANGE) {
        let range_str = range_header.to_str().unwrap_or("");
        if let Some((start, end)) = parse_range(range_str, file_size) {
            let length = end - start + 1;
            let data = read_file_range(path, start, length);
            return Response::builder()
                .status(206)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CONTENT_LENGTH, length)
                .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, file_size))
                .header(header::ETAG, &base_etag)
                .header(header::LAST_MODIFIED, &last_modified)
                .body(Body::from(data))
                .unwrap();
        }
    }

    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            log::error!("[static] read error {:?}: {}", path, e);
            return error_response(500, "Internal Server Error");
        }
    };

    let (body_bytes, encoding_header): (Vec<u8>, Option<&'static str>) = match chosen_encoding {
        Some(enc) => (crate::compression::compress(enc, &data), Some(enc.header_value())),
        None => (data, None),
    };

    let mut builder = Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, &content_type)
        .header(header::CONTENT_LENGTH, body_bytes.len())
        .header(header::ETAG, &etag)
        .header(header::LAST_MODIFIED, &last_modified);

    if crate::compression::is_compressible(&content_type) {
        builder = builder.header(header::VARY, "Accept-Encoding");
    }
    if let Some(enc_name) = encoding_header {
        builder = builder.header(header::CONTENT_ENCODING, enc_name);
    }

    builder.body(Body::from(body_bytes)).unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn run_cgi_handler(
    state: &AppState,
    script_path: &Path,
    ext: &str,
    req_path: &str,
    query: &str,
    method: &str,
    req_headers: &HeaderMap,
    stdin_body: Vec<u8>,
    peer_ip: IpAddr,
) -> Response<Body> {
    let server_name = "localhost";
    let peer_ip_str = peer_ip.to_string();
    let remote_addr: &str = &peer_ip_str;
    let content_type = req_headers.get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let content_length = stdin_body.len();

    let mut http_headers: HashMap<String, String> = HashMap::new();
    for (k, v) in req_headers.iter() {
        if let Ok(val) = v.to_str() {
            http_headers.insert(k.to_string(), val.to_string());
        }
    }

    let port_str = state.config.port.to_string();
    let content_length_str = content_length.to_string();

    let env_vars = build_cgi_env(
        method,
        query,
        script_path.to_str().unwrap_or(""),
        req_path,
        server_name,
        &port_str,
        remote_addr,
        &content_type,
        &content_length_str,
        &http_headers,
    );

    let cgi_out = run_cgi(script_path, ext, env_vars, stdin_body, state.config.cgi_timeout).await;

    let mut builder = Response::builder().status(cgi_out.status);
    for (k, v) in &cgi_out.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    builder.body(Body::from(cgi_out.body)).unwrap()
}

fn check_basic_auth(headers: &HeaderMap, user_file: &str) -> Option<String> {
    let auth_header = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = auth_header.strip_prefix("Basic ")?;
    use base64::{Engine, engine::general_purpose::STANDARD};
    let decoded = STANDARD.decode(encoded).ok()?;
    let credentials = String::from_utf8_lossy(&decoded);
    let (username, password) = credentials.split_once(':')?;
    let entries = match parse_htpasswd_file(user_file) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("[auth] cannot read htpasswd file: {}", e);
            return None;
        }
    };
    if check_credentials(username, password, &entries) {
        Some(username.to_string())
    } else {
        None
    }
}

fn auth_challenge_response(realm: &str) -> Response<Body> {
    Response::builder()
        .status(401)
        .header(header::WWW_AUTHENTICATE, format!("Basic realm=\"{}\"", realm))
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("401 Unauthorized"))
        .unwrap()
}

fn error_response(status: u16, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(format!("{} {}", status, message)))
        .unwrap()
}

/// Join root + rel_path safely, preventing traversal outside root.
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let joined = root.join(rel);
    let mut components = Vec::new();
    for comp in joined.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => { components.pop(); }
            Component::CurDir => {}
            c => components.push(c),
        }
    }
    let normalized: PathBuf = components.iter().collect();
    if normalized.starts_with(root) {
        Some(normalized)
    } else {
        None
    }
}

fn read_file_range(path: &Path, start: u64, length: u64) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut buf = vec![0u8; length as usize];
    let n = file.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use base64::{Engine, engine::general_purpose::STANDARD};

    #[tokio::test]
    async fn auth_scope_required_only_on_php() {
        use axum::body::Body as AxumBody;
        use axum::http::Request as AxumRequest;
        use base64::Engine as _;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let pw_path = dir.path().join(".htpasswd");
        std::fs::write(&pw_path, "alice:{SHA}5en6G6MezRroT3XKqkdPOmY/BfQ=\n").unwrap();
        let ht = format!(
            "<FilesMatch \"\\.php$\">\nAuthType Basic\nAuthName \"Test\"\nAuthUserFile {}\nRequire valid-user\n</FilesMatch>\n",
            pw_path.display()
        );
        std::fs::write(dir.path().join(".htaccess"), ht).unwrap();
        std::fs::write(dir.path().join("public.html"), "ok").unwrap();
        std::fs::write(
            dir.path().join("private.php"),
            "Content-Type: text/plain\n\nok",
        )
        .unwrap();

        // --no-cgi php so .php is served as static; otherwise the test
        // would depend on having a php interpreter installed.
        use clap::Parser as _;
        let config = Arc::new(crate::cli::CliConfig::parse_from(["webrunner", "--no-cgi", "php"]));
        let state = AppState {
            root: dir.path().to_path_buf(),
            config: config.clone(),
            access_log: None,
        };

        async fn build_req(path: &str, creds: Option<&str>) -> AxumRequest<AxumBody> {
            let mut builder = AxumRequest::builder()
                .method(axum::http::Method::GET)
                .uri(path);
            if let Some(c) = creds {
                let enc = base64::engine::general_purpose::STANDARD.encode(c.as_bytes());
                builder = builder.header(
                    axum::http::header::AUTHORIZATION,
                    format!("Basic {}", enc),
                );
            }
            builder.body(AxumBody::empty()).unwrap()
        }

        let peer: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();

        // 1. /public.html: 200 without creds.
        let r = handle_request(
            axum::extract::State(state.clone()),
            axum::extract::ConnectInfo(peer),
            build_req("/public.html", None).await,
        )
        .await;
        assert_eq!(r.status().as_u16(), 200, "unscoped path should be public");

        // 2. /private.php: 401 without creds.
        let r = handle_request(
            axum::extract::State(state.clone()),
            axum::extract::ConnectInfo(peer),
            build_req("/private.php", None).await,
        )
        .await;
        assert_eq!(r.status().as_u16(), 401, "scoped .php path should require auth");

        // 3. /private.php: 200 with valid creds.
        let r = handle_request(
            axum::extract::State(state.clone()),
            axum::extract::ConnectInfo(peer),
            build_req("/private.php", Some("alice:secret")).await,
        )
        .await;
        assert_eq!(r.status().as_u16(), 200, "scoped .php with valid creds");
    }

    /// Build a `HeaderMap` containing a `Basic` Authorization header
    /// for `<user>:<pass>`. Test helper.
    fn basic_auth_headers(user: &str, pass: &str) -> HeaderMap {
        let raw = format!("{}:{}", user, pass);
        let encoded = STANDARD.encode(raw.as_bytes());
        let value = format!("Basic {}", encoded);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&value).unwrap(),
        );
        headers
    }

    /// Write `<user>:{SHA}<sha1-of-pass>` to a tempfile and return the
    /// path string. `{SHA}` (RFC 2307 sha1) is the fastest of the three
    /// supported hash formats; bcrypt would slow the test suite.
    fn write_htpasswd(dir: &std::path::Path, user: &str, sha1_hash: &str) -> String {
        let path = dir.join(".htpasswd");
        std::fs::write(&path, format!("{}:{}\n", user, sha1_hash)).unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn check_basic_auth_returns_username_on_success() {
        let dir = tempfile::tempdir().unwrap();
        // {SHA} hash of "secret"; reused from auth::tests::test_verify_sha1.
        let user_file = write_htpasswd(
            dir.path(),
            "alice",
            "{SHA}5en6G6MezRroT3XKqkdPOmY/BfQ=",
        );
        let headers = basic_auth_headers("alice", "secret");

        let result = check_basic_auth(&headers, &user_file);
        assert_eq!(result.as_deref(), Some("alice"));
    }

    #[test]
    fn check_basic_auth_returns_none_on_bad_password() {
        let dir = tempfile::tempdir().unwrap();
        let user_file = write_htpasswd(
            dir.path(),
            "alice",
            "{SHA}5en6G6MezRroT3XKqkdPOmY/BfQ=",
        );
        let headers = basic_auth_headers("alice", "wrong");

        let result = check_basic_auth(&headers, &user_file);
        assert!(result.is_none());
    }
}
