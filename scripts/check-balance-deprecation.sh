#!/usr/bin/env bash
# check-balance-deprecation.sh
#
# Verifies that the deprecated DataKey::Balance(Address) variant is not
# referenced in production contract code paths. It IS expected to appear in:
#   - The DataKey enum definition itself (lib.rs)
#   - Mock token helpers (tests/utils.rs, fuzz targets) — these use a separate
#     TokenDataKey::Balance, not the vault's DataKey::Balance
#   - Test files that explicitly verify discriminant stability
#
# Usage:
#   bash scripts/check-balance-deprecation.sh [contract_src_dir]
#
# Defaults to neurowealth-vault/contracts/vault/src

# -e: exit immediately on any command failure
# -u: treat unset variables as errors
# -o pipefail: a pipeline fails if any command in it fails, not just the last
set -euo pipefail

# Contract source directory: take as $1 if given, else default to the vault's src/.
CONTRACT_SRC="${1:-neurowealth-vault/contracts/vault/src}"
# Resolve the repo root relative to this script's own location, so the
# script works regardless of the caller's current working directory.
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Running tallies used to build the final summary and exit code.
PASS=0
FAIL=0
WARN=0

# Small helpers to print a consistently-formatted result line and bump the
# corresponding counter.
pass() { echo "  ✅ PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  ❌ FAIL: $1"; FAIL=$((FAIL + 1)); }
warn() { echo "  ⚠️  WARN: $1"; WARN=$((WARN + 1)); }

echo "=== DataKey::Balance Deprecation Check ==="
echo "Contract source: $CONTRACT_SRC"
echo ""

# ── 1. Verify the enum variant exists and is documented as deprecated ────────
echo "1. Checking DataKey::Balance is declared and marked deprecated..."

# Confirm the enum variant itself is still present in lib.rs (its absence
# would mean the enum layout changed unexpectedly).
if grep -n 'Balance(Address)' "$CONTRACT_SRC/lib.rs" > /dev/null 2>&1; then
  # Look at the 5 lines immediately preceding the variant for a
  # "deprecated" doc-comment marker.
  if grep -B5 'Balance(Address)' "$CONTRACT_SRC/lib.rs" | grep -qi 'deprecated\|DEPRECATED'; then
    pass "DataKey::Balance(Address) exists and is documented as deprecated"
  else
    warn "DataKey::Balance(Address) exists but lacks a deprecation comment"
  fi
else
  fail "DataKey::Balance(Address) not found in lib.rs — enum layout may have shifted!"
fi

# ── 2. Ensure no production code reads or writes DataKey::Balance ────────────
echo ""
echo "2. Checking for production reads/writes of DataKey::Balance..."

# Search only the main contract file (not test modules, not fuzz, not utils)
PROD_HITS=0
while IFS= read -r line; do
  # Each `line` here is a "file:linenum:content" match from grep -n.
  # Exclude the enum definition itself (lines near Balance(Address))
  FILE=$(echo "$line" | cut -d: -f1)
  LINE_NUM=$(echo "$line" | cut -d: -f2)
  CONTENT=$(echo "$line" | cut -d: -f3-)

  # Skip if this is the enum definition area (within 5 lines of Balance(Address))
  # We check if the line contains just the variant definition
  if echo "$CONTENT" | grep -q '^\s*Balance(Address)'; then
    continue
  fi
  # Skip comments
  if echo "$CONTENT" | grep -q '^\s*//\|^\s*///\|^\s*\*'; then
    continue
  fi
  # Skip the doc comment mentioning Balance
  if echo "$CONTENT" | grep -qi 'Balance.*deprecated\|deprecated.*Balance'; then
    continue
  fi
  # Anything that survives the filters above is a genuine production
  # reference to the deprecated key — print it and count it as a hit.
  echo "  Found: $line"
  ((PROD_HITS++))
done < <(grep -rn 'DataKey::Balance' "$CONTRACT_SRC/lib.rs" 2>/dev/null | grep -v 'Balance(Address)' || true)

if [ "$PROD_HITS" -eq 0 ]; then
  pass "No production code paths read or write DataKey::Balance"
else
  fail "Found $PROD_HITS production reference(s) to DataKey::Balance"
fi

# ── 3. Verify all Balance references in test code use TokenDataKey, not DataKey ─
echo ""
echo "3. Checking test/fuzz code for correct Balance usage..."

