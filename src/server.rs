use axum::{Router, routing::any};
use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use std::net::{IpAddr, SocketAddr};
use std::net::TcpListener;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::handler::{AppState, handle_request};
use crate::cli::CliConfig;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .fallback(any(handle_request))
        .with_state(state)
}

pub async fn run(config: CliConfig, root: PathBuf) -> Result<(), String> {
    let state = AppState {
        root: root.clone(),
        config: Arc::new(config.clone()),
    };
    let app = build_router(state);

    let bind_ips: Vec<IpAddr> = config.bind.iter()
        .map(|s| IpAddr::from_str(s).expect("bind already validated"))
        .collect();

    let (http_listeners, http_addrs) = bind_listeners_for_port(
        &bind_ips,
        config.port,
        config.bind_explicit,
        "HTTP",
    )?;

    let handle = Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        log::info!("\nShutting down gracefully...");
        shutdown_handle.graceful_shutdown(Some(Duration::from_secs(5)));
    });

    let mut https_pairs: Vec<(TcpListener, RustlsConfig)> = Vec::new();
    let mut https_addrs: Vec<SocketAddr> = Vec::new();
    let fingerprint = if config.https_active() {
        let cert_paths = crate::tls::ensure_cert(
            config.cert.as_deref(),
            config.key.as_deref(),
        ).map_err(|e| e.to_string())?;
        let fp = crate::tls::cert_fingerprint(&cert_paths.cert);
        let tls_config = RustlsConfig::from_pem_file(
            &cert_paths.cert,
            &cert_paths.key,
        ).await.map_err(|e| format!("TLS config error: {}", e))?;
        let (listeners, addrs) = bind_listeners_for_port(
            &bind_ips,
            config.https_port,
            config.bind_explicit,
            "HTTPS",
        )?;
        for l in listeners {
            https_pairs.push((l, tls_config.clone()));
        }
        https_addrs = addrs;
        Some(fp)
    } else {
        None
    };

    log::info!("webrunner v{}", env!("CARGO_PKG_VERSION"));
    log::info!("Serving {}", root.display());
    log::info!("http://localhost:{}", config.port);
    if let Some(ref fp) = fingerprint {
        log::info!("https://localhost:{}  (self-signed, fingerprint: {})", config.https_port, fp);
    }
    let mut bound_urls: Vec<String> = Vec::new();
    for a in &http_addrs {
        bound_urls.push(format_url("http", a));
    }
    for a in &https_addrs {
        bound_urls.push(format_url("https", a));
    }
    log::info!("bound: {}", bound_urls.join(", "));
    log::info!("Press Ctrl+C to stop.");

    bind_and_serve(app, http_listeners, https_pairs, handle).await
}

fn bind_listeners_for_port(
    ips: &[IpAddr],
    port: u16,
    explicit: bool,
    label: &str,
) -> Result<(Vec<TcpListener>, Vec<SocketAddr>), String> {
    let mut listeners: Vec<TcpListener> = Vec::new();
    let mut addrs: Vec<SocketAddr> = Vec::new();
    let mut v4_ok = false;
    for ip in ips {
        let addr = SocketAddr::new(*ip, port);
        match TcpListener::bind(addr) {
            Ok(l) => {
                if ip.is_ipv4() { v4_ok = true; }
                listeners.push(l);
                addrs.push(addr);
            }
            Err(e) => {
                if explicit {
                    return Err(format!("failed to bind {} {}: {}", label, addr, e));
                }
                log::warn!("warn: skipping {} {} ({} unavailable: {})", label, addr, ip, e);
            }
        }
    }
    if listeners.is_empty() || (!explicit && !v4_ok) {
        let tried: Vec<String> = ips.iter().map(|ip| ip.to_string()).collect();
        return Err(format!("{}: no bindable address (tried: {})", label, tried.join(", ")));
    }
    Ok((listeners, addrs))
}

fn format_url(scheme: &str, addr: &SocketAddr) -> String {
    match addr.ip() {
        IpAddr::V4(_) => format!("{}://{}:{}", scheme, addr.ip(), addr.port()),
        IpAddr::V6(_) => format!("{}://[{}]:{}", scheme, addr.ip(), addr.port()),
    }
}

