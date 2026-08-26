#!/usr/bin/env bash
# Pack the committed bundled backends (not a fresh unattested build) plus LICENSE
# into per-architecture GitHub Release tarballs.
#
# Usage: scripts/package-release.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
license="$repo_root/LICENSE"
version="$(awk -F '"' '/^version = / {print $2; exit}' "$repo_root/Cargo.toml")"
outdir="$repo_root/dist"
archs=("x86_64" "aarch64")

mkdir -p "$outdir"

for arch in "${archs[@]}"; do
  bin="$repo_root/omarchy/bin/obsidian-daily-qs-$arch"
  asset="obsidian-daily-qs-${version}-linux-${arch}.tar.gz"

  [[ -f "$bin" && -f "$license" ]] || {
    echo "package-release: missing $bin or $license" >&2
    exit 1
  }

  tmp="$(mktemp -d)"
  install -Dm755 "$bin" "$tmp/obsidian-daily-qs"
  install -Dm644 "$license" "$tmp/LICENSE"

  tar -C "$tmp" -czf "$outdir/$asset" obsidian-daily-qs LICENSE
  rm -rf "$tmp"
  echo "packaged: $outdir/$asset"
  sha256sum "$outdir/$asset"
done
