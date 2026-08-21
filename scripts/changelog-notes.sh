#!/usr/bin/env bash
# Print the CHANGELOG section for a version (default: crate version).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="${1:-$(awk -F '"' '/^version = / {print $2; exit}' "$repo_root/Cargo.toml")}"

notes="$(awk -v ver="$version" '
  $0 ~ "^## \\[" ver "\\]" {p=1; next}
  p && $0 ~ /^## \[/ {exit}
  p {print}
' "$repo_root/CHANGELOG.md" | sed -e '1{/^$/d;}' -e '${/^$/d;}')"

# Fail closed: a typo'd version or forgotten changelog entry must block the
# release instead of publishing it with empty notes.
if [[ -z "$notes" ]]; then
  echo "changelog-notes: no CHANGELOG section for version $version" >&2
  exit 1
fi

printf '%s\n' "$notes"
