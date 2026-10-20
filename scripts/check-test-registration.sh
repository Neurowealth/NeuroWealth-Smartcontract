#!/usr/bin/env bash
# check-test-registration.sh
#
# Validates that every tests/test_*.rs file is referenced by a `mod` line
# in tests/mod.rs, so cargo test actually compiles and runs it. Prevents
# a file silently going untested after being added but never wired in
# (Issue #989).
#
# Usage:
#   bash scripts/check-test-registration.sh
#
# Exits non-zero if any test_*.rs file has NEITHER an active `mod foo;`
# line NOR a commented-out `// mod foo;` line immediately preceded by an
# explanatory comment (the repo's existing convention for a documented,
# deliberately-excluded module, e.g. a pre-existing compile failure).
# A file with no `mod` reference at all — active or documented-commented —
# is what this check exists to catch: it means the file was added and
# silently never wired in, with no record of why.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TESTS_DIR="$REPO_ROOT/neurowealth-vault/contracts/vault/src/tests"
MOD_RS="$TESTS_DIR/mod.rs"

if [ ! -f "$MOD_RS" ]; then
  echo "ERROR: mod.rs not found at $MOD_RS" >&2
  exit 1
fi

# ── 1. Collect every test_*.rs file's module name ───────────────────────────
FILE_MODULES=()
for f in "$TESTS_DIR"/test_*.rs; do
  [ -e "$f" ] || continue
  name="$(basename "$f" .rs)"
  FILE_MODULES+=("$name")
done

if [ ${#FILE_MODULES[@]} -eq 0 ]; then
  echo "ERROR: No test_*.rs files found in $TESTS_DIR — check the path." >&2
  exit 1
fi

echo "Found ${#FILE_MODULES[@]} test_*.rs files"

# ── 2. Collect registered modules: active, or documented-and-commented ─────
# An active `mod test_foo;` counts as registered outright. A commented-out
# `// mod test_foo;` counts too, but only when the immediately preceding
# non-blank line is itself a comment (the repo's convention for recording
# *why* a module is deliberately excluded) — a bare `// mod foo;` with no
# explanation above it does not count, since that's indistinguishable from
# someone forgetting to register a new file.
REGISTERED=()
DOCUMENTED_EXCLUSIONS=()
PREV_WAS_COMMENT=0
while IFS= read -r line; do
  trimmed="$(echo "$line" | sed -E 's/^[[:space:]]+//')"

  if [ -z "$trimmed" ]; then
    PREV_WAS_COMMENT=0
    continue
  fi

  case "$trimmed" in
    //*)
      mod_name=$(echo "$trimmed" | sed -nE 's/^\/\/[[:space:]]*mod[[:space:]]+(test_[A-Za-z0-9_]+);.*/\1/p')
      if [ -n "$mod_name" ] && [ "$PREV_WAS_COMMENT" -eq 1 ]; then
        REGISTERED+=("$mod_name")
        DOCUMENTED_EXCLUSIONS+=("$mod_name")
      fi
      PREV_WAS_COMMENT=1
      continue
      ;;
  esac

  mod_name=$(echo "$trimmed" | sed -nE 's/^(#\[[^]]*\][[:space:]]*)?mod[[:space:]]+(test_[A-Za-z0-9_]+);.*/\2/p')
  if [ -n "$mod_name" ]; then
    REGISTERED+=("$mod_name")
  fi
  PREV_WAS_COMMENT=0
done < "$MOD_RS"

echo "Found ${#REGISTERED[@]} registered modules in mod.rs (${#DOCUMENTED_EXCLUSIONS[@]} documented exclusions)"
echo ""

# ── 3. Compare ───────────────────────────────────────────────────────────────
MISSING=()
for f in "${FILE_MODULES[@]}"; do
  found=0
  for m in "${REGISTERED[@]}"; do
    if [ "$f" = "$m" ]; then
      found=1
      break
    fi
  done
  if [ "$found" -eq 0 ]; then
    MISSING+=("$f")
  fi
done

if [ ${#DOCUMENTED_EXCLUSIONS[@]} -gt 0 ]; then
  echo "ℹ️  Documented exclusions (commented 'mod' line with an explanation above it):"
  for d in "${DOCUMENTED_EXCLUSIONS[@]}"; do
    echo "   - $d.rs"
  done
  echo ""
fi

if [ ${#MISSING[@]} -gt 0 ]; then
  echo "❌ test_*.rs files with NO 'mod' line at all in tests/mod.rs (active or documented-commented):"
  for f in "${MISSING[@]}"; do
    echo "   - $f.rs"
  done
  echo ""
  echo "❌ TEST REGISTRATION CHECK FAILED"
  echo "   For each file above, either:"
  echo "     1. Register it: add 'mod <name>;' to tests/mod.rs, or"
  echo "     2. Deliberately exclude it: add a comment explaining why directly"
  echo "        above a commented-out '// mod <name>;' line, or"
  echo "     3. Delete the file if it is genuinely obsolete."
  exit 1
else
  echo "✅ TEST REGISTRATION CHECK PASSED — every test_*.rs file is registered or documented in mod.rs"
  exit 0
fi
