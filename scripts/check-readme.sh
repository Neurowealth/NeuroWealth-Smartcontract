#!/usr/bin/env bash
# =============================================================================
# check-readme.sh — Validate README code fences, key links, and command
# snippets for drift.
#
# Checks performed:
#   1. No unclosed/unmatched code fences (``` without a matching closing ```).
#   2. All relative Markdown links in README.md resolve to existing files.
#   3. Required command snippets are present (cargo test, cargo build, etc.).
#
# Exit codes:
#   0 — all checks passed (CI green)
#   1 — one or more checks failed (CI red)
# =============================================================================

set -euo pipefail

REPO_ROOT="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
README="$REPO_ROOT/README.md"

if [[ ! -f "$README" ]]; then
  echo "ERROR: README.md not found at $README" >&2
  exit 1
fi

ERRORS=0

# ---------------------------------------------------------------------------
# 1. Balanced code fences
# ---------------------------------------------------------------------------
echo "=== Check 1: balanced code fences ==="
fence_count=$(grep -c '^\s*```' "$README" || true)
if (( fence_count % 2 != 0 )); then
  echo "FAIL: odd number of \`\`\` fences ($fence_count) — a fence is unclosed."
  ERRORS=$((ERRORS + 1))
else
  echo "OK: $fence_count fence markers (${fence_count}/2 pairs)."
fi

# ---------------------------------------------------------------------------
# 2. Relative Markdown links resolve
# ---------------------------------------------------------------------------
echo ""
echo "=== Check 2: relative Markdown links ==="
broken=0

# Extract relative links: [text](path) — skip http/https and anchors
while IFS= read -r link; do
  # Strip leading ./ if present
  clean="${link#./}"
  # Resolve relative to REPO_ROOT
  target="$REPO_ROOT/$clean"
  if [[ ! -e "$target" ]]; then
    echo "FAIL: broken link -> $link"
    broken=$((broken + 1))
  fi
done < <(grep -oP '\[.+?\]\(\K[^)]+' "$README" | grep -v '^https\?://' | grep -v '^#' || true)

if [[ $broken -gt 0 ]]; then
  echo "FAIL: $broken broken link(s) found in README.md."
  ERRORS=$((ERRORS + 1))
else
  echo "OK: all relative links resolve."
fi

# ---------------------------------------------------------------------------
# 3. Required command snippets present
# ---------------------------------------------------------------------------
echo ""
echo "=== Check 3: required command snippets ==="
declare -A REQUIRED_SNIPPETS=(
  ["cargo test"]="cargo test"
  ["cargo build / stellar contract build"]="stellar contract build|cargo build"
  ["rustup target add wasm32"]="rustup target add wasm32"
)

missing=0
for label in "${!REQUIRED_SNIPPETS[@]}"; do
  pattern="${REQUIRED_SNIPPETS[$label]}"
  if ! grep -qE "$pattern" "$README"; then
    echo "FAIL: required snippet not found: $label"
    missing=$((missing + 1))
  fi
done

if [[ $missing -gt 0 ]]; then
  echo "FAIL: $missing required snippet(s) missing."
  ERRORS=$((ERRORS + 1))
else
  echo "OK: all required snippets present."
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
if [[ $ERRORS -gt 0 ]]; then
  echo "README validation FAILED ($ERRORS check(s) failed)."
  exit 1
fi
echo "README validation PASSED — all checks OK."
