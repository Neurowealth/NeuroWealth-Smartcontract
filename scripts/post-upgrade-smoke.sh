#!/usr/bin/env bash
# =============================================================================
# post-upgrade-smoke.sh — Post-Upgrade Getter Sweep Smoke Validation
# =============================================================================
#
# Sweeps all read-only getters on the NeuroWealth Vault contract after an
# `execute_upgrade` invocation. Compares values against pre-upgrade state
# (or assertions) to ensure:
#   1. Contract owner and agent are unchanged.
#   2. Contract version is incremented by exactly 1.
#   3. Total shares, total deposits, and total assets are consistent.
#   4. USDC token address is unchanged.
#   5. Pending upgrade state is cleared.
#   6. All read-only view entrypoints respond without trapping.
#
# Usage:
#   # 1. Take snapshot before upgrade:
#   ./scripts/post-upgrade-smoke.sh --mode snapshot --contract-id <ID> --snapshot <FILE>
#
#   # 2. Verify after execute_upgrade:
#   ./scripts/post-upgrade-smoke.sh --mode verify --contract-id <ID> --snapshot <FILE>
#
# Exit codes:
#   0 — All getters responded successfully and all assertions passed
#   1 — One or more getter invocations failed or state mismatched
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ARTIFACTS_DIR="$SCRIPT_DIR/e2e-artifacts"
DEFAULT_SNAPSHOT="$ARTIFACTS_DIR/pre_upgrade_snapshot.json"
DEFAULT_OUTPUT="$ARTIFACTS_DIR/post_upgrade_smoke.txt"

MODE="verify"
CONTRACT_ID=""
NETWORK="testnet"
SOURCE="e2e-deployer"
USER_ADDR=""
SNAPSHOT_FILE="$DEFAULT_SNAPSHOT"
OUTPUT_FILE="$DEFAULT_OUTPUT"
TIMEOUT_SECS=30

usage() {
  local code="${1:-0}"
  cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --mode <snapshot|verify>     Mode of operation (default: verify)
  --contract-id <ID>           Vault contract ID (default: reads from artifacts/contract_id.txt)
  --network <NET>              Stellar network (default: testnet)
  --source <IDENTITY>          Source identity for read-only invokes (default: e2e-deployer)
  --user <ADDRESS>             Optional user address for user-specific getters
  --snapshot <FILE>            Path to snapshot file (default: $DEFAULT_SNAPSHOT)
  --output <FILE>              Path to smoke sweep report output (default: $DEFAULT_OUTPUT)
  -h, --help                   Display this help message
EOF
  exit "$code"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) MODE="$2"; shift 2 ;;
    --contract-id) CONTRACT_ID="$2"; shift 2 ;;
    --network) NETWORK="$2"; shift 2 ;;
    --source) SOURCE="$2"; shift 2 ;;
    --user) USER_ADDR="$2"; shift 2 ;;
    --snapshot) SNAPSHOT_FILE="$2"; shift 2 ;;
    --output) OUTPUT_FILE="$2"; shift 2 ;;
    -h|--help) usage 0 ;;
    *) echo "Unknown option: $1" >&2; usage 1 ;;
  esac
done

mkdir -p "$ARTIFACTS_DIR"

if [[ -z "$CONTRACT_ID" ]]; then
  if [[ -f "$ARTIFACTS_DIR/contract_id.txt" ]]; then
    CONTRACT_ID=$(cat "$ARTIFACTS_DIR/contract_id.txt" | tr -d '[:space:]')
  else
    echo "❌ ERROR: --contract-id not specified and $ARTIFACTS_DIR/contract_id.txt not found." >&2
    exit 1
  fi
fi

# Detect stellar or soroban CLI
if command -v stellar &>/dev/null; then
  CLI="stellar"
elif command -v soroban &>/dev/null; then
  CLI="soroban"
else
  echo "❌ ERROR: Neither 'stellar' nor 'soroban' CLI found in PATH." >&2
  exit 1
fi

invoke_getter() {
  local getter="$1"
  shift
  local out
  if out=$(timeout "$TIMEOUT_SECS" "$CLI" contract invoke \
    --id "$CONTRACT_ID" \
    --network "$NETWORK" \
    --source "$SOURCE" \
    --send=no \
    -- "$getter" "$@" 2>&1); then
    # Extract last non-empty line
    echo "$out" | grep -v '^\s*$' | tail -1 | tr -d '[:space:]"'
    return 0
  else
    echo "ERROR: $out"
    return 1
  fi
}

log_msg() {
  echo "[$(date -u +"%Y-%m-%dT%H:%M:%SZ")] $*"
}

