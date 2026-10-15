#!/usr/bin/env bash
# Reproducibility environment shared by scripts/build-bundle.sh and
# scripts/verify-bundle.sh.
#
# Byte-for-byte reproducibility depends on both scripts exporting the exact
# same flags. The remap-path-prefix entries keep the checkout path and the
# cargo registry path out of the artifact, and rust-lld lets a musl build work
# on a host that has no musl gcc cross toolchain. When the two copies drift
# (for example a new --remap-path-prefix added to only one), the release gate
# fails with a misleading "toolchain/flags drift" message for a binary that is
# actually correct, and refresh-bundle artifacts stop verifying — so the
# definition lives here once.
#
# Usage: source this file after assigning `repo_root` and `cargo_home`, then:
#   bundle_env <target-triple>...
bundle_env() {
  # Same literal layout as the original inline blocks: the backslash-newline
  # is a line continuation, so the value is "<pre> --remap... --remap...".
  export RUSTFLAGS="${RUSTFLAGS:-} \
  --remap-path-prefix=${cargo_home}/registry/src=./registry/src \
  --remap-path-prefix=${repo_root}=."

  local target normalized linker_var
  for target in "$@"; do
    normalized="${target^^}"
    normalized="${normalized//-/_}"
    linker_var="CARGO_TARGET_${normalized}_LINKER"
    # `export`, not `declare -x`: inside a function `declare` would create a
    # function-local variable and the linker would silently stay unset.
    if [[ -z "${!linker_var:-}" ]]; then
      export "$linker_var"="rust-lld"
    fi
  done
}
