#!/usr/bin/env bash
# Pack the committed bundled backend (not a fresh unattested build) plus LICENSE
# into the GitHub Release tarball layout used by previous tags.
#
# Usage: scripts/package-release.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin="$repo_root/omarchy/bin/obsidian-daily-qs"
license="$repo_root/LICENSE"
version="$(awk -F '"' '/^version = / {print $2; exit}' "$repo_root/Cargo.toml")"
asset="obsidian-daily-qs-${version}-linux-x86_64.tar.gz"
outdir="$repo_root/dist"

[[ -f "$bin" && -f "$license" ]] || {
  echo "package-release: missing $bin or $license" >&2
  exit 1
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
install -Dm755 "$bin" "$tmp/obsidian-daily-qs"
install -Dm644 "$license" "$tmp/LICENSE"

mkdir -p "$outdir"
tar -C "$tmp" -czf "$outdir/$asset" obsidian-daily-qs LICENSE
echo "packaged: $outdir/$asset"
sha256sum "$outdir/$asset"