# =============================================================================
# MODE: SNAPSHOT
# =============================================================================
if [[ "$MODE" == "snapshot" ]]; then
  log_msg "Taking pre-upgrade snapshot for contract: $CONTRACT_ID"

  VERSION=$(invoke_getter get_version || echo "ERROR")
  OWNER=$(invoke_getter get_owner || echo "ERROR")
  AGENT=$(invoke_getter get_agent || echo "ERROR")
  USDC=$(invoke_getter get_usdc_token || echo "ERROR")
  SHARES=$(invoke_getter get_total_shares || echo "ERROR")
  ASSETS=$(invoke_getter get_total_assets || echo "ERROR")
  DEPOSITS=$(invoke_getter get_total_deposits || echo "ERROR")
  PAUSED=$(invoke_getter is_paused || echo "ERROR")
  PROTOCOL=$(invoke_getter get_current_protocol || echo "ERROR")
  TVL_CAP=$(invoke_getter get_tvl_cap || echo "ERROR")

  python3 -c "
import json
data = {
    'contract_id': '$CONTRACT_ID',
    'version': '$VERSION',
    'owner': '$OWNER',
    'agent': '$AGENT',
    'usdc_token': '$USDC',
    'total_shares': '$SHARES',
    'total_assets': '$ASSETS',
    'total_deposits': '$DEPOSITS',
    'is_paused': '$PAUSED',
    'current_protocol': '$PROTOCOL',
    'tvl_cap': '$TVL_CAP'
}
with open('$SNAPSHOT_FILE', 'w') as f:
    json.dump(data, f, indent=2)
"
  log_msg "Pre-upgrade snapshot saved to: $SNAPSHOT_FILE"
  cat "$SNAPSHOT_FILE"
  exit 0
fi

# =============================================================================
# MODE: VERIFY (POST-UPGRADE SMOKE SWEEP)
# =============================================================================
log_msg "================================================================="
log_msg "  POST-UPGRADE READ-ONLY GETTER SWEEP"
log_msg "================================================================="
log_msg "Target Contract: $CONTRACT_ID"
log_msg "Network:         $NETWORK"

FAILURES=0
MISMATCHES=()
REPORT=""

add_report() {
  REPORT+="$1\n"
  echo "$1"
}

add_report "================================================================="
add_report "  NeuroWealth Post-Upgrade Smoke Validation Report"
add_report "================================================================="
add_report "Timestamp: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
add_report "Contract ID: $CONTRACT_ID"
add_report "-----------------------------------------------------------------"

# Load snapshot if available
HAS_SNAPSHOT=0
if [[ -f "$SNAPSHOT_FILE" ]]; then
  HAS_SNAPSHOT=1
  PRE_VERSION=$(python3 -c "import json; print(json.load(open('$SNAPSHOT_FILE')).get('version', ''))")
  PRE_OWNER=$(python3 -c "import json; print(json.load(open('$SNAPSHOT_FILE')).get('owner', ''))")
  PRE_AGENT=$(python3 -c "import json; print(json.load(open('$SNAPSHOT_FILE')).get('agent', ''))")
  PRE_USDC=$(python3 -c "import json; print(json.load(open('$SNAPSHOT_FILE')).get('usdc_token', ''))")
  PRE_SHARES=$(python3 -c "import json; print(json.load(open('$SNAPSHOT_FILE')).get('total_shares', ''))")
  PRE_ASSETS=$(python3 -c "import json; print(json.load(open('$SNAPSHOT_FILE')).get('total_assets', ''))")
  PRE_DEPOSITS=$(python3 -c "import json; print(json.load(open('$SNAPSHOT_FILE')).get('total_deposits', ''))")
  add_report "Baseline: Snapshot loaded from $SNAPSHOT_FILE (Pre-upgrade version: $PRE_VERSION)"
else
  add_report "Baseline: No snapshot file found at $SNAPSHOT_FILE (Performing structural sanity sweep)"
fi
add_report "-----------------------------------------------------------------"

sweep_getter() {
  local name="$1"
  local expected="${2:-}"
  local compare_desc="${3:-}"
  shift 3 || true

  local val
  if val=$(invoke_getter "$name" "$@"); then
    if [[ "$val" == ERROR* ]]; then
      add_report "[FAIL] $name: Invocation error: $val"
      FAILURES=$((FAILURES + 1))
      MISMATCHES+=("$name (Invocation error)")
      return 1
    fi

    if [[ -n "$expected" ]]; then
      local matched=0
      local IFS='|'
      for exp_opt in $expected; do
        if [[ "$val" == "$exp_opt" ]]; then
          matched=1
          break
        fi
      done
      unset IFS

      if [[ "$matched" -eq 0 ]]; then
        add_report "[FAIL] $name: Value mismatch ($compare_desc) — Expected: '$expected', Got: '$val'"
        FAILURES=$((FAILURES + 1))
        MISMATCHES+=("$name (Expected: $expected, Got: $val)")
        return 1
      fi
      add_report "[ OK ] $name: $val (matches expected: $expected)"
    else
      add_report "[ OK ] $name: $val"
    fi
    return 0
  else
    add_report "[FAIL] $name: Command execution failed"
    FAILURES=$((FAILURES + 1))
    MISMATCHES+=("$name (Execution failed)")
    return 1
  fi
}

