#!/usr/bin/env bash
# Reproducibly build the bundled backends and record their hashes.
#
# The output binaries are byte-for-byte reproducible regardless of where the
# checkout or the cargo registry live, because the only machine-specific
# paths that would otherwise leak into the artifact (the registry path and
# the workspace path) are remapped to fixed relative prefixes. The musl
# targets are linked with rust-lld so the build works on hosts that do not
# have a native musl gcc cross toolchain installed.
#
# Usage: scripts/build-bundle.sh [target-triple]
#   With no argument, builds all supported targets (used by `make bundle`,
#   where a local dev machine typically has both musl targets installed).
#   With a target-triple argument, builds only that target (used by CI, where
#   each matrix job only has its own musl target's std installed, and cross-
#   building the other one there would produce a non-reproducible binary).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
all_targets=("x86_64-unknown-linux-musl" "aarch64-unknown-linux-musl")
if [[ $# -gt 0 ]]; then
  targets=("$1")
else
  targets=("${all_targets[@]}")
fi

export RUSTFLAGS="${RUSTFLAGS:-} \
  --remap-path-prefix=${cargo_home}/registry/src=./registry/src \
  --remap-path-prefix=${repo_root}=."

cd "$repo_root"

# Prefer rust-lld for the musl targets so the same command works on x86_64
# and aarch64 hosts, native or cross, without installing a musl gcc toolchain.
for target in "${targets[@]}"; do
  normalized="${target^^}"
  normalized="${normalized//-/_}"
  linker_var="CARGO_TARGET_${normalized}_LINKER"
  if [[ -z "${!linker_var:-}" ]]; then
    declare -x "$linker_var"="rust-lld"
  fi
done

for target in "${targets[@]}"; do
  cargo build --release --locked --target "$target"
done

bin_dir="$repo_root/omarchy/bin"
install -d "$bin_dir"
for target in "${targets[@]}"; do
  arch="${target%%-*}"
  src="$repo_root/target/$target/release/obsidian-daily-qs"
  dst="$bin_dir/obsidian-daily-qs-$arch"
  install -Dm755 "$src" "$dst"
  (
    cd "$bin_dir"
    sha256sum "obsidian-daily-qs-$arch" > "obsidian-daily-qs-$arch.sha256"
  )
done

srcid="$("$repo_root/scripts/bundle-source-id.sh")"
printf '%s  src Cargo.toml Cargo.lock rust-toolchain.toml\n' "$srcid" > "$bin_dir/obsidian-daily-qs.srcid"

echo "bundled:"
for target in "${targets[@]}"; do
  arch="${target%%-*}"
  sha256sum "$bin_dir/obsidian-daily-qs-$arch"
done
echo "source-id: $srcid"