TEST_FILE_HITS=0
while IFS= read -r match; do
  FILE=$(echo "$match" | cut -d: -f1)
  LINE_NUM=$(echo "$match" | cut -d: -f2)
  CONTENT=$(echo "$match" | cut -d: -f3-)

  # Skip the DataKey enum definition
  if echo "$CONTENT" | grep -q 'Balance(Address)'; then
    continue
  fi
  # Skip comments
  if echo "$CONTENT" | grep -q '^\s*//\|^\s*///\|^\s*\*'; then
    continue
  fi

  # In test/fuzz code, Balance should always be TokenDataKey::Balance, never DataKey::Balance
  if echo "$CONTENT" | grep -q 'TokenDataKey::Balance'; then
    continue  # OK — mock token
  fi
  # If it's a genuine DataKey::Balance (not the mock TokenDataKey variant),
  # flag it for manual review rather than an automatic failure — tests may
  # legitimately reference it for discriminant-stability checks.
  if echo "$CONTENT" | grep -q 'DataKey::Balance'; then
    echo "  ⚠️  $FILE:$LINE_NUM:$CONTENT"
    ((TEST_FILE_HITS++))
  fi
done < <(grep -rn 'DataKey::Balance\|TokenDataKey::Balance' \
  "$CONTRACT_SRC/tests/" \
  "$CONTRACT_SRC/../fuzz/" \
  "$CONTRACT_SRC/tests/utils.rs" \
  2>/dev/null || true)

if [ "$TEST_FILE_HITS" -eq 0 ]; then
  pass "All test/fuzz Balance references use TokenDataKey (mock), not DataKey"
else
  warn "Found $TEST_FILE_HITS test reference(s) to vault DataKey::Balance — verify these are intentional"
fi

# ── 4. Verify discriminant stability ────────────────────────────────────────
echo ""
echo "4. Checking DataKey discriminant layout..."

# Enum variant order determines discriminant values in Rust, so reordering
# Balance relative to Shares would silently change stored discriminants
# and could corrupt existing on-chain state. Check that Balance still
# comes before Shares in source order, as a proxy for "layout unchanged".
BALANCE_LINE=$(grep -n 'Balance(Address)' "$CONTRACT_SRC/lib.rs" | head -1 | cut -d: -f1 || true)
SHARES_LINE=$(grep -n 'Shares(Address)' "$CONTRACT_SRC/lib.rs" | head -1 | cut -d: -f1 || true)

if [ -n "$BALANCE_LINE" ] && [ -n "$SHARES_LINE" ]; then
  if [ "$BALANCE_LINE" -lt "$SHARES_LINE" ]; then
    pass "Balance(Address) appears before Shares(Address) — discriminant layout preserved"
  else
    warn "Shares(Address) appears before Balance(Address) — discriminant layout may have shifted"
  fi
elif [ -n "$BALANCE_LINE" ]; then
  # Shares(Address) wasn't found at all, so we can't compare order — just
  # confirm Balance itself is present.
  pass "Balance(Address) found at line $BALANCE_LINE"
else
  warn "Balance(Address) not found in lib.rs"
fi

# ── 5. Check that shares-based accounting is used everywhere ─────────────────
echo ""
echo "5. Verifying shares-based accounting (no Balance reads in getters)..."

# Count any remaining DataKey::Balance references in lib.rs, excluding
# comment lines and the enum definition itself. A nonzero count here means
# some getter/accounting path is still reading the deprecated balance key
# instead of computing balances from shares.
BALANCE_GETTER_HITS=$(grep -n 'DataKey::Balance' "$CONTRACT_SRC/lib.rs" 2>/dev/null \
  | grep -v '^\s*//' \
  | grep -v 'Balance(Address)' \
  | wc -l || true)

if [ "$BALANCE_GETTER_HITS" -eq 0 ]; then
  pass "No Balance reads found in getter functions"
else
  fail "Found $BALANCE_GETTER_HITS Balance read(s) in getter code paths"
fi

# ── Summary ─────────────────────────────────────────────────────────────────
echo ""
echo "=== Summary ==="
echo "  Passed: $PASS"
echo "  Failed: $FAIL"
echo "  Warnings: $WARN"
echo ""

# Exit non-zero (CI failure) only on hard FAILs; WARNs are surfaced but
# don't block the pipeline on their own.
if [ "$FAIL" -gt 0 ]; then
  echo "❌ DEPRECATION CHECK FAILED — see failures above"
  echo "   DataKey::Balance may be incorrectly used in production code."
  exit 1
else
  echo "✅ DEPRECATION CHECK PASSED"
  if [ "$WARN" -gt 0 ]; then
    echo "   ($WARN warning(s) — review recommended)"
  fi
  exit 0
fi