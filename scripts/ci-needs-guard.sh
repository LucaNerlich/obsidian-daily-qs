#!/usr/bin/env bash
# Fail-closed gate: the `verify-bundle` CI job must not declare `needs:`.
#
# Usage: scripts/ci-needs-guard.sh <workflow.yml>
#
# Exit 0 -> the caller must fail, and the reason is printed:
#             * the job declares `needs:`, or
#             * the job could not be located in the file at all.
#           A renamed, moved, quoted, re-indented or CRLF-rewritten job must be
#           a loud failure, not a silently disarmed attestation gate.
# Exit 1 -> clean (job present, no `needs:`).
set -euo pipefail

file="${1:?usage: ci-needs-guard.sh <workflow.yml>}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ ! -f "$file" ]]; then
  echo "cannot check the verify-bundle job: $file does not exist"
  exit 0
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
# Normalise line endings so a CRLF checkout cannot hide the job or its keys.
tr -d '\r' < "$file" > "$tmp"

if ! grep -qE '^  "?verify-bundle"?:' "$tmp"; then
  echo "CI job verify-bundle is not defined in $file (renamed or moved?); the needs: guard cannot be evaluated"
  exit 0
fi

if awk -f "$here/ci-needs-guard.awk" "$tmp"; then
  echo "CI job verify-bundle must not use needs: (a failed format check must not skip this attestation)"
  exit 0
fi

exit 1
