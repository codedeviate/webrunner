//! Colorized `--examples` output. Style mirrors `codedeviate/recon`'s
//! examples module for cross-repo uniformity: yellow section headers,
//! bold descriptions, cyan commands, dimmed notes.

use colored::Colorize;

pub fn print() {
    let title = "webrunner — usage examples";
    println!("\n{}\n", title.bold());

    section("BASIC USAGE");

    example("Serve the current directory on http://localhost:8080", &[
        "webrunner",
    ]);
    example("Serve on a different port", &[
        "webrunner -p 3000",
    ]);
    example("Serve a specific directory (positional or --root)", &[
        "webrunner --root ./public",
        "webrunner ./public",
    ]);
    example("Disable directory listings (return 403 for directories)", &[
        "webrunner --no-index",
    ]);
    example("Restrict to loopback or a specific interface", &[
        "webrunner --bind 127.0.0.1,::1",
        "webrunner --bind 192.168.1.10",
    ]);

    section("HTTPS");

    example("HTTPS on :8443 with auto-generated self-signed cert", &[
        "webrunner --https",
    ]);
    note("Certificate is cached in ~/.config/webrunner/ and reused across runs.");

    example("HTTPS on a custom port", &[
        "webrunner --https --https-port 443",
    ]);
    example("Bring your own cert/key", &[
        "webrunner --cert cert.pem --key key.pem",
    ]);
    example("HTTP and HTTPS simultaneously", &[
        "webrunner -p 8080 --https",
    ]);
    example("HSTS header on HTTPS responses", &[
        "webrunner --https --hsts 60",
        "webrunner --https --hsts 31536000 --hsts-include-subdomains --hsts-preload",
    ]);
    note("Start with a short max-age when experimenting — browsers cache HSTS aggressively.");

    section("CGI SCRIPTS");

    example("Always-on CGI for Perl (.pl) and PHP (.php) — no flags needed", &[
        "webrunner",
    ]);
    example("Opt in to JS/TS execution via bun", &[
        "webrunner --cgi js,ts",
    ]);
    example("Serve .pl / .php as static text instead", &[
        "webrunner --no-cgi pl,php",
    ]);
    example("Adjust the CGI script timeout (default 30s; 0 disables)", &[
        "webrunner --cgi-timeout 60",
        "webrunner --cgi-timeout 0",
    ]);
    note("Scripts get standard CGI/1.1 env (REQUEST_METHOD, QUERY_STRING, PATH_INFO, SCRIPT_NAME, HTTP_*, …). POST body on stdin. Output is headers + blank line + body. Use 'Status: 404 Not Found' to set a non-200 response.");

    section(".HTACCESS — COMMON RECIPES");

    example("Front-controller pattern with index.php", &[
        "DirectoryIndex index.php",
        "RewriteEngine On",
        "RewriteCond %{REQUEST_FILENAME} !-f",
        "RewriteCond %{REQUEST_FILENAME} !-d",
        "RewriteRule ^(.*)$ index.php [L]",
    ]);
    example("Permanent and temporary redirects", &[
        "Redirect 301 /old-path /new-path",
        "Redirect /also-old /also-new",
        "RedirectMatch 204 /favicon.ico$",
        "RedirectMatch ^/old/(.*)$ /new/$1",
    ]);
    example("Basic auth (htpasswd via `htpasswd -B .htpasswd alice`)", &[
        "AuthType Basic",
        "AuthName \"Restricted Area\"",
        "AuthUserFile /path/to/.htpasswd",
        "Require valid-user",
    ]);
    note("Supported hash formats: bcrypt ($2y$…), SHA-1 ({SHA}…), Apache MD5 ($apr1$…). Use `Require user alice bob` to limit by username.");

    example("IP access control (Apache 2.4 syntax)", &[
        "Require ip 127.0.0.1/32 ::1/128",
        "Require not ip 10.0.0.0/8",
    ]);
    example("IP access control (Apache 2.2 legacy)", &[
        "Order allow,deny",
        "Allow from 192.168.1.0/24",
    ]);
    example("Per-file scoping with <FilesMatch>", &[
        "<FilesMatch \"\\.php$\">",
        "    Header set Cache-Control \"no-cache\"",
        "    AuthType Basic",
        "    AuthName \"PHP Only\"",
        "    AuthUserFile /path/to/.htpasswd",
        "    Require valid-user",
        "</FilesMatch>",
    ]);
    example("Response header rules (mod_headers subset)", &[
        "Header always set X-Frame-Options \"DENY\"",
        "Header set Referrer-Policy \"no-referrer\"",
        "Header unset X-Powered-By",
        "Header echo \"X-Forwarded-.*\"",
        "Header always set X-Request-Duration \"%D us\"",
    ]);
    note("Actions: set, setifempty, add, append, merge, unset, echo, edit, edit*. Condition: always|onsuccess (default onsuccess). Placeholders: %t, %D, %l, %s, %H, %m, %U.");

    example("Cache-Control via mod_expires", &[
        "ExpiresActive on",
        "ExpiresDefault \"access plus 1 month\"",
        "ExpiresByType text/css \"access plus 1 year\"",
        "ExpiresByType application/javascript \"access plus 1 week\"",
    ]);
    example("MIME-type overrides and default charset", &[
        "AddType application/wasm wasm",
        "AddDefaultCharset utf-8",
    ]);
    example("RewriteRule flags: F (403), G (410), R (redirect), NC, L, QSA", &[
        "RewriteRule ^secret/.*$ - [F]",
        "RewriteRule ^gone-page$ - [G]",
        "RewriteRule ^(.*)$ /index.php [L,QSA]",
        "RewriteRule ^go$ https://example.com [R=302]",
    ]);

    section("LOGGING");

    example("Combined Log Format access log to stdout or a file", &[
        "webrunner --log -",
        "webrunner --log /var/log/webrunner.access.log",
    ]);
    example("Adjust verbosity level (off | warn | info | debug)", &[
        "webrunner --log-level debug",
        "webrunner --log-level warn",
    ]);
    note("`--log-level debug` surfaces recognized-but-unsupported .htaccess directives — use it when diagnosing why a directive doesn't seem to apply.");

    section("COMPRESSION");

    example("Brotli / gzip compression (on by default for text-shaped responses)", &[
        "webrunner",
        "webrunner --compression off",
    ]);
    note("Applies to text/*, application/json, application/javascript, image/svg+xml, application/wasm. Responses < 256 bytes and Range requests are served uncompressed.");

    println!();
}

fn section(title: &str) {
    println!("  {}", title.yellow().bold());
    println!();
}

fn example(desc: &str, commands: &[&str]) {
    println!("    {}", desc.bold());
    for cmd in commands {
        println!("      {}", cmd.cyan());
    }
    println!();
}

fn note(text: &str) {
    println!("    {} {}", "note:".dimmed().bold(), text.dimmed());
    println!();
}
