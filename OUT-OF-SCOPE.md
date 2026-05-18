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
- **Separate `--bind-http` / `--bind-https`** for asymmetric bind sets
  between protocols (currently `--bind` applies to both).

### `.htaccess` directives

- `Allow` / `Deny` / `Require ip` for IP-based access control.
- `Require user <name>` and `Require group` (currently only `valid-user`).
- `AuthDigest` for digest auth.
- `RewriteBase`, `RewriteMap`.
- More `RewriteRule` flags: `[F]`, `[G]`, `[NC]`, `[P]`, `[E]`.
  `[E=VAR:value]` depends on the env-var system (see `SetEnv` /
  `SetEnvIf` under Deferred). `[P]` is reverse-proxy territory and
  conflicts with the Out-of-scope policy.

### CGI

- **Per-directory script aliases** (`AddHandler`, `Action`).
- **`REMOTE_USER` in CGI environment.** When webrunner-managed
  `AuthType Basic` precedes a CGI script, the script does not see
  `REMOTE_USER=<name>` in its env. The username is already known
  inside the handler (see access-log `%u`); plumbing it into
  `build_cgi_env` is a small follow-up.

### Operational

- **File watching / hot reload** of `.htaccess` so edits don't require
  request-time re-parse cost (or, conversely, don't get cached past edit).
- **Config file** (`webrunner.toml`) for repeated invocations.

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
- **`mod_headers` placeholders for request/response headers**
  (`%{NAME}i`, `%{NAME}o`). Small extension to the placeholder
  resolver in `header_directive.rs`. Subproject #3 of the
  full-mod_headers roadmap.
- **`SetEnv` / `SetEnvIf` / `SetEnvIfNoCase` directives + env-var
  placeholders (`%{NAME}e`) + `env=VAR` Header condition + `[E]`
  rewrite flag.** Requires a per-request env-var bag. Subproject
  #4 of the full-mod_headers roadmap; also unblocks the `[E]`
  rewrite flag and the `RequestHeader` directive.
- **`expr=` conditions on Header rules.** Apache `ap_expr` subset.
  Requires an expression evaluator. Subproject #5.
- **SSL placeholders (`%{NAME}s`).** Surface rustls connection
  info (cipher, protocol version, peer cert) to Header values.
  Subproject #6.
- **Regex named-capture placeholders (`%{NAME}r`).** Refers to
  named captures from rewrite rules. Requires rewrite engine
  refactor. Subproject #6.
- **`<Directory>` in `.htaccess`** — Apache itself disallows it
  there; webrunner follows. If actually-functional `<Directory>`
  ever becomes useful (e.g., for nested per-directory `.htaccess`
  merge override behaviour), it builds on sub-project B's scope
  stack.
- **`modification` base, legacy `A<N>`/`M<N>` short form, and
  multi-component specs** for mod_expires directives. The basic
  `[access|now] plus N <unit>` form is supported.
- **`<If "expression">` evaluator** — needs the mod_headers
  subproject #5 (`expr=`) evaluator first.
- **`RequestHeader` directive** — modifies request headers (different
  from `Header` which modifies responses). Adjacent to mod_headers
  subproject #4 (env-var system) but works on the request side.

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
