#!/usr/bin/env bash
# shellcheck shell=bash
# =============================================================================
# check-reproducible-build.sh — Assert the optimised release WASM is
# reproducible.
#
# Exports the committed source (git archive HEAD) into two independent build
# roots with deliberately different absolute paths, runs the exact CI release
# build in each (cargo build --release for wasm32-unknown-unknown followed by
# wasm-opt --strip-target-features), and asserts the two artifacts are
# byte-identical by SHA-256. Differing path lengths are chosen on purpose:
# embedded absolute paths (debug info, panic messages, macro expansions) are
# the most common source of WASM nondeterminism, and identical-length paths
# can mask them.
#
# This lets auditors and the community verify that a deployed artifact can be
# regenerated from source: two clean builds that hash-match here, combined
# with one rebuild on an independent host, demonstrate the toolchain output
# is not path- or environment-dependent.
#
# Exit codes:
#   0 — both builds produced byte-identical WASM (hashes printed)
#   1 — hash mismatch (nondeterminism) or a build failed
#
# Usage:
#   ./scripts/check-reproducible-build.sh [work_dir]
#
# Arguments:
#   work_dir  Scratch directory for the two build roots
#             (default: a fresh mktemp -d; kept on failure for inspection)
#
# Requirements: git, cargo with the wasm32-unknown-unknown target, wasm-opt,
# and sha256sum or shasum. Run from anywhere inside the repository.
# =============================================================================

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
WORK_DIR="${1:-$(mktemp -d "${TMPDIR:-/tmp}/nw-repro-XXXXXX")}"

# Deliberately different directory names (and lengths) for the two roots.
BUILD_A="$WORK_DIR/a"
BUILD_B="$WORK_DIR/build-root-b-with-a-much-longer-path-component"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

build_wasm() {
  local root="$1"
  local src="$root/src-tree"

  mkdir -p "$src"
  # Export the committed tree only: uncommitted changes and untracked files
  # must not influence a reproducibility attestation.
  git -C "$REPO_ROOT" archive HEAD | tar -x -C "$src"

  # Isolated target dir per root so no artifacts are shared between builds.
  (
    cd "$src/neurowealth-vault"
    CARGO_TARGET_DIR="$root/target" \
    RUSTFLAGS="-C target-cpu=mvp" \
      cargo build -p neurowealth-vault --target wasm32-unknown-unknown --release
  )

  local wasm="$root/target/wasm32-unknown-unknown/release/neurowealth_vault.wasm"
  wasm-opt --strip-target-features "$wasm" -o "$wasm"
  echo "$wasm"
}

echo "Reproducible-build check"
echo "  commit:   $(git -C "$REPO_ROOT" rev-parse HEAD)"
echo "  work dir: $WORK_DIR"
echo

echo "[1/2] Building in $BUILD_A ..."
WASM_A="$(build_wasm "$BUILD_A")"
HASH_A="$(sha256_of "$WASM_A")"
SIZE_A="$(wc -c < "$WASM_A" | tr -d ' ')"
echo "      sha256=$HASH_A size=${SIZE_A}B"

echo "[2/2] Building in $BUILD_B ..."
WASM_B="$(build_wasm "$BUILD_B")"
HASH_B="$(sha256_of "$WASM_B")"
SIZE_B="$(wc -c < "$WASM_B" | tr -d ' ')"
echo "      sha256=$HASH_B size=${SIZE_B}B"
echo

if [ "$HASH_A" = "$HASH_B" ]; then
  echo "PASS: builds are byte-identical (sha256 $HASH_A)"
  rm -rf "$WORK_DIR"
  exit 0
fi

echo "FAIL: WASM artifacts differ between the two build roots."
echo "  build A: $WASM_A ($SIZE_A bytes, $HASH_A)"
echo "  build B: $WASM_B ($SIZE_B bytes, $HASH_B)"
echo
echo "Both artifacts are kept in $WORK_DIR for root-cause analysis."
echo "Common nondeterminism sources to check:"
echo "  - absolute paths embedded in the binary (panic messages, debug info):"
echo "      strings on both files and diff; fix with"
echo "      RUSTFLAGS='--remap-path-prefix \$PWD=/build' or Cargo trim-paths"
echo "  - toolchain drift: confirm both builds used the same rustc/wasm-opt"
echo "      versions (rustc -V; wasm-opt --version)"
echo "  - non-pinned dependencies: confirm Cargo.lock is committed and used"
echo "Document any accepted, root-caused difference in docs/WASM_SIZE.md."
exit 1
