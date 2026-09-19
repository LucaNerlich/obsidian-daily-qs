# AGENTS.md — obsidian-daily-qs

Rust backend (`src/`, `Cargo.toml`) + QML frontend (`omarchy/`). Bar widget for Obsidian daily-note todos.

## Checks (run before push)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
node omarchy/model.test.mjs
```

## Bundle rule — READ THIS, it breaks every release otherwise

Any edit under `src/`, `Cargo.toml`, `Cargo.lock`, or `rust-toolchain.toml` requires a fresh `omarchy/bin/` bundle in the same change (`make verify-bundle` is the CI/release gate).

**Never commit a cross-built `obsidian-daily-qs-aarch64` from an x86_64 host.** `make bundle` locally builds both arches, but the aarch64 output is cross-compiled with `rust-lld` and is NOT byte-identical to the native ARM build that CI (`ci.yml:verify-bundle`) and `release.yml` rebuild on `ubuntu-24.04-arm`. Result: `verify-bundle: bundled binary for aarch64 does not match the reproducible build`, Release skipped. x86_64 local builds are fine (native == CI).

Correct flow for any Rust change:

1. Build + commit only what you can attest locally:
   ```bash
   scripts/build-bundle.sh x86_64-unknown-linux-musl
   VERIFY_BUNDLE_SKIP_REBUILD=1 scripts/verify-bundle.sh x86_64-unknown-linux-musl  # full rebuild check also OK on x86_64
   ```
   Do NOT commit `omarchy/bin/obsidian-daily-qs-aarch64*` from this host.
2. Get the native binaries from CI (per-arch native runners, uploads ELFs + hashes + srcid, does not push):
   ```bash
   gh workflow run "Refresh marketplace bundle" --ref <branch>
   gh run download <run-id> --dir /tmp/opencode/refresh-bundle
   ```
3. Copy into place (`aarch64` from the ARM artifact, `x86_64` either — they must agree on `.srcid`):
   ```bash
   cp /tmp/opencode/refresh-bundle/omarchy-bin-aarch64-unknown-linux-musl/obsidian-daily-qs-aarch64 omarchy/bin/
   cp /tmp/opencode/refresh-bundle/omarchy-bin-aarch64-unknown-linux-musl/obsidian-daily-qs-aarch64.sha256 omarchy/bin/
   chmod 755 omarchy/bin/obsidian-daily-qs-aarch64
   VERIFY_BUNDLE_SKIP_REBUILD=1 scripts/verify-bundle.sh aarch64-unknown-linux-musl
   ```
4. Commit `omarchy/bin/obsidian-daily-qs-*{,.sha256}` + `.srcid` together with the Rust edit. Source fingerprint covers comments/whitespace — a comment-only `src/*.rs` edit still needs a rebuild.

## Releasing (tag-driven)

1. Bump `Cargo.toml`, `Cargo.lock` (via build), `manifest.json`, `CHANGELOG.md` (`[Unreleased]` → `## [X.Y.Z] - YYYY-MM-DD`).
2. Refresh bundles per above, commit as `chore: release vX.Y.Z`, push to `main`.
3. `git tag vX.Y.Z && git push origin vX.Y.Z` — `release.yml` re-verifies both arches natively, then publishes the GitHub Release. Never move a published tag; fix forward and retag only if the release workflow hasn't published yet.
