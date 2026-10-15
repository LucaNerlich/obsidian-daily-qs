#!/usr/bin/env bash
# Marketplace-facing checks for the bundled backend.
#
# Reviewers bind approval to an exact SHA. At that SHA the committed ELF must
# be inspectable with `nm` (not stripped), its recorded hash must match the
# bytes in git, its source fingerprint must match src/ + Cargo.* + the
# toolchain pin (comments count), and a fresh pinned rebuild must be
# byte-identical. This script is the single gate used by CI and by the
# tag-release workflow so a later release cannot skip any of those
# attestations.
#
# Bundles are refreshed only in the release PR (AGENTS.md), so CI runs this
# in two modes. With VERIFY_BUNDLE_ALLOW_STALE=1 a source fingerprint that
# trails src/ is reported, not fatal, and the byte-for-byte rebuild is skipped
# (it can only pass while the fingerprint is current); the recorded-hash and
# inspectability attestations above still run. Every other caller — `make
# verify-bundle`, the tag Release workflow, any change that edits
# omarchy/bin/ — keeps the strict behavior.
#
# Usage: scripts/verify-bundle.sh [target-triple]
#   With no argument, verifies (and rebuilds) all supported targets (used by
#   `make verify-bundle` and the tag Release workflow).
#   With a target-triple argument, scopes both the fast inspect checks and the
#   rebuild-and-compare to just that target (used by CI's per-target matrix,
#   where each runner only has its own musl target's std installed — cross-
#   building the other target there produced a non-reproducible mismatch).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
all_targets=("x86_64-unknown-linux-musl" "aarch64-unknown-linux-musl")
if [[ $# -gt 0 ]]; then
  targets=("$1")
else
  targets=("${all_targets[@]}")
fi
bin_dir="$repo_root/omarchy/bin"

fail() {
  echo "verify-bundle: $*" >&2
  exit 1
}

cd "$repo_root"

# Set while a target's recorded source id trails the tree under
# VERIFY_BUNDLE_ALLOW_STALE (see the header); read after the loop.
stale=0

verify_one() {
  local target="$1"
  local arch="${target%%-*}"
  local bin="$bin_dir/obsidian-daily-qs-$arch"
  local expected_file="$bin_dir/obsidian-daily-qs-$arch.sha256"
  local srcid_file="$bin_dir/obsidian-daily-qs.srcid"

  # --- inspectability and recorded identity (fast; no rebuild) ---

  [[ -f "$bin" ]] || fail "missing committed ELF $bin"
  [[ -f "$expected_file" ]] || fail "missing $expected_file"

  local expected committed file_out recorded_srcid actual_srcid

  expected="$(awk '{print $1}' "$expected_file")"
  [[ ${#expected} -eq 64 ]] || fail "recorded hash in $expected_file is not a SHA-256"

  committed="$(sha256sum "$bin" | awk '{print $1}')"
  if [[ "$committed" != "$expected" ]]; then
    fail "committed ELF does not match the recorded hash
  recorded:  $expected
  committed: $committed
Run 'scripts/build-bundle.sh' and commit the binary, .sha256, and .srcid."
  fi

  [[ -f "$srcid_file" ]] || fail "missing $srcid_file
Run 'scripts/build-bundle.sh' and commit the binary, .sha256, and .srcid."
  recorded_srcid="$(awk '{print $1}' "$srcid_file")"
  [[ ${#recorded_srcid} -eq 64 ]] || fail "recorded source id in $srcid_file is not a SHA-256"
  actual_srcid="$("$repo_root/scripts/bundle-source-id.sh")"
  if [[ "$recorded_srcid" != "$actual_srcid" ]]; then
    if [[ "${VERIFY_BUNDLE_ALLOW_STALE:-}" == 1 ]]; then
      # Bundles are refreshed only in the release PR (AGENTS.md), so CI
      # accepts a trailing fingerprint for commits that leave omarchy/bin/
      # alone. The recorded hash and the inspectability checks above still
      # ran, and the rebuild below is skipped: it can only pass while the
      # fingerprint is current.
      stale=1
      echo "verify-bundle: committed ELF trails the tracked source by design; the rebuild is deferred to the release PR" >&2
      echo "  recorded source id: $recorded_srcid" >&2
      echo "  current source id:  $actual_srcid" >&2
    else
      fail "committed ELF is stale relative to the tracked Rust source
  recorded source id: $recorded_srcid
  current source id:  $actual_srcid

Comments, docs, and whitespace in src/*.rs all count: rustc hashes them
into symbol names (the crate disambiguator). Bundles are refreshed in the
release PR; a change that edits omarchy/bin/ must run 'scripts/build-bundle.sh'
and commit omarchy/bin/obsidian-daily-qs-*{,.sha256} and .srcid with it."
    fi
  fi

  file_out="$(file -b "$bin")"
  [[ "$file_out" == ELF* ]] || fail "committed file is not an ELF: $file_out"
  [[ "$file_out" == *"not stripped"* ]] || fail "committed ELF is stripped (marketplace review inspects symbols with nm): $file_out"

  command -v nm >/dev/null || fail "nm is required to attest inspectability"
  nm "$bin" >/dev/null 2>&1 || fail "nm cannot read the committed ELF"
  # Avoid grep -q: with pipefail, an early match SIGPIPEs nm and looks like failure.
  if ! nm "$bin" | grep 'obsidian_daily_qs' >/dev/null; then
    fail "committed ELF has no crate symbols; it is not inspectable against the tracked Rust source"
  fi

  echo "verified ($arch): hash $committed, source id $actual_srcid, non-stripped"
}

if grep -Eq '^strip[[:space:]]*=[[:space:]]*(true|"symbols"|"all")' "$repo_root/Cargo.toml"; then
  fail "Cargo.toml must not fully strip the release binary (use strip = \"debuginfo\")"
fi
grep -q '^strip = "debuginfo"' "$repo_root/Cargo.toml" || fail "Cargo.toml [profile.release] must set strip = \"debuginfo\" so nm can inspect the bundle"

# Match the semantic `components = [...]` line, not any mention in a comment.
toolchain_components="$(grep -E '^components[[:space:]]*=' "$repo_root/rust-toolchain.toml" || true)"
grep -q 'rustfmt' <<<"$toolchain_components" || fail "rust-toolchain.toml must pin rustfmt on this channel (installing it only on stable skips CI format, which used to skip this job)"
grep -q 'clippy' <<<"$toolchain_components" || fail "rust-toolchain.toml must pin clippy on this channel"

# Fail-closed: exit 0 from the guard means the caller must abort, whether the
# job declares `needs:` or can no longer be located at all.
if guard_reason="$(scripts/ci-needs-guard.sh "$repo_root/.github/workflows/ci.yml")"; then
  fail "$guard_reason"
fi

crate_version="$(awk -F '"' '/^version = / {print $2; exit}' "$repo_root/Cargo.toml")"
manifest_version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$repo_root/manifest.json")"
[[ "$crate_version" == "$manifest_version" ]] || fail "Cargo.toml version ($crate_version) != manifest.json version ($manifest_version)"

# The manifest is the contract the settings UI is generated from: every default
# must have a schema field, and the widget must not read a setting the manifest
# does not declare. That drift is invisible — it just drops a field from the
# settings form — and it is exactly how the archive-folder setting came to be
# unreachable in the UI (#57).
python3 - "$repo_root/manifest.json" "$repo_root/omarchy/BarWidget.qml" <<'PY' || fail "manifest settings drifted from the widget (see the message above)"
import json, re, sys

manifest, qml = sys.argv[1], sys.argv[2]
bar = json.load(open(manifest))["barWidget"]
declared = set(bar["defaults"])
schema = {entry["key"] for entry in bar["schema"]}
if declared != schema:
    sys.exit(f"manifest defaults/schema key mismatch: {sorted(declared ^ schema)}")
read = set(re.findall(r'setting\(\s*"([^"]+)"', open(qml).read()))
undeclared = read - declared
if undeclared:
    sys.exit(f"BarWidget.qml reads settings the manifest does not declare: {sorted(undeclared)}")
PY

# The committed ELF is never executed here: until the byte-for-byte rebuild
# below has passed, it is untrusted input, and running it (e.g. `--version`)
# would let it tamper with this script, PATH, or the toolchain before
# attestation completes. Version identity is attested by the manifest/tag
# alignment above plus the rebuild comparison against the tracked source.

if [[ "${GITHUB_REF_TYPE:-}" == tag ]]; then
  tag="${GITHUB_REF_NAME#v}"
  [[ "$crate_version" == "$tag" ]] || fail "git tag ${GITHUB_REF_NAME} does not match crate version $crate_version"
fi

for target in "${targets[@]}"; do
  verify_one "$target"
done

if [[ "$stale" == 1 ]]; then
  echo "verified: committed ELFs are non-stripped, version $crate_version; bundle is stale by design (rebuild skipped)"
  exit 0
fi

if [[ "${VERIFY_BUNDLE_SKIP_REBUILD:-}" == 1 ]]; then
  echo "verified: committed ELFs are non-stripped, version $crate_version (rebuild skipped)"
  exit 0
fi

# --- byte-for-byte rebuild of the tracked source ---

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Same reproducibility flags the bundle was built with, so the rebuild
# compares byte-for-byte; see scripts/bundle-env.sh.
source "$repo_root/scripts/bundle-env.sh"
bundle_env "${targets[@]}"
export CARGO_TARGET_DIR="$tmp/target"

for target in "${targets[@]}"; do
  arch="${target%%-*}"
  expected="$(awk '{print $1}' "$bin_dir/obsidian-daily-qs-$arch.sha256")"

  cargo build --release --locked --target "$target"

  actual="$(sha256sum "$tmp/target/$target/release/obsidian-daily-qs" | awk '{print $1}')"
  if [[ "$expected" != "$actual" ]]; then
    fail "bundled binary for $arch does not match the reproducible build of the tracked source
  expected: $expected
  actual:   $actual

The source fingerprint matches, so this is a toolchain/flags drift rather
than a forgotten rebuild. Run 'scripts/build-bundle.sh' with the pinned
1.98.1 musl toolchain and commit the result."
  fi
  echo "rebuild verified ($arch): $actual"
done

echo "verified: all committed ELFs are non-stripped, version $crate_version, and match reproducible builds"
