#!/usr/bin/env bash
# check-access-control.sh
#
# Verifies that the access control table in SECURITY.md is accurate and
# complete by cross-referencing it against the function definitions in
# contract-spec.json. Fails on any access level or coverage mismatch.
#
# Usage:
#   bash scripts/check-access-control.sh [security_md_path] [spec_path]
#
# Defaults:
#   SECURITY.md        (repo root)
#   contract-spec.json (repo root)
#
# Exit codes:
#   0  All function access levels match between contract-spec.json and SECURITY.md.
#   1  One or more mismatches or unclassified functions detected.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

SECURITY_MD="${1:-$REPO_ROOT/SECURITY.md}"
SPEC_JSON="${2:-$REPO_ROOT/contract-spec.json}"

echo "=== ACCESS CONTROL TABLE ACCURACY CHECK ==="
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

CHECK_EXIT=0
"$PYTHON" - "$SECURITY_MD" "$SPEC_JSON" <<'PYEOF' || CHECK_EXIT=$?
import io
import json
import os
import sys

# Ensure UTF-8 output on all platforms
if sys.platform == "win32":
    try:
        sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
        sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")
    except Exception:
        pass

security_md_path = sys.argv[1]
spec_json_path = sys.argv[2]

if not os.path.exists(security_md_path):
    print(f"❌ Error: SECURITY.md not found at {security_md_path}", file=sys.stderr)
    sys.exit(1)

if not os.path.exists(spec_json_path):
    print(f"❌ Error: contract-spec.json not found at {spec_json_path}", file=sys.stderr)
    sys.exit(1)

# 1. Parse the access control table from SECURITY.md
with open(security_md_path, encoding="utf-8") as f:
    sec_content = f.read()

in_table = False
security_funcs = {}
for line in sec_content.splitlines():
    stripped = line.strip()
    if stripped.startswith("| Function"):
        in_table = True
        continue
    if in_table:
        if stripped.startswith("|---") or stripped.startswith("| ---"):
            continue  # skip markdown separator
        if stripped.startswith("|") and not stripped.startswith("| Function"):
            cells = [c.strip() for c in stripped.split("|")]
            cells = [c for c in cells if c]
            if len(cells) < 5:
                continue
            func_name = cells[0].strip().replace("`", "")
            owner = "yes" if cells[1].strip() in ("yes", "✅") else "no"
            agent = "yes" if cells[2].strip() in ("yes", "✅") else "no"
            user = "yes" if cells[3].strip() in ("yes", "✅") else "no"
            anyone = cells[4].strip() if cells[4].strip() not in ("-", "no", "") else ""

            # Determine canonical access level
            if anyone in ("pending owner", "pending-owner"):
                access = "pending-owner"
            elif anyone == "anyone":
                access = "anyone"
            elif owner == "yes":
                access = "owner"
            elif agent == "yes":
                access = "agent"
            elif user == "yes":
                access = "user"
            else:
                access = "unknown"

            security_funcs[func_name] = access
        else:
            break

print(f"1. Parsed {len(security_funcs)} function(s) from SECURITY.md access control table.")

# 2. Parse contract-spec.json
with open(spec_json_path, encoding="utf-8") as f:
    spec_data = json.load(f)

SKIP_NAMES = {"initialize"}

spec_funcs = {}
for fn in spec_data.get("functions", []):
    name = fn["name"]
    if name in SKIP_NAMES:
        continue

    raw_access = fn.get("access", "unknown")
    requires_auth = fn.get("requires_auth", True)

    # Map spec access to canonical form
    if raw_access in ("owner-only", "owner"):
        access = "owner"
    elif raw_access in ("agent-only", "agent"):
        access = "agent"
    elif raw_access in ("pending-owner-only", "pending-owner"):
        access = "pending-owner"
    elif raw_access in ("user-only", "user"):
        access = "user"
    elif raw_access == "public":
        # "public" in spec: distinguish between auth-required (user) and permissionless (anyone)
        if requires_auth:
            access = "user"
        else:
            access = "anyone"
    elif raw_access in ("anyone", "permissionless"):
        access = "anyone"
    else:
        access = raw_access

    spec_funcs[name] = {
        "access": access,
        "state_changing": fn.get("state_changing", False),
        "requires_auth": requires_auth,
    }

print(f"2. Parsed {len(spec_funcs)} function(s) from contract-spec.json.")

# 3. Cross-reference tables
print("\n3. Cross-referencing tables...")

access_mismatches = []
missing_in_security = []
missing_in_spec = []
matches = []

# Check functions in SECURITY.md against contract-spec.json
for func_name, sec_access in sorted(security_funcs.items()):
    if func_name not in spec_funcs:
        missing_in_spec.append((func_name, sec_access))
    else:
        spec_access = spec_funcs[func_name]["access"]
        if spec_access != sec_access:
            access_mismatches.append((func_name, spec_access, sec_access))
        else:
            matches.append((func_name, sec_access))

# Check state-changing functions in contract-spec.json that are missing from SECURITY.md
for func_name, fn_info in sorted(spec_funcs.items()):
    if fn_info["state_changing"] and func_name not in security_funcs:
        missing_in_security.append((func_name, fn_info["access"]))

# 4. Report results
for name, access in matches:
    print(f"  ✅ PASS: {name} ({access})")

if access_mismatches:
    print(f"\n  ❌ ACCESS LEVEL MISMATCHES ({len(access_mismatches)}):")
    for name, spec_acc, sec_acc in access_mismatches:
        print(f"     • {name}: contract-spec.json='{spec_acc}' != SECURITY.md='{sec_acc}'")

if missing_in_security:
    print(f"\n  ❌ MISSING FROM SECURITY.md ({len(missing_in_security)}):")
    for name, spec_acc in missing_in_security:
        print(f"     • {name} (spec access: '{spec_acc}')")

if missing_in_spec:
    print(f"\n  ❌ MISSING FROM contract-spec.json ({len(missing_in_spec)}):")
    for name, sec_acc in missing_in_spec:
        print(f"     • {name} (SECURITY.md access: '{sec_acc}')")

total_failures = len(access_mismatches) + len(missing_in_security) + len(missing_in_spec)

print(f"\n=== Summary ===")
print(f"  Passed:   {len(matches)}")
print(f"  Failed:   {total_failures}")
print(f"  Warnings: 0")

if total_failures > 0:
    print(f"\n❌ ACCESS CONTROL CHECK FAILED: {total_failures} mismatch(es) detected.")
    print("   The access control documentation does not match contract-spec.json.")
    print("   Update the metadata in contract-spec.json or SECURITY.md to resolve.")
    sys.exit(1)
else:
    print(f"\n✅ ACCESS CONTROL CHECK PASSED: All {len(matches)} function access levels match.")
    sys.exit(0)
PYEOF

exit "$CHECK_EXIT"
