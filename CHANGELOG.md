# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- HTTP and HTTPS listeners (concurrent when `--https` is set), with
  graceful shutdown on `Ctrl+C` / `SIGTERM`.
- Auto-generated self-signed TLS certificates cached in
  `~/.config/webrunner/`, with startup-printed cert fingerprint.
  Bring-your-own cert via `--cert` / `--key`.
- Static file serving with MIME-type detection.
- Directory listings, toggleable via `--no-index` and `Options ±Indexes`.
- CGI/1.1 execution for `.pl`, `.php`, `.ts`, and `.js` scripts. Standard
  CGI environment variables, POST body on stdin, header-parsed responses
  with `Status:` support. Parent `PATH` is preserved when spawning.
- `.htaccess` parser with per-directory override and merge-from-root
  resolution. Supported directives: `DirectoryIndex`, `Options ±Indexes`,
  `ErrorDocument`, `AddType`, `AddDefaultCharset`, `Redirect` (301/302),
  `RewriteEngine` / `RewriteRule` / `RewriteCond` (with `[QSA]`, `[L]`,
  `[R]` flags), and `AuthType Basic` with `AuthUserFile` / `AuthName` /
  `Require valid-user`.
- HTTP Basic auth with `.htpasswd` files supporting bcrypt (`$2y$…`),
  SHA-1 (`{SHA}…`), and Apache MD5 (`$apr1$…`) hashes. Missing
  `AuthUserFile` denies the request rather than allowing it.
- `--examples` flag prints a complete usage cookbook covering CGI,
  `.htaccess`, auth, rewrites, and HTTPS.
- Startup warnings for missing CGI interpreters (`perl`, `php`, `bun`).
- Project documentation: `README.md`, `LICENSE` (MIT), `CLAUDE.md`,
  `OUT-OF-SCOPE.md`, this changelog, and a draft Homebrew formula under
  `packaging/homebrew/`.
- Crates.io-ready metadata in `Cargo.toml` (description, authors,
  repository, keywords, categories, MSRV `1.75`).

[Unreleased]: https://github.com/codedeviate/webrunner/compare/HEAD...HEAD