/// Serve HTTP and (optionally) HTTPS on pre-bound TCP listeners.
///
/// `http_listeners` and `https_listeners` are eagerly-bound listeners
/// produced by the caller; this function consumes them and spawns one
/// axum_server task per listener. All tasks share the supplied
/// `Handle` for unified graceful shutdown.
///
/// On the first listener failure, the shared `Handle` is asked to
/// gracefully shut down all sibling listeners before this function
/// returns `Err`.
pub async fn bind_and_serve(
    app: Router,
    http_listeners: Vec<TcpListener>,
    https_listeners: Vec<(TcpListener, RustlsConfig)>,
    handle: Handle,
) -> Result<(), String> {
    let mut tasks: Vec<tokio::task::JoinHandle<Result<(), std::io::Error>>> = Vec::new();

    for listener in http_listeners {
        listener.set_nonblocking(true).map_err(|e| format!("set_nonblocking: {}", e))?;
        let app = app.clone();
        let handle = handle.clone();
        tasks.push(tokio::spawn(async move {
            axum_server::from_tcp(listener)
                .handle(handle)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await
        }));
    }

    for (listener, tls_config) in https_listeners {
        listener.set_nonblocking(true).map_err(|e| format!("set_nonblocking: {}", e))?;
        let app = app.clone();
        let handle = handle.clone();
        tasks.push(tokio::spawn(async move {
            axum_server::tls_rustls::from_tcp_rustls(listener, tls_config)
                .handle(handle)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await
        }));
    }

    for task in tasks {
        match task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                handle.graceful_shutdown(Some(Duration::from_secs(5)));
                return Err(format!("listener error: {}", e));
            }
            Err(e) => {
                handle.graceful_shutdown(Some(Duration::from_secs(5)));
                return Err(format!("listener join error: {}", e));
            }
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    };

    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliConfig;
    use clap::Parser;
    use std::net::{IpAddr, SocketAddr, TcpListener as StdTcpListener};
    use std::str::FromStr;
    use tempfile::tempdir;

    fn build_app_for_test(root: PathBuf) -> Router {
        let mut config = CliConfig::parse_from(["webrunner"]);
        config.validate().unwrap();
        let state = AppState {
            root,
            config: Arc::new(config),
        };
        build_router(state)
    }

    async fn http_get_status(addr: SocketAddr) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // Retry connect for up to ~1s so the test isn't tied to acceptor wake-up timing on slow CI.
        let mut stream = None;
        for _ in 0..20 {
            if let Ok(s) = tokio::net::TcpStream::connect(addr).await {
                stream = Some(s);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut stream = stream.expect("connect (after retry)");
        stream.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").await.unwrap();
        let mut buf = Vec::with_capacity(256);
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            stream.read_to_end(&mut buf),
        ).await;
        String::from_utf8_lossy(&buf[..buf.len().min(64)]).to_string()
    }

    #[tokio::test]
    async fn bind_and_serve_accepts_v4_and_v6() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), b"<h1>ok</h1>").unwrap();

        let v4_listener = StdTcpListener::bind(SocketAddr::new(IpAddr::from_str("127.0.0.1").unwrap(), 0)).unwrap();
        let v6_listener = StdTcpListener::bind(SocketAddr::new(IpAddr::from_str("::1").unwrap(), 0)).unwrap();
        let v4_addr = v4_listener.local_addr().unwrap();
        let v6_addr = v6_listener.local_addr().unwrap();

        let app = build_app_for_test(dir.path().to_path_buf());
        let handle = Handle::new();
        let server_handle = handle.clone();
        let server = tokio::spawn(async move {
            bind_and_serve(app, vec![v4_listener, v6_listener], vec![], server_handle).await
        });

        let v4_response = http_get_status(v4_addr).await;
        let v6_response = http_get_status(v6_addr).await;
        assert!(v4_response.starts_with("HTTP/"), "v4 didn't respond: {:?}", v4_response);
        assert!(v6_response.starts_with("HTTP/"), "v6 didn't respond: {:?}", v6_response);

        handle.graceful_shutdown(Some(Duration::from_millis(100)));
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
    }

    #[tokio::test]
    async fn run_returns_error_on_explicit_bind_conflict() {
        // Occupy a port deterministically.
        let blocker = StdTcpListener::bind(SocketAddr::new(IpAddr::from_str("127.0.0.1").unwrap(), 0)).unwrap();
        let port = blocker.local_addr().unwrap().port();

        let mut config = CliConfig::parse_from([
            "webrunner",
            "--port", &port.to_string(),
            "--bind", "127.0.0.1",
        ]);
        config.validate().unwrap();

        let dir = tempdir().unwrap();
        let result = run(config, dir.path().to_path_buf()).await;
        assert!(result.is_err(), "expected bind conflict error, got Ok");
        let msg = result.unwrap_err();
        assert!(msg.contains("127.0.0.1") && msg.contains(&port.to_string()),
            "error didn't mention conflicting socket: {}", msg);
    }
}
