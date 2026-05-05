# Out of scope / wishlist

A single source of truth for things that have been **considered and deferred**
or are **deliberate non-goals** for `webrunner`. Doubles as a wishlist: items
under "Deferred" are open to implementation; items under "Non-goals" should
stay out unless the project's purpose changes.

When picking up a deferred item, move it into `CHANGELOG.md`'s `[Unreleased]`
section as part of the implementing commit and remove it from this file.

## Deferred — open to implementation

### Server / transport

- **HTTP→HTTPS redirect.** When both listeners are active, optionally redirect
  plain HTTP to HTTPS instead of serving on both.
- **HSTS header** on HTTPS responses (`Strict-Transport-Security`), opt-in.
- **HTTP/2** for HTTPS connections.
- **IPv6 binding.** Currently binds `0.0.0.0`; should also bind `::` or take a
  configurable bind address.
- **Range requests** (`Range:` / `206 Partial Content`) for static files —
  matters for video and large downloads.
- **Conditional requests** — `ETag`, `Last-Modified`, `If-None-Match`,
  `If-Modified-Since` for static files.
- **Compression** — `Content-Encoding: gzip` / `br` for compressible MIME
  types, gated by `Accept-Encoding`.

### `.htaccess` directives

- `Header set/add/unset` (mod_headers).
- `ExpiresActive` / `ExpiresByType` / `ExpiresDefault` (mod_expires).
- `Allow` / `Deny` / `Require ip` for IP-based access control.
- `Require user <name>` and `Require group` (currently only `valid-user`).
- `AuthDigest` for digest auth.
- `RewriteBase`, `RewriteMap`.
- More `RewriteRule` flags: `[F]`, `[G]`, `[NC]`, `[P]`, `[E]`.
- `<Files>`, `<FilesMatch>`, `<Directory>`, `<IfModule>` blocks.

### CGI

- **FastCGI** / **SCGI** support for persistent script processes (notably
  PHP-FPM).
- **Per-directory script aliases** (`AddHandler`, `Action`).
- **CGI timeouts** — currently scripts can hang indefinitely.
- **Resource limits** on CGI children (CPU, memory, output size).

### Operational

- **Access logs** in Common or Combined Log Format, with a `--log` flag.
- **Configurable log level** (currently warnings to stderr; nothing else).
- **File watching / hot reload** of `.htaccess` so edits don't require
  request-time re-parse cost (or, conversely, don't get cached past edit).
- **`--bind <ADDR>`** flag to choose the bind address.
- **`--root <DIR>`** flag to serve a directory other than the cwd.
- **Config file** (`webrunner.toml`) for repeated invocations.

### Distribution

- **Pre-built release binaries** attached to GitHub releases (linux/macos/win,
  x86_64/aarch64) so Homebrew's bottle path or scoop/winget can pull binaries
  instead of building from source every time.
- **`homebrew-core` submission** once the project meets notability rules
  (≥75 stars, ≥30 days, stable releases). Tracked in `BREW.md`.

## Non-goals — deliberately out

- **Production deployment.** `webrunner` is a development server. No request
  size limits, rate limiting, slowloris protection, or process isolation
  between CGI children. Use nginx/Caddy/Apache for production.
- **Reverse proxy / load balancer.** Out of scope; that's what nginx is for.
- **Virtual hosts.** A `webrunner` invocation serves one document root.
- **PHP without `php` installed.** `webrunner` is a CGI host, not a PHP
  runtime — the `php` binary must be in `PATH`.
- **Apache parity.** We implement a useful, documented subset of `.htaccess`.
  Full mod_rewrite / mod_auth feature parity is not a goal.
- **WebSocket / SSE proxying.** Possible to host a CGI script that streams,
  but no first-class WebSocket upgrade support is planned.
- **Mutating HTTP methods on static files.** `PUT`, `DELETE` etc. on the
  static-file path return 405; webrunner is not a WebDAV server.
