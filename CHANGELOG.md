# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).

> Versioning was adopted retroactively after the initial development burst on
> 2026-04-03. The 0.1.x and 0.2.0 entries below are reconstructed from
> `git log`; only 0.3.x onwards was authored against the SemVer / Keep a
> Changelog discipline described in `CLAUDE.md`.

## [Unreleased]

### Changed

- Access log `%u` field is now populated with the authenticated
  username for requests that passed `AuthType Basic` auth.
  Previously always `-`.

## [0.5.0] - 2026-05-17

### Added

- `--bind <ADDR[,ADDR...]>` flag to choose the listener address(es).
  Accepts IPv4 and IPv6 literals; comma-separated and/or repeatable.
- `--root <DIR>` flag (and equivalent positional argument) for
  choosing the document root. The path is canonicalised at startup
  and validated to be an existing directory. Default behaviour
  (current working directory) is unchanged.
- `--no-cgi <ext[,ext...]>` flag to disable always-on CGI execution
  for `pl` and/or `php`. Mirror of `--cgi`. Passing the same
  extension to both `--cgi` and `--no-cgi` is rejected at startup.
- `--cgi-timeout <SECS>` flag to configure the CGI script timeout
  (default 30 seconds; `0` disables). Previously fixed at 30
  seconds.
- `--log-level <off|warn|info|debug>` flag for filtering output.
  Default `info` preserves prior behaviour.
- `--log <PATH>` (or `--log -`) flag to write per-request access log
  lines in Apache Combined Log Format. Disabled by default.
- Conditional GET support for static files via `If-Modified-Since`
  (complements the existing `If-None-Match` / ETag handling).
  Returns 304 when the file has not been modified since the
  client's reported time.
- `--compression <on|off>` flag (default `on`) enabling brotli / gzip
  Content-Encoding for compressible static files (text/*,
  application/json, application/javascript, SVG, wasm). Responses
  under 256 bytes and Range responses are served uncompressed.
- `--hsts <SECS>` flag (plus `--hsts-include-subdomains` and
  `--hsts-preload` modifiers) to send `Strict-Transport-Security`
  on HTTPS responses. Default: header not sent. HTTPS-only per
  RFC 6797 §7.2.

### Changed

- webrunner now binds to `0.0.0.0` and `::` by default (previously
  `0.0.0.0` only). Existing IPv4 access is unaffected; IPv6 clients
  can now reach the server out of the box. On systems where IPv6 is
  disabled at the kernel level, the IPv6 listener is skipped with a
  stderr warning and the server continues on IPv4.
- Output now routes via the `log` crate. Error/warning messages go to
  stderr (unchanged); informational messages (startup banner, shutdown
  notice, cert generation) go to stdout. Existing message prefixes
  (`[CGI ERROR]`, `[.htaccess]`, `[auth]`, etc.) are preserved.

### Fixed

- CGI children are now killed when the timeout fires. Previously
  the script remained running after the 504 response was sent,
  leaving an orphaned process until it finished on its own.
- `REMOTE_ADDR` in CGI env now reflects the actual peer IP.
  Previously hard-coded to `127.0.0.1`.

## [0.4.0] - 2026-05-15

### Added

- `--cgi <ext[,ext...]>` flag to opt extensions into CGI execution
  on top of the always-on `pl` / `php` defaults.

### Changed

- **Breaking (0.x):** `.js` and `.ts` files are no longer executed as
  CGI by default — they are served as static assets so plain HTML +
  JS sites work without configuration. To restore the previous
  behaviour, pass `--cgi js,ts`.

## [0.3.1] - 2026-05-08

### Added

- `README.md` install section now lists
  `brew install codedeviate/cli/webrunner` and `cargo install webrunner`
  alongside the build-from-source path.

### Changed

- Homebrew formula moved out of this repo and into the dedicated
  [`codedeviate/homebrew-cli`](https://github.com/codedeviate/homebrew-cli)
  tap. `packaging/homebrew/` and the matching `Cargo.toml` excludes were
  removed.
- `OUT-OF-SCOPE.md` restructured into four explicit buckets — *Waiting*,
  *Deferred*, *Not yet supported*, *Out of scope* — for clearer triage.
- Release-checklist step in `CLAUDE.md` now points at the external tap
  instead of the (never-committed) `BREW.md`.
- `Cargo.toml` `exclude` list now drops `.git/` and `OUT-OF-SCOPE.md`
  from the published tarball, leaving only public source and user-facing
  docs.

## [0.3.0] - 2026-05-05

### Added

- `README.md` with feature overview, install instructions, usage examples,
  and the per-module responsibility map.
- `LICENSE` (MIT).
- `CLAUDE.md` documenting project conventions: SemVer, Conventional
  Commits, Keep a Changelog discipline, and the release checklist.
- `CHANGELOG.md` (this file) and `OUT-OF-SCOPE.md` for tracking deferred
  work.
- Crates.io-ready package metadata in `Cargo.toml`: `description`,
  `license`, `authors`, `repository`, `homepage`, `documentation`,
  `keywords`, `categories`, MSRV `1.75`, and a curated `exclude` list.

## [0.2.0] - 2026-04-03

### Added

- `--examples` flag prints a complete usage cookbook covering CGI,
  `.htaccess`, auth, rewrites, and HTTPS, then exits.

## [0.1.1] - 2026-04-03

### Fixed

- Auth now denies the request when `AuthUserFile` is missing instead of
  silently allowing it.
- `CONTENT_LENGTH` is now derived from the body when the client does not
  supply it, so CGI scripts see the correct value.
- `AddDefaultCharset` is honored on text responses.
- CGI children now inherit the parent's `PATH` so interpreter resolution
  works from a stripped environment.
- `Range` request seek failures are surfaced as proper HTTP responses
  instead of propagating I/O errors.
- Removed a dead `error_doc` parameter from the static-file path.

### Removed

- Dropped the unused `mime_guess` dependency in favor of the in-tree
  `mime` module.

## [0.1.0] - 2026-04-03

### Added

- HTTP and HTTPS listeners (concurrent when `--https` is set) with
  graceful shutdown on `Ctrl+C` / `SIGTERM`.
- Auto-generated self-signed TLS certificates cached in
  `~/.config/webrunner/`, with startup-printed certificate fingerprint.
  Bring-your-own cert via `--cert` / `--key`.
- Static file serving with MIME-type detection.
- Directory listings, toggleable via `--no-index` and
  `Options ±Indexes`.
- CGI/1.1 execution for `.pl`, `.php`, `.ts`, and `.js` scripts.
  Standard CGI environment variables, POST body on stdin, header-parsed
  responses with `Status:` support.
- `.htaccess` parser with per-directory override and merge-from-root
  resolution. Supported directives: `DirectoryIndex`, `Options ±Indexes`,
  `ErrorDocument`, `AddType`, `AddDefaultCharset`, `Redirect` (301/302),
  `RewriteEngine` / `RewriteRule` / `RewriteCond` (with `[QSA]`, `[L]`,
  `[R]` flags), and `AuthType Basic` with `AuthUserFile` / `AuthName` /
  `Require valid-user`.
- HTTP Basic auth with `.htpasswd` files supporting bcrypt (`$2y$…`),
  SHA-1 (`{SHA}…`), and Apache MD5 (`$apr1$…`) hashes.
- CLI argument parsing via `clap` (`--port`, `--https`, `--https-port`,
  `--cert`, `--key`, `--no-index`).
- Startup warnings for missing CGI interpreters (`perl`, `php`, `bun`).

