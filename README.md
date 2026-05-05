# webrunner

A zero-config development web server in Rust, with CGI and `.htaccess` support.

Point it at a directory and you get static files, directory listings, classic CGI
scripts (Perl/PHP/TypeScript via `bun`), URL rewriting, basic auth, custom error
pages, and self-signed HTTPS — all driven by familiar Apache-style `.htaccess`
files.

Built for local development and quick prototyping. Not a production server.

## Features

- Static file serving with automatic MIME detection
- Directory listings (toggleable per-request and via `Options ±Indexes`)
- CGI/1.1 execution for `.pl`, `.php`, `.ts`, and `.js` scripts
- `.htaccess` support with per-directory overrides:
  - `DirectoryIndex`, `Options ±Indexes`
  - `ErrorDocument` for custom error pages
  - `AddType`, `AddDefaultCharset`
  - `Redirect` (301/302)
  - `RewriteEngine`, `RewriteRule`, `RewriteCond` (incl. `[QSA]`, `[L]`, `[R]`)
  - `AuthType Basic` with `.htpasswd` (bcrypt, SHA-1, Apache MD5)
- HTTPS with auto-generated self-signed certificates (cached in
  `~/.config/webrunner/`) or your own cert/key
- Simultaneous HTTP + HTTPS listeners
- Graceful shutdown on `Ctrl+C` / `SIGTERM`

## Install

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
      --examples             Print rich usage examples and exit
  -h, --help                 Show help
  -V, --version              Show version
```

Run `webrunner --examples` for a complete cookbook covering CGI, `.htaccess`,
auth, rewrites, and HTTPS.

## Examples

### Serve a static site

```sh
webrunner -p 3000 --no-index
```

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

## License

[MIT](LICENSE) © Thomas Björk
