# AGENTS.md — obsidian-daily-qs

Rust backend (`src/`, `Cargo.toml`) + QML frontend (`omarchy/`). Bar widget for Obsidian daily-note todos.

## Checks (run before push)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
node omarchy/model.test.mjs
node omarchy/widget-argv.test.mjs
```

## Local privacy — never expose vault contents to other local users

Treat Obsidian vault contents (note bodies, todo text, undo payloads, paths
that encode private data, secrets, tokens) as sensitive on a multi-user
machine. Marketplace review (#7777) has blocked verification for local
disclosure paths that look fine in single-user testing. Prefer the stricter
permission / transport from day one; do not rely on “fix permissions later”
or “argv is only briefly visible.”

### Process argv / environment

Anything private must not appear in process argv. On systems without
`hidepid`, other local users can read `/proc/<pid>/cmdline`. Prefer not to
put secrets in `/proc/<pid>/environ` either.

**Widget / Quattro `Process` invocations (required):**

- Pass sensitive values through a non-argv channel: stdin (preferred here), or
  another private transport that does not show up in `/proc/<pid>/cmdline`
  (and preferably not in `/proc/<pid>/environ` either).
- Keep non-sensitive flags (`--date`, `--line`, `--heading`, etc.) on argv as
  usual. `--vault` / `--archive-folder` are for interactive CLI use only: the
  widget sends those through stdin (see below).
- When you add a new Process launch or CLI flag that carries private data,
  wire the widget through stdin (or equivalent) from day one — do not put
  that data on the command line in QML.

**Concrete pattern in this repo (todo text and the vault root, #7777, #102):**

- Every backend launch from `omarchy/BarWidget.qml` (`watch`, `week`, actions)
  passes `--stdin` and one JSON payload written after start
  (`Process.stdinEnabled = true`, then `write(...)`, then close stdin). The
  payload always carries `vault` (and `archiveFolder` when the setting is
  set); mutations add their todo fields. Do **not** pass `--vault`,
  `--archive-folder`, `--text` or `--expect-text` from the widget.
- Payload shape: `{"vault":"...","archiveFolder":"...","text":"..."}` for add;
  `{"vault":"...","text":"...","expectText":"..."}` for edit; `{"vault":"..."}`
  for status/week/watch/undo/carry-over/open; `expectText` for
  toggle/delete/defer/indent/outdent when an expect check is needed.
- The CLI keeps `--vault` / `--archive-folder` / `--text` / `--expect-text`
  for interactive use, and `omarchy/widget-argv.test.mjs` guards the widget
  side against a reintroduced argv path.
- Exception: saving a *new* vault path from the panel calls `omarchy bar set
  … vaultPath <path>` — omarchy's settings CLI (`bin/omarchy-bar`) takes the
  value only as an argument. Keep it user-initiated and brief, and do not spawn
  anything else with the path in argv.

**Interactive CLI (allowed):** humans may pass `--text` / `--expect-text` (and
similar) at a shell. Prefer `--stdin` in scripts or anything that might appear
in process listings shared with other local users.

### Intermediate files (temps, undo, renames)

Vault directories are often traversable by other local users even when note
files themselves are `0600`. Any sibling temp, undo file, or staging path that
holds note content must not be world-/group-readable under a typical umask
(`022`) while that content exists.

**Required when writing private content to disk:**

- Create the file with the intended restrictive mode **before** writing bytes
  (Unix: `OpenOptionsExt::mode(...)` at `open` / `create_new` time). Do **not**
  write the full note under umask defaults and only `set_permissions` afterward —
  that leaves a readable window other local users can open.
- For atomic replace of an existing note: create the temp with the existing
  note's mode (or owner-only `0600` if the target does not exist yet); keep a
  post-create `set_permissions` only as a umask/exact-mode follow-up, never as
  the sole protection.
- Same rule for undo / cache / export helpers that persist vault text: owner-only
  (`0600`) at create time (see `src/undo.rs`), not “write then chmod.”

## Bundle rule — READ THIS, it breaks every release otherwise

The committed `omarchy/bin/` bundles are refreshed **only in the release PR**. Feature PRs leave `omarchy/bin/` alone: between releases the committed backend intentionally trails `src/`, because refreshing it on every Rust edit meant every merge to `main` re-conflicted every open PR on unmergeable binaries (a regenerated binary can never be merged three-way).

`scripts/verify-bundle.sh` has two modes:

- **Strict** — `make verify-bundle`, the tag Release workflow, any change that edits `omarchy/bin/**`, and any commit whose bundle is current: recorded hash, non-stripped/inspectable ELF, source fingerprint == `src/` + `Cargo.*` + the toolchain pin, and a byte-for-byte pinned rebuild.
- **Stale-tolerant** — `ci.yml` for commits that leave `omarchy/bin/` alone (`VERIFY_BUNDLE_ALLOW_STALE=1`): the same integrity checks, but a fingerprint that trails the tree is reported, not fatal, and the rebuild is skipped — it can only pass while the fingerprint is current.

When rebasing a branch that still carries old `chore: refresh marketplace bundle` commits, drop them: resolve the bundle conflicts with `main`'s side (e.g. `git checkout --ours -- omarchy/bin/` before `git rebase --continue`); the replayed commit collapses to empty, and the refresh happens in the release PR.

**Never commit a cross-built `obsidian-daily-qs-aarch64` from an x86_64 host.** `make bundle` locally builds both arches, but the aarch64 output is cross-compiled with `rust-lld` and is NOT byte-identical to the native ARM build that CI (`ci.yml:verify-bundle`) and `release.yml` rebuild on `ubuntu-24.04-arm`. Result: `verify-bundle: bundled binary for aarch64 does not match the reproducible build`, Release skipped. x86_64 local builds are fine (native == CI).

Refreshing the bundle (the release PR, or a change that deliberately edits `omarchy/bin/`):

1. Build + commit only what you can attest locally:
   ```bash
   scripts/build-bundle.sh x86_64-unknown-linux-musl
   VERIFY_BUNDLE_SKIP_REBUILD=1 scripts/verify-bundle.sh x86_64-unknown-linux-musl  # full rebuild check also OK on x86_64
   ```
   Do NOT commit `omarchy/bin/obsidian-daily-qs-aarch64*` from this host.
2. Get the native binaries from CI (per-arch native runners, uploads ELFs + hashes + srcid, does not push). On a PR that touches Rust, the workflow runs automatically; otherwise dispatch it:
 ```bash
 gh workflow run "Refresh marketplace bundle" --ref <branch>   # optional if a PR already triggered it
 gh run list --workflow=refresh-bundle.yml --branch <branch> --limit 1
 gh run download <run-id> --dir /tmp/opencode/refresh-bundle
 ```
3. Copy into place (`aarch64` from the ARM artifact, `x86_64` either — they must agree on `.srcid`):
   ```bash
   cp /tmp/opencode/refresh-bundle/omarchy-bin-aarch64-unknown-linux-musl/obsidian-daily-qs-aarch64 omarchy/bin/
   cp /tmp/opencode/refresh-bundle/omarchy-bin-aarch64-unknown-linux-musl/obsidian-daily-qs-aarch64.sha256 omarchy/bin/
   chmod 755 omarchy/bin/obsidian-daily-qs-aarch64
   VERIFY_BUNDLE_SKIP_REBUILD=1 scripts/verify-bundle.sh aarch64-unknown-linux-musl
   ```
4. Commit `omarchy/bin/obsidian-daily-qs-*{,.sha256}` + `.srcid` with that change. Committing a stale bundle is only fine for changes that leave `omarchy/bin/` alone — CI reports it and skips the rebuild (see the modes above); a bundle-touching change must include a verified rebuild.

## Releasing (tag-driven)

1. Bump `Cargo.toml`, `Cargo.lock` (via build), `manifest.json`, `CHANGELOG.md` (`[Unreleased]` → `## [X.Y.Z] - YYYY-MM-DD`).
2. Refresh the bundle in the release commit for the bumped source (only the release PR refreshes bundles), then commit as `chore: release vX.Y.Z`.
3. `make verify-bundle` (strict) must pass on the release commit; `ci.yml` takes the same strict path because the commit edits `omarchy/bin/`.
4. Push to `main`.
5. `git tag vX.Y.Z && git push origin vX.Y.Z` — `release.yml` re-verifies both arches natively, then publishes the GitHub Release. Never move a published tag; fix forward and retag only if the release workflow hasn't published yet.
