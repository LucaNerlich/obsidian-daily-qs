#!/usr/bin/env bash
# Print the CHANGELOG section for a version (default: crate version).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="${1:-$(awk -F '"' '/^version = / {print $2; exit}' "$repo_root/Cargo.toml")}"

awk -v ver="$version" '
  $0 ~ "^## \\[" ver "\\]" {p=1; next}
  p && $0 ~ /^## \[/ {exit}
  p {print}
' "$repo_root/CHANGELOG.md" | sed -e '1{/^$/d;}' -e '${/^$/d;}'
