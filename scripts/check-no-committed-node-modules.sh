#!/usr/bin/env bash
# =============================================================================
# check-no-committed-node-modules.sh — Guard against committed node_modules.
#
# Scans git tracked files to ensure no node_modules directories or files
# are committed to the repository.
#
# Exit codes:
#   0 — no tracked node_modules found (CI green)
#   1 — tracked node_modules detected (CI red)
# =============================================================================

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

echo "Checking for committed node_modules paths in git..."

TRACKED_NODE_MODULES=$(git ls-files | grep -E '(^|/)node_modules/' || true)

if [[ -n "$TRACKED_NODE_MODULES" ]]; then
  COUNT=$(echo "$TRACKED_NODE_MODULES" | wc -l)
  echo "──────────────────────────────────────────────────────────────────"
  echo "FAIL: $COUNT node_modules path(s) are tracked in git!"
  echo ""
  echo "Tracked node_modules paths:"
  echo "$TRACKED_NODE_MODULES" | head -n 30
  if [[ "$COUNT" -gt 30 ]]; then
    echo "... and $((COUNT - 30)) more."
  fi
  echo ""
  echo "Remediation: Run 'git rm -r --cached <path>' to untrack node_modules."
  echo "──────────────────────────────────────────────────────────────────"
  exit 1
fi

echo "OK: No node_modules paths are tracked in git."
exit 0
