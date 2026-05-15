# Out of Scope & Wishlist

A living list of items raised during design, implementation, or feature sweeps
that are either explicitly deferred, decided against, or noted as "maybe
later". Also doubles as a wishlist — items under "Waiting" are things worth
building once someone explicitly asks. Kept here so ideas don't disappear
into the black hole of spec files after each release.

Organized into four buckets by reason for non-inclusion. When an item ships,
remove it from this file and note the shipping version in the CHANGELOG
entry rather than leaving a crossed-out line here.

- **Waiting** — can be done; nobody's asked for it.
- **Deferred** — possible to implement; actively put off (scope/complexity
  trade-off or waiting on a concrete use case).
- **Not yet supported** — blocked by upstream / ecosystem maturity; may ship
  when the blocker clears.
- **Out of scope** — fundamentally can't be implemented, architecturally
  mismatched, or intentionally declined by policy.

---

## Waiting

### Server / transport

- **HTTP→HTTPS redirect.** When both listeners are active, optionally redirect
  plain HTTP to HTTPS instead of serving on both.
- **HSTS header** on HTTPS responses (`Strict-Transport-Security`), opt-in.
- **Separate `--bind-http` / `--bind-https`** for asymmetric bind sets
  between protocols (currently `--bind` applies to both).

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

- **Per-directory script aliases** (`AddHandler`, `Action`).

### Operational

- **File watching / hot reload** of `.htaccess` so edits don't require
  request-time re-parse cost (or, conversely, don't get cached past edit).
- **Config file** (`webrunner.toml`) for repeated invocations.
- **Authenticated user in access logs.** The `%u` field stays `-`
  until the auth check threads the username out into the middleware
  context via request extensions.

### Distribution

- **Pre-built release binaries** attached to GitHub releases (linux/macos/win,
  x86_64/aarch64) so Homebrew's bottle path or scoop/winget can pull binaries
  instead of building from source every time.

## Deferred

- **HTTP/2** for HTTPS connections. Significant dependency and complexity
  cost; HTTP/1.1 is fine for a dev server until someone has a concrete need.
- **FastCGI** / **SCGI** support for persistent script processes (notably
  PHP-FPM). Worth doing once a user asks — adds a non-trivial transport layer
  and process-management surface.
- **Resource limits** on CGI children (CPU, memory, output size). Platform-
  specific (`setrlimit`, job objects) and only matters once people start
  pointing webrunner at untrusted scripts.

## Not yet supported

- **`homebrew-core` submission.** Blocked on notability rules
  (≥75 stars, ≥30 days, stable releases). Will revisit when the project
  clears the threshold. Until then, distribution goes through the
  [`codedeviate/homebrew-cli`](https://github.com/codedeviate/homebrew-cli)
  tap.

## Out of scope

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
