mod auth;
mod cgi;
mod cli;
mod handler;
mod htaccess;
mod mime;
mod rewrite;
mod server;
mod static_files;
mod tls;

use clap::Parser;
use std::env;

#[tokio::main]
async fn main() {
    let config = cli::CliConfig::parse();

    // Validate CLI args
    if let Err(e) = config.validate() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    // Warn about missing interpreters
    check_interpreter("perl",  &["pl"]);
    check_interpreter("php",   &["php"]);
    check_interpreter("bun",   &["ts", "js"]);

    let root = env::current_dir().expect("Cannot determine current directory");

    if let Err(e) = server::run(config, root).await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// Check if an interpreter is available in PATH. Warns if not found.
fn check_interpreter(binary: &str, extensions: &[&str]) {
    if which(binary).is_none() {
        eprintln!("Warning: '{}' not found in PATH — .{} scripts will return 500",
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
