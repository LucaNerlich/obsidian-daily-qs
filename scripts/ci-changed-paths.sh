#!/usr/bin/env bash
# Changed-path facts for the CI gates, in one place.
#
# Usage: scripts/ci-changed-paths.sh <event-name> <pr-base-sha> <push-before-sha> [head]
#
# Prints two `key=value` lines, ready to append to $GITHUB_OUTPUT:
#   rust=true|false     src/, tests/, Cargo.*, the toolchain pin, scripts, CI
#   bundle=true|false   omarchy/bin/ (the committed marketplace bundles)
#
# `bundle=true` means "this change edits the committed bundle", so the caller
# must verify it strictly. `bundle=false` only ever relaxes the freshness half
# of the attestation, and only while the committed bundle is already stale by
# design: bundles are refreshed only in the release PR (see AGENTS.md).
#
# Anything we can't compute a sensible diff base for (workflow_dispatch, first
# push, force-push whose `before` was rewritten away) defaults to `true` so we
# never silently skip a check we're unsure about.
set -euo pipefail

event="${1:-}"
pr_base="${2:-}"
push_before="${3:-}"
head="${4:-HEAD}"

base=""
case "$event" in
  pull_request) base="$pr_base" ;;
  push) base="$push_before" ;;
esac

if [[ -z "$base" ]] || ! git cat-file -e "$base" 2>/dev/null; then
  echo "rust=true"
  echo "bundle=true"
  exit 0
fi

files="$(git diff --name-only "$base" "$head")"

if grep -qE '^(src/|tests/|Cargo\.(toml|lock)$|rust-toolchain\.toml$|omarchy/bin/|scripts/.*\.sh$|Makefile$|\.github/workflows/ci\.yml$)' <<<"$files"; then
  echo "rust=true"
else
  echo "rust=false"
fi

if grep -qE '^omarchy/bin/' <<<"$files"; then
  echo "bundle=true"
else
  echo "bundle=false"
fi
