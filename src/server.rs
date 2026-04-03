use axum::{Router, routing::any};
use axum_server::Handle;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::handler::{AppState, handle_request};
use crate::cli::CliConfig;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .fallback(any(handle_request))
        .with_state(state)
}

pub async fn run(config: CliConfig, root: std::path::PathBuf) -> Result<(), String> {
    let state = AppState {
        root: root.clone(),
        config: Arc::new(config.clone()),
    };

    let app = build_router(state);

    let http_addr: SocketAddr = format!("0.0.0.0:{}", config.port)
        .parse()
        .map_err(|e| format!("Invalid port: {}", e))?;

    let handle = Handle::new();
    let shutdown_handle = handle.clone();

    // Graceful shutdown on Ctrl+C or SIGTERM
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

        let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
            &cert_paths.cert,
            &cert_paths.key,
        ).await.map_err(|e| format!("TLS config error: {}", e))?;

        // Print banner
        println!("webrunner v{}", env!("CARGO_PKG_VERSION"));
        println!("Serving {}", root.display());
        println!("http://localhost:{}", config.port);
        println!("https://localhost:{}  (self-signed, fingerprint: {})", config.https_port, fingerprint);
        println!("Press Ctrl+C to stop.");

        // HTTP listener
        let http_app = app.clone();
        let http_handle = handle.clone();
        tokio::spawn(async move {
            axum_server::bind(http_addr)
                .handle(http_handle)
                .serve(http_app.into_make_service())
                .await
                .unwrap_or_else(|e| eprintln!("HTTP server error: {}", e));
        });

        // HTTPS listener
        axum_server::bind_rustls(https_addr, tls_config)
            .handle(handle)
            .serve(app.into_make_service())
            .await
            .map_err(|e| format!("HTTPS server error: {}", e))?;
    } else {
        // HTTP only
        println!("webrunner v{}", env!("CARGO_PKG_VERSION"));
        println!("Serving {}", root.display());
        println!("http://localhost:{}", config.port);
        println!("Press Ctrl+C to stop.");

        axum_server::bind(http_addr)
            .handle(handle)
            .serve(app.into_make_service())
            .await
            .map_err(|e| format!("HTTP server error: {}", e))?;
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
