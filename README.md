# webrunner

[![GitHub](https://img.shields.io/badge/github-codedeviate%2Fwebrunner-181717?logo=github)](https://github.com/codedeviate/webrunner)
[![Latest release](https://img.shields.io/badge/release-v0.8.0-blue)](https://github.com/codedeviate/webrunner/releases)
[![crates.io](https://img.shields.io/crates/v/webrunner?logo=rust&color=orange)](https://crates.io/crates/webrunner)
[![Homebrew](https://img.shields.io/badge/homebrew-codedeviate%2Fcli-FBB040?logo=homebrew)](https://github.com/codedeviate/homebrew-cli)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Rust edition 2021](https://img.shields.io/badge/rust-2021_edition_(MSRV_1.75)-dea584?logo=rust)](https://www.rust-lang.org)

A zero-config development web server in Rust, with CGI and `.htaccess` support.

Point it at a directory and you get static files, directory listings, classic CGI
scripts (Perl/PHP/TypeScript via `bun`), URL rewriting, basic auth, custom error
pages, and self-signed HTTPS — all driven by familiar Apache-style `.htaccess`
files.

Built for local development and quick prototyping. Not a production server.

## Features

- Static file serving with automatic MIME detection
- Directory listings (toggleable per-request and via `Options ±Indexes`)
- CGI/1.1 execution for `.pl` and `.php` (always on); `.ts` and `.js`
  opt-in via `--cgi`
- `.htaccess` support with per-directory overrides:
  - `DirectoryIndex`, `Options ±Indexes`
  - `ErrorDocument` for custom error pages
  - `AddType`, `AddDefaultCharset`
  - `Redirect` (301/302)
  - `RedirectMatch` (regex-based redirect with `$N` capture
    substitution; supports status-only forms like
    `RedirectMatch 204 /favicon.ico$`)
  - `ExpiresActive`, `ExpiresDefault`, `ExpiresByType` (mod_expires
    subset: emits `Cache-Control: max-age=N` and `Expires` headers
    based on response Content-Type; supports `access plus N <unit>`
    time-spec)
  - `Require valid-user` and `Require user <name>` for per-user
    authorization (basic auth)
  - `Allow from` / `Deny from` / `Order` (Apache 2.2 IP-based
    access control) and `Require ip <CIDR>` / `Require not ip
    <CIDR>` (Apache 2.4 syntax)
  - `RewriteEngine`, `RewriteRule`, `RewriteCond` (incl. flags
    `[QSA]`, `[L]`, `[R]`, `[NC]`, `[F]`, `[G]`), `RewriteBase`
  - `AuthType Basic` with `.htpasswd` (bcrypt, SHA-1, Apache MD5)
  - `Header set/add/unset/append/merge/setifempty/echo/edit/edit*`
    with `always|onsuccess` condition and basic placeholders
    (`%t`, `%D`, `%l`, `%s`, `%H`, `%m`, `%U`)
- HTTPS with auto-generated self-signed certificates (cached in
  `~/.config/webrunner/`) or your own cert/key
- Simultaneous HTTP + HTTPS listeners on IPv4 and IPv6 (default)
- Graceful shutdown on `Ctrl+C` / `SIGTERM`

## Install

### Homebrew (macOS / Linux)

```sh
brew install codedeviate/cli/webrunner
```

### From crates.io

```sh
cargo install webrunner
```

### From source

Requires Rust 1.75+ (2021 edition).

```sh
git clone https://github.com/codedeviate/webrunner.git
cd webrunner
cargo install --path .
```

Or build a release binary:

```sh
cargo build --release
./target/release/webrunner
```

Optional runtime dependencies (only needed if you serve CGI scripts of that
type): `perl`, `php`, `bun`. Missing interpreters trigger a startup warning, and
the corresponding scripts return `500`.

## Quick start

```sh
cd /path/to/your/site
webrunner
```

Then open <http://localhost:8080>.

## Usage

```
webrunner [OPTIONS]

  -p, --port <PORT>          HTTP port [default: 8080]
      --https                Enable HTTPS (auto-generates a self-signed cert)
      --https-port <PORT>    HTTPS port [default: 8443]
      --cert <PATH>          Path to TLS certificate (PEM); requires --key
      --key  <PATH>          Path to TLS private key (PEM); requires --cert
      --no-index             Disable directory listing (return 403 for dirs)
      --root <DIR>           Directory to serve (also accepted as a
                             positional argument). Default: cwd.
      --bind <ADDR>          Bind addresses (IPv4/IPv6 literals).
                             Repeatable / comma-separated.
                             Default: 0.0.0.0,::
      --cgi <EXT[,EXT...]>   Extra extensions to execute as CGI (js, ts).
                             pl and php are CGI by default.
      --no-cgi <EXT>         Disable always-on CGI for listed extensions
                             (pl, php). Repeatable / comma-separated.
                             Mutually exclusive with --cgi per extension.
      --cgi-timeout <SECS>   CGI script timeout. 0 disables.
                             Default: 30
      --log-level <LEVEL>    off | warn | info | debug. Default: info
      --log <PATH>           Access log file, or - for stdout.
                             Combined Log Format.
      --compression <ON|OFF> Enable brotli / gzip compression for
                             text-shaped static files. Default: on
      --hsts <SECS>          Send Strict-Transport-Security on HTTPS
                             with this max-age. Default: no header.
      --hsts-include-subdomains
                             Add includeSubDomains directive.
                             Requires --hsts.
      --hsts-preload         Add preload directive. Requires --hsts.
      --examples             Print rich usage examples and exit
  -h, --help                 Show help
  -V, --version              Show version
```

Run `webrunner --examples` for a complete cookbook covering CGI, `.htaccess`,
auth, rewrites, and HTTPS.

### Logging

webrunner prints to stderr (`error`, `warn`) and stdout (`info`,
`debug`). Default level is `info`. Use `--log-level warn` to suppress
the startup banner, or `--log-level off` to silence everything except
fatal startup errors.

**Access logs.** Pass `--log <PATH>` to write one Combined Log Format
line per request. `--log -` writes to stdout instead. Access logs are
separate from `--log-level`: setting `--log-level off` does not
silence them. The `%u` field is the authenticated username for
requests that passed `AuthType Basic` auth, and `-` otherwise.

**Compression.** webrunner serves text-shaped responses (`text/*`,
`application/json`, `application/javascript`, SVG, wasm) compressed
with brotli or gzip when the client's `Accept-Encoding` allows it and
the response is at least 256 bytes. `--compression off` disables this
entirely. Range requests are always served uncompressed.

## Examples

### Serve a static site

```sh
webrunner -p 3000 --no-index
```

### Serve a specific directory

```sh
webrunner --root ./docs
webrunner ./docs              # same thing, positional form
```

The path is canonicalised at startup; the `Serving …` banner shows the
absolute directory webrunner is actually serving. The default (no flag,
no positional) is unchanged: serve the current working directory.

### HTTPS with a self-signed cert

```sh
webrunner --https
# https://localhost:8443  (cert cached in ~/.config/webrunner/)
```

The certificate fingerprint is printed at startup so you can verify the
exception your browser shows.

### HTTPS with your own cert

```sh
webrunner --cert ./cert.pem --key ./key.pem
```

### HSTS (Strict-Transport-Security)

Pass `--hsts <SECS>` to send `Strict-Transport-Security:
max-age=<SECS>` on HTTPS responses. Use `--hsts-include-subdomains`
and `--hsts-preload` for the corresponding directives:

```sh
webrunner --https --hsts 60                                # short experiment
webrunner --https --hsts 31536000 --hsts-include-subdomains
webrunner --https --hsts 31536000 --hsts-include-subdomains --hsts-preload
```

**Caution for localhost work:** browsers cache HSTS for the full
`max-age` and apply it to all paths on that origin — testing HSTS
against `https://localhost` can leave a browser refusing
`http://localhost` (or other ports on that origin) until the cache
expires. Start with a short `max-age` (e.g. `--hsts 60`) when
experimenting.

### Restrict to loopback

```sh
webrunner --bind 127.0.0.1,::1
```

Use this on a shared dev box so only `localhost` clients can reach the
server. Comma-separated IP literals; repeat the flag if you prefer.

### Bind to a specific interface

```sh
webrunner --bind 192.168.1.10
```

Useful for serving the dev site to another device on the LAN (e.g. a
phone for mobile testing).

### CGI script

`hello.pl` (must be executable):

```perl
#!/usr/bin/perl
print "Content-Type: text/html\n\n";
print "<h1>Hello from Perl</h1>\n";
```

Browse to <http://localhost:8080/hello.pl>.

Scripts receive standard CGI/1.1 environment variables (`REQUEST_METHOD`,
`QUERY_STRING`, `CONTENT_TYPE`, `CONTENT_LENGTH`, `PATH_INFO`, `SCRIPT_NAME`,
`HTTP_*`, …). The POST body is delivered on stdin. Output is parsed as
headers + blank line + body; use `Status: 404 Not Found` to set a non-200
response.

CGI scripts that don't return within `--cgi-timeout` seconds (default
30) are killed and the client receives a 504. Pass `--cgi-timeout 0`
to disable the limit while debugging a slow script interactively.

### Executing server-side JavaScript / TypeScript

By default, `.js` and `.ts` files are served as static assets — which is
what a normal static site needs. To execute them server-side via `bun run`
instead, opt in with `--cgi`:

```sh
webrunner --cgi js,ts
```

The script's stdout is parsed as CGI output (headers, blank line, body),
same as for `.pl` / `.php`. `bun` must be on `PATH`.

### Serve `.pl` or `.php` files as static text

```sh
webrunner --no-cgi pl,php
```

Useful when serving a directory of Perl/PHP source for reading rather
than execution. Mutually exclusive with `--cgi` for the same extension.

### Front-controller rewrite + basic auth

`.htaccess`:

```apache
DirectoryIndex index.php

RewriteEngine On
RewriteCond %{REQUEST_FILENAME} !-f
RewriteCond %{REQUEST_FILENAME} !-d
RewriteRule ^(.*)$ index.php [L]

AuthType Basic
AuthName "Restricted Area"
AuthUserFile /absolute/path/to/.htpasswd
Require valid-user

ErrorDocument 404 /errors/404.html
```

Generate the password file with:

```sh
htpasswd -B .htpasswd alice
```

Supported hash formats: bcrypt (`$2y$…`), SHA-1 (`{SHA}…`), Apache MD5
(`$apr1$…`).

### Custom response headers

Use the `Header` directive in `.htaccess` to set, add, remove, or
rewrite response headers per directory:

```apache
# Security headers, applied to every response (including errors).
Header always set X-Frame-Options "DENY"
Header always set Referrer-Policy "no-referrer"

# CORS preflight responses.
Header set Access-Control-Allow-Origin "*"
Header set Access-Control-Allow-Methods "GET, POST, OPTIONS"

# Strip a leaky header.
Header unset X-Powered-By

# Echo selected request headers back (matched as regex on names).
Header echo "X-Forwarded-.*"

# Add a timing header for browser DevTools.
Header always set X-Request-Duration "%D us"
```

Supported actions: `set`, `setifempty`, `add`, `append`, `merge`,
`unset`, `echo`, `edit`, `edit*`. The optional `always|onsuccess`
modifier defaults to `onsuccess` (only applied to 2xx responses).
Placeholders in values: `%t` (Unix microseconds), `%D` (request
duration), `%l` (body size), `%s` (status), `%H` (protocol), `%m`
(method), `%U` (URL path), `%%` (literal `%`).

Note: when `--hsts` is set, the HSTS layer overwrites any
`Header set Strict-Transport-Security` for HTTPS responses.

## How `.htaccess` is resolved

For each request, webrunner walks from the document root down to the target
file's directory and merges the `.htaccess` files it finds. Deeper files
override shallower ones. Unknown directives are skipped with a warning on
stderr.

## Development

```sh
cargo test          # run the test suite
cargo build         # debug build
cargo build --release
```

The crate is organised by concern:

| Module          | Responsibility                                        |
| --------------- | ----------------------------------------------------- |
| `cli`           | Command-line parsing and validation                   |
| `server`        | HTTP/HTTPS listeners and graceful shutdown            |
| `handler`       | Request dispatch and `.htaccess` evaluation           |
| `static_files`  | Static file serving and directory listings            |
| `cgi`           | CGI/1.1 process spawning and response parsing         |
| `htaccess`      | `.htaccess` parser and merger                         |
| `rewrite`       | `RewriteRule` / `RewriteCond` engine                  |
| `auth`          | HTTP Basic auth with `.htpasswd` verification         |
| `mime`          | MIME-type lookup                                      |
| `tls`           | Self-signed cert generation and loading               |

## Caveats

- **Not a production server.** No request size limits, no rate limiting, no
  process isolation between CGI scripts. Use it for local development.
- CGI scripts inherit the parent's `PATH` so interpreters resolve correctly.
- Only a practical subset of `.htaccess` is implemented — the directives listed
  in the Features section. Unknown directives are warned and ignored.
- `<IfModule>` blocks are treated as passthrough (their contents always
  apply, since webrunner implements the relevant module semantics
  natively). `<FilesMatch "regex">` and `<Files "glob">` now scope
  contained `Header`, `RewriteRule`, auth directives, and `AddType` to
  files whose basename matches the pattern. `<Directory>` in
  `.htaccess` is recognized but doesn't scope (Apache itself disallows
  it there). `<If "expression">` is recognized but doesn't scope until
  the expression evaluator ships. Common Apache
  directives webrunner does not implement (`php_flag`, `php_value`,
  `FileETag`, `AddEncoding`, `AddCharset`,
  `AddOutputFilterByType`, `SetEnv`, `SetEnvIf`, `SetEnvIfNoCase`,
  `RequestHeader`, `DirectorySlash`) are silently skipped — pass
  `--log-level debug` to see what was skipped.
- mod_expires supports only the `access` / `now` base and
  single-component specs (`access plus 1 year`). `modification`
  base, legacy short form (`A3600`/`M86400`), and multi-component
  specs (`access plus 1 year 6 months`) are not yet implemented.

## License

[MIT](LICENSE) © Thomas Björk
