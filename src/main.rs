mod access_control;
mod access_log;
mod auth;
mod cgi;
mod cli;
mod compression;
mod examples;
mod expires;
mod handler;
mod header_directive;
mod htaccess;
mod hsts;
mod logging;
mod mime;
mod rewrite;
mod server;
mod static_files;
mod tls;

use clap::Parser;
use std::env;

#[tokio::main]
async fn main() {
    let mut config = cli::CliConfig::parse();

    if config.examples {
        examples::print();
        return;
    }

    // Validate CLI args
    if let Err(e) = config.validate() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    logging::init(config.log_level.to_filter());

    // Warn about missing interpreters
    check_interpreter("perl",  &["pl"]);
    check_interpreter("php",   &["php"]);
    check_interpreter("bun",   &["ts", "js"]);

    let root = match config.resolve_root() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = server::run(config, root).await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// Check if an interpreter is available in PATH. Warns if not found.
fn check_interpreter(binary: &str, extensions: &[&str]) {
    if which(binary).is_none() {
        log::warn!("Warning: '{}' not found in PATH — .{} scripts will return 500",
            binary, extensions.join(", ."));
    }
}

fn which(binary: &str) -> Option<std::path::PathBuf> {
    let path_var = env::var("PATH").unwrap_or_default();
    for dir in path_var.split(':') {
        let candidate = std::path::Path::new(dir).join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
