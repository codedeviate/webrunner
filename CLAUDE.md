# CLAUDE.md — project conventions for webrunner

This file documents the conventions Claude (and humans) should follow when
working in this repo. Keep it short; treat it as authoritative.

## Project overview

`webrunner` is a single-binary Rust development web server with CGI and
`.htaccess` support. Not for production. Source is organised by concern under
`src/` — see the module table in `README.md`.

## Versioning

Strict [Semantic Versioning 2.0.0](https://semver.org/):

- **MAJOR** — breaking changes to CLI flags, `.htaccess` semantics, on-disk
  layout (e.g. cert cache location), or anything a user's setup depends on.
- **MINOR** — backwards-compatible new features (new flag, new `.htaccess`
  directive, new auth hash format, etc.).
- **PATCH** — bug fixes and internal changes with no user-visible behavior
  change.

While the project is `0.x`, MINOR bumps may include breaking changes — call
those out explicitly in the changelog.

The version lives in `Cargo.toml` and **must** be bumped in the same commit
that tags a release.

**Tagging implies a GitHub release.** When you push a `vX.Y.Z` tag, immediately
create the matching GitHub release:

```sh
gh release create vX.Y.Z --generate-notes
```

This is step 9 of the release checklist below — don't skip it. A tag without a
release leaves the GitHub Releases page out of sync with tag history and hides
the version from anyone browsing the repo's front page.

## Commits

[Conventional Commits 1.0.0](https://www.conventionalcommits.org/):

```
<type>(<optional scope>): <subject>

<optional body>

<optional footer>
```

Types in use here:

| Type       | When                                                         |
| ---------- | ------------------------------------------------------------ |
| `feat`     | User-visible new capability                                  |
| `fix`      | Bug fix                                                      |
| `docs`     | Docs only (README, CHANGELOG, OUT-OF-SCOPE, comments)        |
| `refactor` | Internal restructuring with no behavior change               |
| `perf`     | Performance improvement                                      |
| `test`     | Adding or revising tests                                     |
| `chore`    | Tooling, deps, packaging, CI                                 |
| `build`    | Cargo/build-system changes                                   |

Breaking changes get a `!` after the type/scope (`feat!: …`) **and** a
`BREAKING CHANGE:` footer explaining the migration.

Subject is imperative, lowercase, no trailing period, ≤ 72 chars. Scope is
optional but useful (`feat(cgi): …`, `fix(htaccess): …`).

## Changelog

Maintained in `CHANGELOG.md` following
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/).

**Update the `[Unreleased]` section in the same commit as the change.** Do not
batch changelog edits at release time — they go stale. On release: rename
`[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD`, add a fresh empty `[Unreleased]`,
and update the link references at the bottom.

Sections, in order: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`,
`Security`. Skip empty sections.

## Out-of-scope / wishlist

`OUT-OF-SCOPE.md` is the single source of truth for **deferred features** and
**deliberate non-goals**. It doubles as the wishlist.

When something is descoped during a discussion, or when the user says "not
now," add it there with a one-line rationale. When picking it up later, move
the entry into the appropriate section of the changelog as part of the
implementing commit, and remove it from `OUT-OF-SCOPE.md`.

Don't silently drop ideas. If it's worth saying "no, not yet" out loud, it's
worth a line in `OUT-OF-SCOPE.md`.

## Man page

`man/webrunner.1` is the canonical man page. It is hand-written (groff /
troff format) and **must be kept in sync with the CLI** — any change to
`Cargo.toml::version`, `src/cli.rs` flags, or the `.htaccess` directive
list requires a matching edit in `man/webrunner.1`. Verify the rendered
output looks right after editing:

```sh
man ./man/webrunner.1   # macOS / BSD
groff -mandoc -Tutf8 ./man/webrunner.1 | less    # Linux
```

The `.TH` header at the top carries the version (`webrunner X.Y.Z`) and
date — bump both alongside the `Cargo.toml` version and CHANGELOG date
in the release commit.

Distributors (Homebrew, distro packagers) install this file under
`share/man/man1/`. Don't move or rename it without coordinating with
the Homebrew formula in
[`codedeviate/homebrew-cli`](https://github.com/codedeviate/homebrew-cli).

## README badges

`README.md` carries a shields.io badge header (GitHub, latest release,
crates.io, Homebrew tap, license, Rust edition + MSRV). Keep it in sync
with the project state:

- **Latest release** is hardcoded — bump in the release checklist
  (step 3).
- **License (MIT)** is hardcoded — update if `Cargo.toml::license`
  changes.
- **Rust edition + MSRV** is hardcoded — update if `Cargo.toml::edition`
  or `Cargo.toml::rust-version` changes.
- **GitHub / Homebrew tap** badges are hardcoded paths — update if
  the repo or tap location moves.
- **crates.io** badge is dynamic (shields.io reads the registry); no
  manual maintenance needed.

If you change any field above in `Cargo.toml`, update the matching
badge in `README.md` in the same commit. The badge header style mirrors
`codedeviate/loganalyzer` for cross-repo uniformity — don't drift
without a reason.

## Build, test, run

```sh
cargo build              # debug build
cargo build --release    # release build
cargo test               # full test suite — must pass before any commit
cargo clippy             # lint (fix or justify warnings before committing)
cargo fmt                # format
```

`cargo publish --dry-run --allow-dirty` validates crates.io metadata without
uploading — run it after touching `Cargo.toml` metadata fields.

When building the release target, **skip the debug target**. Don't run a bare
`cargo build` alongside `cargo build --release` to "also check debug" — if a
debug build is needed, it will be built separately or the user will explicitly
request it.

## Code style notes

- Modules are organised by concern, not by layer. Adding a new feature usually
  means extending an existing module or adding one peer to it under `src/`.
- Tests live in `#[cfg(test)] mod tests` blocks at the bottom of each module.
- No `unwrap()` in request-handling paths — return errors and let the handler
  translate them to HTTP responses.
- Keep `main.rs` thin: parsing, validation, banner, dispatch to `server::run`.
- Don't add dependencies casually. The dep list in `Cargo.toml` is curated;
  prefer the standard library or an existing dep over pulling something new.

## Release checklist

1. All work for the release is on `master` and `cargo test` passes.
2. Bump `version` in `Cargo.toml`.
3. Bump the `Latest release` badge version in `README.md` to match
   `Cargo.toml` (the badge is hardcoded — same convention as
   `codedeviate/loganalyzer`).
4. Bump the `.TH` header in `man/webrunner.1` — both the version
   (`webrunner X.Y.Z`) and the date (`"Month YYYY"`).
5. Convert `[Unreleased]` in `CHANGELOG.md` to `[X.Y.Z] - YYYY-MM-DD`; add a
   fresh empty `[Unreleased]`.
6. Move any newly-shipped items out of `OUT-OF-SCOPE.md` into the changelog
   entry.
7. Single commit: `chore(release): vX.Y.Z`.
8. Tag: `git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z`.
9. `gh release create vX.Y.Z --generate-notes`.
10. `cargo publish` (when ready for crates.io).
11. Update the Homebrew formula in the
   [`codedeviate/homebrew-cli`](https://github.com/codedeviate/homebrew-cli)
   tap (bump `url`, `sha256`, and `version` to match the new tag).
