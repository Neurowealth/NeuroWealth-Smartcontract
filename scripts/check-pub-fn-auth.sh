#!/usr/bin/env bash
# check-pub-fn-auth.sh
#
# CI gate: verifies that every state-changing public function in
# contract-spec.json has a corresponding row in the SECURITY.md access-control
# table.
#
# This script is intentionally complementary to check-access-control.sh:
#   - check-access-control.sh  → verifies ACCURACY of existing rows (spec vs table)
#   - check-pub-fn-auth.sh     → verifies COVERAGE (no new function silently missing)
#
# Usage:
#   bash scripts/check-pub-fn-auth.sh [security_md_path] [spec_path]
#
# Defaults:
#   SECURITY.md        (repo root)
#   contract-spec.json (repo root)
#
# Environment variables:
#   AUTH_GATE_NA_FUNCTIONS  Colon-separated list of function names that are
#                           intentionally not classified in the access-control
#                           table (e.g., truly permissionless helpers).
#                           Example: AUTH_GATE_NA_FUNCTIONS=touch_user_ttl:get_version
#
# A function can also be marked N/A directly in contract-spec.json by adding:
#   "auth_gate_na": true
# to its entry.  Both escape hatches produce the same effect.
#
# Exit codes:
#   0  All state-changing functions are classified (or explicitly N/A).
#   1  One or more state-changing functions lack a SECURITY.md row.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

SECURITY_MD="${1:-$REPO_ROOT/SECURITY.md}"
SPEC_JSON="${2:-$REPO_ROOT/contract-spec.json}"

echo "=== PUBLIC FUNCTION AUTH GATE ==="
echo "SECURITY.md:   $SECURITY_MD"
echo "Contract spec: $SPEC_JSON"
echo ""

# Resolve Python 3 executable (cross-platform: Linux, macOS, Windows/Git Bash)
PYTHON="${PYTHON:-}"
if [ -z "$PYTHON" ]; then
  for candidate in python3 python py; do
    if command -v "$candidate" >/dev/null 2>&1 && "$candidate" -c "import sys; sys.exit(0 if sys.version_info[0] >= 3 else 1)" >/dev/null 2>&1; then
      PYTHON="$candidate"
      break
    fi
  done
  if [ -z "$PYTHON" ]; then
    echo "❌ Error: Neither python3, python, nor py found with Python 3 support" >&2
    exit 1
  fi
fi

export PYTHONIOENCODING=utf-8

# ── 1. Parse the SECURITY.md access-control table ───────────────────────────
echo "1. Parsing access control table from SECURITY.md..."

SECURITY_FUNCTIONS=$("$PYTHON" - "$SECURITY_MD" <<'PYEOF'
import re
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as f:
    content = f.read()

# Collect function names from ALL access-control tables in SECURITY.md.
# The file may have more than one such table (e.g. the Emergency Harvest
# subsection adds a second small table).  We scan every table whose header
# row contains the word "Function" in its first column.
classified = set()
in_table = False

for line in content.splitlines():
    stripped = line.strip()
    if stripped.startswith("| Function"):
        in_table = True
        continue
    if in_table:
        if stripped.startswith("|---") or stripped.startswith("| ---"):
            continue  # skip separator
        if stripped.startswith("|") and not stripped.startswith("| Function"):
            cells = [c.strip() for c in stripped.split("|")]
            cells = [c for c in cells if c]
            if cells:
                classified.add(cells[0].strip())
        else:
            in_table = False  # end of this table; keep scanning for more

import json
print(json.dumps(sorted(classified)))
PYEOF
)

SECURITY_COUNT=$(echo "$SECURITY_FUNCTIONS" | "$PYTHON" -c "import json,sys; print(len(json.load(sys.stdin)))" 2>/dev/null || echo 0)
echo "  Found ${SECURITY_COUNT} function(s) classified in SECURITY.md"

