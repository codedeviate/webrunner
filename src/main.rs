mod access_log;
mod auth;
mod cgi;
mod cli;
mod compression;
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
        print_examples();
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

fn print_examples() {
    println!(r#"webrunner — usage examples

BASIC USAGE
  webrunner                        Serve current directory on http://localhost:8080
  webrunner -p 3000                Serve on port 3000
  webrunner --no-index             Disable directory listing (return 403 for directories)

HTTPS
  webrunner --https                HTTPS on :8443, auto-generate self-signed cert in
                                   ~/.config/webrunner/ (reused on next run)
  webrunner --https --https-port 443
                                   HTTPS on port 443
  webrunner --cert cert.pem --key key.pem
                                   HTTPS using your own certificate (implies --https)
  webrunner -p 8080 --https        HTTP on :8080 and HTTPS on :8443 simultaneously

CGI SCRIPTS
  Place executable scripts in the served directory and request them by URL:

  index.pl                         Perl script (requires perl in PATH)
    #!/usr/bin/perl
    print "Content-Type: text/html\n\n";
    print "<h1>Hello from Perl</h1>\n";

  hello.php                        PHP script (requires php in PATH)
    <?php header("Content-Type: text/plain"); echo "Hello from PHP"; ?>

  api.ts                           TypeScript/JavaScript (requires bun in PATH)
    console.log("Content-Type: application/json\n");
    console.log(JSON.stringify({{ ok: true }}));

  Scripts receive standard CGI/1.1 environment variables:
    REQUEST_METHOD, QUERY_STRING, CONTENT_TYPE, CONTENT_LENGTH,
    PATH_INFO, SCRIPT_NAME, SERVER_NAME, SERVER_PORT, REMOTE_ADDR,
    HTTP_* (one var per request header)

  POST body is passed via stdin. Output must be headers + blank line + body.
  Use "Status: 404 Not Found" header to set a non-200 response code.

.HTACCESS
  Place a .htaccess file in any directory. Supported directives:

  # Directory index files (default: index.html index.htm)
  DirectoryIndex index.php index.html

  # Enable/disable directory listing
  Options -Indexes
  Options +Indexes

  # Custom error pages
  ErrorDocument 404 /errors/404.html
  ErrorDocument 500 /errors/500.html

  # MIME type overrides
  AddType application/wasm .wasm
  AddDefaultCharset UTF-8

  # Permanent and temporary redirects
  Redirect 301 /old-path /new-path
  Redirect /also-old /also-new      (302 by default)

  # URL rewriting
  RewriteEngine On
  RewriteRule ^api/(.*)$ handler.php?route=$1 [QSA,L]
  RewriteRule ^(.*)$ index.php [L]

  # RewriteCond before a RewriteRule
  RewriteCond %{{REQUEST_FILENAME}} !-f
  RewriteCond %{{REQUEST_FILENAME}} !-d
  RewriteRule ^(.*)$ index.php [L]

  # Basic authentication
  AuthType Basic
  AuthName "Restricted Area"
  AuthUserFile /path/to/.htpasswd
  Require valid-user

  .htpasswd format (generate with: htpasswd -B .htpasswd username):
    username:$2y$10$...   (bcrypt)
    username:{{SHA}}...    (SHA-1)
    username:$apr1$...     (Apache MD5)

  Deeper .htaccess files override parent directory settings.
  Unknown directives are skipped with a warning on stderr.
"#);
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
