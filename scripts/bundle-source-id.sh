#!/usr/bin/env bash
# Fingerprint of the tracked inputs that rustc hashes into the bundled ELF.
#
# rustc's crate disambiguator is embedded in symbol names. Any change to this
# crate's source — including comments and docs — changes that hash, so the
# committed binary must be rebuilt in the same change. This script is the
# fast check; `scripts/verify-bundle.sh` still does a byte-for-byte musl
# rebuild as the marketplace attestation.
#
# Usage: scripts/bundle-source-id.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

{
  find src -type f -name '*.rs' | LC_ALL=C sort
  printf '%s\n' Cargo.toml Cargo.lock rust-toolchain.toml
} | xargs sha256sum | sha256sum | awk '{print $1}'