# ── 2. Parse contract-spec.json for state-changing functions ────────────────
echo ""
echo "2. Parsing contract-spec.json for state-changing functions..."

SPEC_FUNCTIONS=$("$PYTHON" - "$SPEC_JSON" "${AUTH_GATE_NA_FUNCTIONS:-}" <<'PYEOF'
import json
import sys

spec_path = sys.argv[1]
na_env     = sys.argv[2] if len(sys.argv) > 2 else ""

with open(spec_path, encoding="utf-8") as f:
    spec = json.load(f)

# Functions that are always skipped — initialize has special deployment-time
# semantics and is already excluded from the accuracy check as well.
ALWAYS_SKIP = {"initialize"}

# Build the N/A set from the environment variable (colon-separated).
na_from_env = set(filter(None, na_env.split(":")))

state_changing = []
for fn in spec.get("functions", []):
    name = fn["name"]

    if name in ALWAYS_SKIP:
        continue

    if not fn.get("state_changing", False):
        continue  # query-only — no auth classification needed

    # Inline N/A flag in the spec entry itself.
    if fn.get("auth_gate_na", False):
        continue

    # N/A override via environment variable.
    if name in na_from_env:
        continue

    state_changing.append(name)

print(json.dumps(sorted(state_changing)))
PYEOF
)

SPEC_COUNT=$(echo "$SPEC_FUNCTIONS" | "$PYTHON" -c "import json,sys; print(len(json.load(sys.stdin)))" 2>/dev/null || echo 0)
echo "  Found ${SPEC_COUNT} state-changing function(s) in contract-spec.json"

# ── 3. Diff: functions in spec that are absent from SECURITY.md ─────────────
echo ""
echo "3. Checking coverage..."

UNCLASSIFIED=$("$PYTHON" - "$SECURITY_FUNCTIONS" "$SPEC_FUNCTIONS" <<'PYEOF'
import json
import sys

security_set = set(json.loads(sys.argv[1]))
spec_list    = json.loads(sys.argv[2])

missing = [name for name in spec_list if name not in security_set]

print(json.dumps(missing))
PYEOF
)

MISSING_COUNT=$(echo "$UNCLASSIFIED" | "$PYTHON" -c "import json,sys; print(len(json.load(sys.stdin)))" 2>/dev/null || echo 0)

# ── 4. Report ────────────────────────────────────────────────────────────────
echo ""
echo "=== Summary ==="
echo "  State-changing functions in spec:  ${SPEC_COUNT}"
echo "  Functions classified in SECURITY.md: ${SECURITY_COUNT}"
echo "  Unclassified functions:             ${MISSING_COUNT}"
echo ""

if [ "$MISSING_COUNT" -gt 0 ]; then
  echo "❌ PUBLIC FUNCTION AUTH GATE FAILED"
  echo ""
  echo "   The following state-changing function(s) exist in contract-spec.json"
  echo "   but have no row in the SECURITY.md Access Control Summary table:"
  echo ""
  echo "$UNCLASSIFIED" | "$PYTHON" -c "
import json, sys
names = json.load(sys.stdin)
for name in names:
    print(f'     • {name}')
"
  echo ""
  echo "   To fix, choose ONE of the following actions for each listed function:"
  echo ""
  echo "   A) Add a row to the SECURITY.md Access Control Summary table AND"
  echo "      set the correct \"access\" field in contract-spec.json."
  echo ""
  echo "   B) If the function genuinely needs no access-control classification"
  echo "      (e.g. a permissionless helper), mark it as N/A using either:"
  echo "        - Add  \"auth_gate_na\": true  to its entry in contract-spec.json"
  echo "        - Or set  AUTH_GATE_NA_FUNCTIONS=<fn_name>  before running this script"
  echo ""
  exit 1
else
  echo "✅ PUBLIC FUNCTION AUTH GATE PASSED"
  echo "   All state-changing functions are classified in SECURITY.md."
  exit 0
fi