# 1. Admin & Version Getters
if [[ "$HAS_SNAPSHOT" -eq 1 && "$PRE_VERSION" =~ ^[0-9]+$ ]]; then
  EXPECTED_VERSION=$((PRE_VERSION + 1))
  sweep_getter "get_version" "$EXPECTED_VERSION" "Version must be bumped by +1"
else
  sweep_getter "get_version" "" ""
fi

if [[ "$HAS_SNAPSHOT" -eq 1 && -n "$PRE_OWNER" ]]; then
  sweep_getter "get_owner" "$PRE_OWNER" "Owner must remain unchanged"
else
  sweep_getter "get_owner" "" ""
fi

if [[ "$HAS_SNAPSHOT" -eq 1 && -n "$PRE_AGENT" ]]; then
  sweep_getter "get_agent" "$PRE_AGENT" "Agent must remain unchanged"
else
  sweep_getter "get_agent" "" ""
fi

if [[ "$HAS_SNAPSHOT" -eq 1 && -n "$PRE_USDC" ]]; then
  sweep_getter "get_usdc_token" "$PRE_USDC" "USDC token address must remain unchanged"
else
  sweep_getter "get_usdc_token" "" ""
fi

# 2. Financial & Accounting Getters
if [[ "$HAS_SNAPSHOT" -eq 1 && -n "$PRE_SHARES" ]]; then
  sweep_getter "get_total_shares" "$PRE_SHARES" "Total shares must remain consistent"
else
  sweep_getter "get_total_shares" "" ""
fi

if [[ "$HAS_SNAPSHOT" -eq 1 && -n "$PRE_ASSETS" ]]; then
  sweep_getter "get_total_assets" "$PRE_ASSETS" "Total assets must remain consistent"
else
  sweep_getter "get_total_assets" "" ""
fi

if [[ "$HAS_SNAPSHOT" -eq 1 && -n "$PRE_DEPOSITS" ]]; then
  sweep_getter "get_total_deposits" "$PRE_DEPOSITS" "Total deposits must remain consistent"
else
  sweep_getter "get_total_deposits" "" ""
fi

sweep_getter "get_exchange_rate" "" ""
sweep_getter "get_idle_balance" "" ""
sweep_getter "get_deployed_assets" "" ""
sweep_getter "get_asset_breakdown" "" ""

# 3. Protocol & Configuration Getters
sweep_getter "is_paused" "" ""
sweep_getter "get_current_protocol" "" ""
sweep_getter "get_tvl_cap" "" ""
sweep_getter "get_user_deposit_cap" "" ""
sweep_getter "get_min_deposit" "" ""
sweep_getter "get_max_deposit" "" ""
sweep_getter "get_approval_ttl" "" ""
sweep_getter "get_rebalance_cooldown" "" ""
sweep_getter "get_max_consecutive_failures" "" ""
sweep_getter "get_consecutive_failures" "" ""

# 4. Timelock & Pending State Getters (Must be cleared post-upgrade)
sweep_getter "get_pending_upgrade" "null|None|()" "Pending upgrade must be cleared" || true

sweep_getter "get_pending_agent_update" "" ""
sweep_getter "get_pending_owner" "" ""

# 5. User-Specific Getters (if user provided)
if [[ -n "$USER_ADDR" ]]; then
  add_report "--- User Getters ($USER_ADDR) ---"
  sweep_getter "get_shares" "" "" --user "$USER_ADDR"
  sweep_getter "get_balance" "" "" --user "$USER_ADDR"
  sweep_getter "get_user_strategy" "" "" --user "$USER_ADDR"
fi

add_report "================================================================="
add_report "Sweep completed with $FAILURES failure(s)."

# Save artifact
printf "%b" "$REPORT" > "$OUTPUT_FILE"
log_msg "Smoke sweep report saved to: $OUTPUT_FILE"

if [[ $FAILURES -gt 0 ]]; then
  echo "" >&2
  echo "❌ POST-UPGRADE SMOKE SWEEP FAILED — $FAILURES getter assertion(s) failed!" >&2
  echo "Mismatched / Errored Getters:" >&2
  for m in "${MISMATCHES[@]}"; do
    echo "  - $m" >&2
  done
  echo "" >&2
  echo "Actionable Diagnosis:" >&2
  echo "  Contract migration may have corrupted storage layout, failed to preserve state," >&2
  echo "  or broke view function return types. Check $OUTPUT_FILE for full trace." >&2
  exit 1
fi

echo ""
echo "✅ POST-UPGRADE SMOKE SWEEP PASSED — All read-only getters responded with sane, consistent values."
exit 0
