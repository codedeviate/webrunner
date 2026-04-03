// Request pipeline orchestrator

use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, header, HeaderMap},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use crate::cli::CliConfig;
use crate::htaccess::parse_htaccess_for_path;
use crate::auth::{parse_htpasswd_file, check_credentials};
use crate::rewrite::{apply_rewrites, RewriteResult};
use crate::cgi::{is_cgi_ext, build_cgi_env, run_cgi};
use crate::static_files::{resolve_index, build_etag, http_date, format_directory_listing, parse_range};
use crate::mime::mime_for_ext_owned;

#[derive(Clone)]
pub struct AppState {
    pub root: PathBuf,
    pub config: Arc<CliConfig>,
}

pub async fn handle_request(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Response<Body> {
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
            eprintln!("[.htaccess] error: {}", e);
            crate::htaccess::HtaccessConfig::default()
        }
    };

    // Auth check
    if htaccess.auth_required {
        if let Some(user_file) = &htaccess.auth_user_file {
            let authorized = check_basic_auth(&req_headers, user_file);
            if !authorized {
                let realm = htaccess.auth_name.as_deref().unwrap_or("Restricted");
                return auth_challenge_response(realm);
            }
        } else {
            // auth_required but no AuthUserFile configured — deny access
            eprintln!("[auth] auth_required but no AuthUserFile configured, denying access");
            return error_response(403, "Forbidden");
        }
    }

    // Redirect/Rewrite
    match apply_rewrites(path_str, query, &htaccess) {
        RewriteResult::Redirect { status, location } => {
            return Response::builder()
                .status(status)
                .header(header::LOCATION, location)
                .body(Body::empty())
                .unwrap();
        }
        RewriteResult::Rewrite(new_path) => {
            let new_rel = new_path.trim_start_matches('/');
            let new_fs = match safe_join(&state.root, new_rel) {
                Some(p) => p,
                None => return error_response(400, "Bad Request"),
            };
            return serve_path(&state, &new_fs, &new_path, query, &method, &req_headers, &htaccess, body_bytes).await;
        }
        RewriteResult::None => {}
    }

    serve_path(&state, &fs_path, path_str, query, &method, &req_headers, &htaccess, body_bytes).await
}

async fn serve_path(
    state: &AppState,
    fs_path: &Path,
    req_path: &str,
    query: &str,
    method: &str,
    req_headers: &HeaderMap,
    htaccess: &crate::htaccess::HtaccessConfig,
    body_bytes: Vec<u8>,
) -> Response<Body> {
    // Directory handling
    if fs_path.is_dir() {
        if let Some(index_path) = resolve_index(fs_path, &htaccess.directory_index) {
            let ext = index_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if is_cgi_ext(ext) {
                return run_cgi_handler(state, &index_path, ext, req_path, query, method, req_headers, body_bytes).await;
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
                    eprintln!("[static] directory listing error: {}", e);
                    return error_response(500, "Internal Server Error");
                }
            }
        } else {
            return error_response(403, "Forbidden");
        }
    }

    // CGI script
    let ext = fs_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if is_cgi_ext(ext) {
        return run_cgi_handler(state, fs_path, ext, req_path, query, method, req_headers, body_bytes).await;
    }

    // Static file
    if !fs_path.exists() {
        return error_response(404, "Not Found");
    }

    serve_static_file(state, fs_path, req_path, req_headers, htaccess).await
}

async fn serve_static_file(
    _state: &AppState,
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
    let etag = build_etag(file_size, modified_secs);
    let last_modified = http_date(modified);

    // ETag / If-None-Match check
    if let Some(inm) = req_headers.get(header::IF_NONE_MATCH) {
        if inm.to_str().unwrap_or("") == etag {
            return Response::builder().status(304).body(Body::empty()).unwrap();
        }
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let content_type = mime_for_ext_owned(ext, &htaccess.add_types);
    let content_type = if let Some(charset) = &htaccess.add_default_charset {
        if !content_type.contains("charset") {
            format!("{}; charset={}", content_type, charset)
        } else {
            content_type
        }
    } else {
        content_type
    };

    // Range request
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
                .header(header::ETAG, &etag)
                .header(header::LAST_MODIFIED, &last_modified)
                .body(Body::from(data))
                .unwrap();
        }
    }

    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[static] read error {:?}: {}", path, e);
            return error_response(500, "Internal Server Error");
        }
    };

    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, data.len())
        .header(header::ETAG, etag)
        .header(header::LAST_MODIFIED, last_modified)
        .body(Body::from(data))
        .unwrap()
}

async fn run_cgi_handler(
    state: &AppState,
    script_path: &Path,
    ext: &str,
    req_path: &str,
    query: &str,
    method: &str,
    req_headers: &HeaderMap,
    stdin_body: Vec<u8>,
) -> Response<Body> {
    let server_name = "localhost";
    let remote_addr = "127.0.0.1";
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
        req_path,
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

    let cgi_out = run_cgi(script_path, ext, env_vars, stdin_body).await;

    let mut builder = Response::builder().status(cgi_out.status);
    for (k, v) in &cgi_out.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    builder.body(Body::from(cgi_out.body)).unwrap()
}

fn check_basic_auth(headers: &HeaderMap, user_file: &str) -> bool {
    let auth_header = match headers.get(header::AUTHORIZATION) {
        Some(v) => v.to_str().unwrap_or(""),
        None => return false,
    };
    let encoded = match auth_header.strip_prefix("Basic ") {
        Some(e) => e,
        None => return false,
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    let decoded = match STANDARD.decode(encoded) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let credentials = String::from_utf8_lossy(&decoded);
    let (username, password) = match credentials.split_once(':') {
        Some(p) => p,
        None => return false,
    };
    let entries = match parse_htpasswd_file(user_file) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[auth] cannot read htpasswd file: {}", e);
            return false;
        }
    };
    check_credentials(username, password, &entries)
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
