use axum::{Router, routing::any};
use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::path::PathBuf;
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

    let http_addr: SocketAddr = format!("0.0.0.0:{}", config.port)
        .parse()
        .map_err(|e| format!("Invalid port: {}", e))?;
    let http_listener = TcpListener::bind(http_addr)
        .map_err(|e| format!("failed to bind {}: {}", http_addr, e))?;

    let handle = Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        eprintln!("\nShutting down gracefully...");
        shutdown_handle.graceful_shutdown(Some(Duration::from_secs(5)));
    });

    if config.https_active() {
        let cert_paths = crate::tls::ensure_cert(
            config.cert.as_deref(),
            config.key.as_deref(),
        ).map_err(|e| e.to_string())?;
        let fingerprint = crate::tls::cert_fingerprint(&cert_paths.cert);
        let https_addr: SocketAddr = format!("0.0.0.0:{}", config.https_port)
            .parse()
            .map_err(|e| format!("Invalid HTTPS port: {}", e))?;
        let https_listener = TcpListener::bind(https_addr)
            .map_err(|e| format!("failed to bind {}: {}", https_addr, e))?;
        let tls_config = RustlsConfig::from_pem_file(
            &cert_paths.cert,
            &cert_paths.key,
        ).await.map_err(|e| format!("TLS config error: {}", e))?;

        println!("webrunner v{}", env!("CARGO_PKG_VERSION"));
        println!("Serving {}", root.display());
        println!("http://localhost:{}", config.port);
        println!("https://localhost:{}  (self-signed, fingerprint: {})", config.https_port, fingerprint);
        println!("Press Ctrl+C to stop.");

        bind_and_serve(app, vec![http_listener], vec![(https_listener, tls_config)], handle).await
    } else {
        println!("webrunner v{}", env!("CARGO_PKG_VERSION"));
        println!("Serving {}", root.display());
        println!("http://localhost:{}", config.port);
        println!("Press Ctrl+C to stop.");

        bind_and_serve(app, vec![http_listener], vec![], handle).await
    }
}

/// Serve HTTP and (optionally) HTTPS on pre-bound TCP listeners.
///
/// `http_listeners` and `https_listeners` are eagerly-bound listeners
/// produced by the caller; this function consumes them and spawns one
/// axum_server task per listener. All tasks share the supplied
/// `Handle` for unified graceful shutdown.
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
                .serve(app.into_make_service())
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
                .serve(app.into_make_service())
                .await
        }));
    }

    for task in tasks {
        match task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(format!("listener error: {}", e)),
            Err(e) => return Err(format!("listener join error: {}", e)),
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
