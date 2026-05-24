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

**Tagging implies three downstream publishes — same release, no exceptions.**
When you push a `vX.Y.Z` tag, the release isn't complete until all three
surfaces below are updated. A tag with only one or two updated leaves install
paths out of sync: shields.io badges turn stale, `cargo install webrunner` and
`brew upgrade webrunner` keep returning the old version, and anyone following
the README installs an older binary. Treat all three as part of the tag —
same flow, no follow-up commits needed on this repo itself. These map to
steps 9, 10, and 11 of the release checklist below.

### 1. GitHub release (step 9)

```sh
gh release create vX.Y.Z --generate-notes
```

### 2. crates.io (step 10)

From the repo root, with the new version already in `Cargo.toml`:

```sh
cargo publish
```

The crate name is **`webrunner`** (per `Cargo.toml [package] name`); the
binary is also `webrunner`. Requires `cargo login` to have been run once
(token stored in `~/.cargo/credentials.toml`). `cargo publish` runs its own
checks (clean working tree, no path-dependencies, etc.) and aborts cleanly
if anything is wrong — fix the underlying issue rather than passing
`--allow-dirty`. Use `cargo publish --dry-run --allow-dirty` to validate
metadata without uploading (covered in "Build, test, run" below).

### 3. Homebrew tap (`../homebrew-cli/`) (step 11)

The tap repo at `../homebrew-cli/` carries `Formula/webrunner.rb`. Update
two fields:

- `url "https://github.com/codedeviate/webrunner/archive/refs/tags/vX.Y.Z.tar.gz"`
- `sha256 "<new-tarball-sha256>"`

Compute the sha256 from the GitHub-generated tarball after the release
exists. **Always pass `-H "Cache-Control: no-cache"` so the fetch bypasses
any intermediate caches (your ISP, corporate proxy, local resolver) and
goes through to GitHub's origin:**

```sh
curl -sL -H "Cache-Control: no-cache" \
    https://github.com/codedeviate/webrunner/archive/refs/tags/vX.Y.Z.tar.gz \
    | shasum -a 256
```

**Important: GitHub's auto-generated tarball CDN can serve a
transient/incomplete payload for the first minute or two after the tag is
pushed.** Run the `shasum` command twice with a short pause between, and
only proceed if both runs return the same hash. If they differ, wait 30–60
seconds and re-check until the hash stabilises. Using an unstable hash is
the single most common cause of "homebrew reports wrong checksum" reports
after a release — `webrunner 0.8.1 — fix sha256` in the tap log was
exactly this scenario, and recon hit the same race twice (v0.82.0 and
v0.85.0). v0.85.0's recheck without `Cache-Control: no-cache` re-read the
same cached payload twice (a "stable" but wrong hash) and the mismatch
surfaced only when users ran `brew install`.

The `Cache-Control: no-cache` rule isn't optional even on a "fresh" shell.
Network paths cache aggressively; a single `curl` without the header is
allowed to return whatever was last cached for that URL — which can be an
early CDN payload that no longer matches what GitHub serves to homebrew
clients.

Then commit and push the tap repo:

```sh
cd ../homebrew-cli
git add Formula/webrunner.rb
git commit -m "webrunner X.Y.Z"
git push origin main   # tap default branch is `main`, not `master`
```

(Tap commits follow the convention `<formula> X.Y.Z` — see `git log
--oneline` in `../homebrew-cli` for examples.)

If `shasum` produces a hash that, after pasting into `webrunner.rb`, makes
`brew install --build-from-source webrunner` fail with "SHA256 mismatch",
recompute against the URL the formula points to (case matters — `vX.Y.Z`
not `VX.Y.Z`) and amend with a follow-up commit like the existing
`webrunner 0.8.1 — fix sha256` precedent in the tap log.

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
