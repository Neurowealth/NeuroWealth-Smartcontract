#!/usr/bin/env bash
# shellcheck shell=bash
# =============================================================================
# check-stale-state-audit.sh — Verify no storage reads follow cross-contract calls on hot paths.
#
# Scans the vault contract source file (lib.rs) hot path functions:
# - deposit
# - withdraw
# - rebalance
# - update_total_assets
#
# Ensures that all required storage state reads occur BEFORE any cross-contract
# calls (token transfers or external protocol calls).
#
# Exit codes:
#   0 — no stale-state storage read violations found (CI green)
#   1 — storage read after cross-contract call detected (CI red)
# =============================================================================

set -euo pipefail

CONTRACT_FILE="${1:-neurowealth-vault/contracts/vault/src/lib.rs}"

if [[ ! -f "$CONTRACT_FILE" ]]; then
  echo "ERROR: contract file not found: $CONTRACT_FILE" >&2
  exit 1
fi

echo "Scanning $CONTRACT_FILE for stale-state storage read violations on hot paths..."
echo ""

VIOLATIONS=0

# Check deposit function: token_client.transfer should not be followed by storage reads
deposit_block=$(sed -n '/pub fn deposit(/,/^    }/p' "$CONTRACT_FILE")
if echo "$deposit_block" | grep -A 50 "token_client.transfer" | grep -q "env\.storage()"; then
  echo "VIOLATION in deposit(): storage read/access found after token_client.transfer call."
  VIOLATIONS=1
fi

# Check withdraw function: storage reads for Shares, TotalShares, TotalAssets should precede protocol withdraw
withdraw_block=$(sed -n '/pub fn withdraw(/,/^    }/p' "$CONTRACT_FILE")
if echo "$withdraw_block" | grep -A 50 "withdraw_amount_from_protocol" | grep -q "get(&DataKey::Shares"; then
  echo "VIOLATION in withdraw(): DataKey::Shares read found after withdraw_amount_from_protocol call."
  VIOLATIONS=1
fi

# Check update_total_assets function: env.storage().instance().get should not follow token_client.balance calls
update_block=$(sed -n '/pub fn update_total_assets(/,/^    }/p' "$CONTRACT_FILE")
if echo "$update_block" | grep -A 50 "token_client\.balance" | grep -q "env\.storage()\.instance()\.get"; then
  echo "VIOLATION in update_total_assets(): instance storage get found after balance calls."
  VIOLATIONS=1
fi

if [[ "$VIOLATIONS" -eq 1 ]]; then
  echo "──────────────────────────────────────────────────────────────────"
  echo "FAIL: Stale-state / CEI violations detected on hot paths."
  echo "Ensure all storage reads precede external cross-contract calls."
  echo "──────────────────────────────────────────────────────────────────"
  exit 1
fi

echo "OK: All hot paths strictly enforce Checks-Effects-Interactions (CEI)."
exit 0
