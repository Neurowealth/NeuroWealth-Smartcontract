#!/usr/bin/env bash
# shellcheck shell=bash
# =============================================================================
# check-no-bare-panic.sh — Block bare panic!/assert! in production contract paths.
#
# Scans the vault contract source files (excluding test modules) for bare
# panic!(), assert!(), assert_eq!(), assert_ne!(), and debug_assert!() calls.
# Use of panic_with_error! is the required pattern in all production paths;
# bare macros are forbidden because they produce opaque, untyped errors.
#
# Exit codes:
#   0 — no bare macros found (CI green)
#   1 — bare macro detected (CI red)
#
# Usage:
#   ./scripts/check-no-bare-panic.sh [src_dir]
#
# Arguments:
#   src_dir   Path to the contract src directory
#             (default: neurowealth-vault/contracts/vault/src)
#
# Allow-list:
#   Lines containing "panic_with_error!" are explicitly permitted — that is the
#   structured alternative this script is designed to encourage.
#
#   Test files (paths matching */tests/* or *test*.rs) are excluded because
#   bare assert! / assert_eq! are idiomatic in test code.
# =============================================================================

set -euo pipefail

SRC_DIR="${1:-neurowealth-vault/contracts/vault/src}"

if [[ ! -d "$SRC_DIR" ]]; then
  echo "ERROR: source directory not found: $SRC_DIR" >&2
  exit 1
fi

echo "Scanning production sources for bare panic!/assert! calls..."
echo "Source directory: $SRC_DIR"
echo ""

# Macros to flag in production code.
BARE_PATTERN='panic!\|assert!\|assert_eq!\|assert_ne!\|debug_assert!\|debug_assert_eq!\|debug_assert_ne!'

FOUND=0

while IFS= read -r file; do
  while IFS= read -r line; do
    # Strip line number prefix from grep -n output
    content="${line#*:}"
    # Skip comment lines
    if [[ "$content" =~ ^[[:space:]]*/[/*] ]]; then
      continue
    fi
    # Skip lines that already use the structured macro
    if [[ "$line" == *"panic_with_error!"* ]]; then
      continue
    fi
    echo "VIOLATION in $file:"
    echo "  $line"
    echo ""
    FOUND=1
  done < <(grep -n "$BARE_PATTERN" "$file" 2>/dev/null || true)
done < <(find "$SRC_DIR" -name "*.rs" \
  ! -path "*/tests/*"  \
  ! -name "*test*.rs"  \
  ! -name "test.rs"    \
  ! -name "*fuzz*")

if [[ "$FOUND" -eq 1 ]]; then
  echo "──────────────────────────────────────────────────────────────────"
  echo "FAIL: bare panic!/assert! macros found in production source."
  echo ""
  echo "Production code must use panic_with_error!(&env, VaultError::...)"
  echo "instead of bare panic!()/assert!(). This produces a typed on-chain"
  echo "error that integrators and indexers can decode."
  echo "──────────────────────────────────────────────────────────────────"
  exit 1
fi

echo "OK: no bare panic!/assert! macros found in production sources."
exit 0
