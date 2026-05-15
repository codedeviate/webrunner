# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).

> Versioning was adopted retroactively after the initial development burst on
> 2026-04-03. The 0.1.x and 0.2.0 entries below are reconstructed from
> `git log`; only 0.3.x onwards was authored against the SemVer / Keep a
> Changelog discipline described in `CLAUDE.md`.

## [Unreleased]

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

