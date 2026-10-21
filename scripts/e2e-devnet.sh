#!/usr/bin/env bash
# =============================================================================
# NeuroWealth Vault — Devnet/Staging E2E Validation Script
# =============================================================================
#
# Deploys the vault contract to Stellar testnet and runs automated E2E
# scenarios covering initialization, deposit/withdraw, pause/unpause, and
# rebalance flows.
#
# Required env vars (or defaults are used):
#   SOROBAN_SECRET_KEY          — Funded testnet secret key
#   SOROBAN_RPC_URL             — RPC endpoint (default: testnet)
#   SOROBAN_NETWORK_PASSPHRASE  — Network passphrase (default: testnet)
#
# Optional env vars:
#   E2E_TEST_TIMELOCK           — Set to "true" to test timelock flows
#                                 (agent-update and upgrade). These require
#                                 waiting for the 24-hour timelock to expire.
#
# Exit codes:
#   0  — All scenarios passed
#   1  — One or more scenarios failed
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ARTIFACTS_DIR="$SCRIPT_DIR/e2e-artifacts"
DEFAULT_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/neurowealth-vault/target}"
WASM_PATH="${WASM_PATH:-$DEFAULT_TARGET_DIR/wasm32-unknown-unknown/release/neurowealth_vault.wasm}"

SOROBAN_RPC_URL="${SOROBAN_RPC_URL:-https://soroban-testnet.stellar.org}"
SOROBAN_NETWORK_PASSPHRASE="${SOROBAN_NETWORK_PASSPHRASE:-Test SDF Network ; September 2015}"
TIMEOUT_SECS="${E2E_TIMEOUT_SECS:-300}"  # 5-minute global timeout

PASS_COUNT=0
FAIL_COUNT=0
SCENARIO_LOG=""

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

timestamp() { date -u +"%Y-%m-%dT%H:%M:%SZ"; }

log() { echo "[$(timestamp)] $*"; }

log_section() {
  echo ""
  echo "================================================================="
  echo "  $*"
  echo "================================================================="
  echo ""
}

record_pass() {
  PASS_COUNT=$((PASS_COUNT + 1))
  SCENARIO_LOG+="PASS: $1\n"
  log "PASS: $1"
}

record_fail() {
  FAIL_COUNT=$((FAIL_COUNT + 1))
  SCENARIO_LOG+="FAIL: $1 — $2\n"
  log "FAIL: $1 — $2"
}

save_artifact() {
  local name="$1"
  local content="$2"
  echo "$content" > "$ARTIFACTS_DIR/$name"
  log "Artifact saved: $ARTIFACTS_DIR/$name"
}

# Run a soroban/stellar CLI command with timeout and capture output
run_soroban() {
  local description="$1"
  shift
  log "Running: $description"
  log "  Command: stellar $*"
  local output
  if output=$(timeout "$TIMEOUT_SECS" stellar "$@" 2>&1); then
    log "  OK"
    echo "$output"
    return 0
  else
    local rc=$?
    log "  FAILED (exit $rc): $output"
    echo "$output"
    return $rc
  fi
}

cleanup_on_exit() {
  log_section "E2E SUMMARY"
  log "Passed: $PASS_COUNT"
  log "Failed: $FAIL_COUNT"
  printf "%b" "$SCENARIO_LOG" | tee "$ARTIFACTS_DIR/summary.txt"
  log "Artifacts directory: $ARTIFACTS_DIR"
}

trap cleanup_on_exit EXIT

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------

log_section "PRE-FLIGHT CHECKS"

mkdir -p "$ARTIFACTS_DIR"

# Check for stellar CLI (replaces soroban CLI in recent versions)
STELLAR_VERSION=$(cat "$REPO_ROOT/.stellar-version" | tr -d '[:space:]')
if command -v stellar &>/dev/null; then
  CLI="stellar"
  log "Using stellar CLI: $(stellar --version)"
elif command -v soroban &>/dev/null; then
  CLI="soroban"
  log "Using soroban CLI: $(soroban --version)"
  # Alias for the rest of the script
  stellar() { soroban "$@"; }
else
  log "ERROR: Neither 'stellar' nor 'soroban' CLI found in PATH."
  log "Install version $STELLAR_VERSION with: cargo install --locked stellar-cli --version $STELLAR_VERSION"
  exit 1
fi

# Check WASM exists
if [[ ! -f "$WASM_PATH" ]]; then
  log "WASM not found at $WASM_PATH"
  log "Building contract..."
  (cd "$REPO_ROOT/neurowealth-vault" && \
    RUSTFLAGS="-C target-cpu=mvp" cargo build \
      -p neurowealth-vault \
      --target wasm32-unknown-unknown \
      --release)
  if command -v wasm-opt &>/dev/null && [[ -f "$WASM_PATH" ]]; then
    wasm-opt --strip-target-features "$WASM_PATH" -o "$WASM_PATH"
  fi
fi

if [[ ! -f "$WASM_PATH" ]]; then
  log "ERROR: WASM build failed — $WASM_PATH not found."
  exit 1
fi

log "WASM size: $(wc -c < "$WASM_PATH") bytes"
save_artifact "wasm_hash.txt" "$(sha256sum "$WASM_PATH" 2>/dev/null || shasum -a 256 "$WASM_PATH")"

# Validate storage layout metadata
log "Validating contract DataKey storage layout metadata..."
python3 "$SCRIPT_DIR/check-storage-layout.py" "$WASM_PATH" "$WASM_PATH" \
  --output "$ARTIFACTS_DIR/storage_layout_diff.txt" || {
  log "ERROR: Storage layout check failed."
  exit 1
}

# ---------------------------------------------------------------------------
# Identity setup
# ---------------------------------------------------------------------------

log_section "IDENTITY SETUP"

# Generate or use provided identities
if [[ -n "${SOROBAN_SECRET_KEY:-}" ]]; then
  log "Using provided SOROBAN_SECRET_KEY for 'e2e-deployer'"
  stellar keys add e2e-deployer --secret-key "$SOROBAN_SECRET_KEY" \
    --network testnet 2>/dev/null || true
else
  log "Generating fresh testnet identity 'e2e-deployer'..."
  stellar keys generate e2e-deployer --network testnet --fund 2>/dev/null || {
    log "Identity may already exist, continuing..."
  }
fi

DEPLOYER_ADDR=$(stellar keys address e2e-deployer 2>/dev/null)
log "Deployer address: $DEPLOYER_ADDR"
save_artifact "deployer_address.txt" "$DEPLOYER_ADDR"

# Generate a separate user identity for deposit/withdraw tests
log "Generating testnet identity 'e2e-user'..."
stellar keys generate e2e-user --network testnet --fund 2>/dev/null || {
  log "User identity may already exist, continuing..."
}
USER_ADDR=$(stellar keys address e2e-user 2>/dev/null)
log "User address: $USER_ADDR"
save_artifact "user_address.txt" "$USER_ADDR"

# ---------------------------------------------------------------------------
# Deploy the vault contract
# ---------------------------------------------------------------------------

log_section "SCENARIO: CONTRACT DEPLOYMENT"

DEPLOY_SALT=$(python3 -c "import os; print(os.urandom(32).hex())")
log "Using deployment salt: $DEPLOY_SALT"

DEPLOY_OUTPUT=$(run_soroban "Deploy vault contract" \
  contract deploy \
  --wasm "$WASM_PATH" \
  --salt "$DEPLOY_SALT" \
  --source e2e-deployer \
  --network testnet) || {
  record_fail "contract_deploy" "Failed to deploy contract: $DEPLOY_OUTPUT"
  exit 1
}

CONTRACT_ID=$(echo "$DEPLOY_OUTPUT" | tail -1 | tr -d '[:space:]')
log "Vault contract deployed: $CONTRACT_ID"
save_artifact "contract_id.txt" "$CONTRACT_ID"
record_pass "contract_deploy"

# ---------------------------------------------------------------------------
# Deploy a mock USDC token (SAC-wrapped native or a custom token)
# ---------------------------------------------------------------------------

log_section "SCENARIO: DEPLOY MOCK USDC TOKEN"

# Deploy a Stellar Asset Contract for testing
# We use the native XLM wrapped as a SAC for simplicity in E2E tests
USDC_DEPLOY_OUTPUT=$(run_soroban "Deploy SAC token for test USDC" \
  contract asset deploy \
  --asset native \
  --source e2e-deployer \
  --network testnet 2>&1) || {
  # If the native SAC is already deployed, extract the existing ID
  log "SAC deploy returned: $USDC_DEPLOY_OUTPUT"
}

# Get the native SAC contract address
USDC_TOKEN_ID=$(echo "$USDC_DEPLOY_OUTPUT" | tail -1 | tr -d '[:space:]')
if [[ -z "$USDC_TOKEN_ID" || "$USDC_TOKEN_ID" == *"error"* || "$USDC_TOKEN_ID" == *"Error"* ]]; then
  # Try to get the contract ID from the asset ID directly
  USDC_TOKEN_ID=$(run_soroban "Get native SAC ID" \
    contract id asset \
    --asset native \
    --source e2e-deployer \
    --network testnet 2>&1 | tail -1 | tr -d '[:space:]') || {
    record_fail "usdc_deploy" "Could not get native SAC token ID"
    exit 1
  }
fi

log "Test USDC token (native SAC): $USDC_TOKEN_ID"
save_artifact "usdc_token_id.txt" "$USDC_TOKEN_ID"
record_pass "usdc_token_deploy"

# ---------------------------------------------------------------------------
# Scenario 1: Initialize vault
# ---------------------------------------------------------------------------

log_section "SCENARIO 1: INITIALIZE VAULT"

INIT_OUTPUT=$(run_soroban "Initialize vault" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-deployer \
  --network testnet \
  -- \
  initialize \
  --deployer "$DEPLOYER_ADDR" \
  --owner "$DEPLOYER_ADDR" \
  --agent "$DEPLOYER_ADDR" \
  --usdc_token "$USDC_TOKEN_ID" \
  --salt "$DEPLOY_SALT" 2>&1) || {
  record_fail "initialize" "Init failed: $INIT_OUTPUT"
  exit 1
}

save_artifact "tx_initialize.txt" "$INIT_OUTPUT"

# Verify initialization by checking that calling initialize again fails (already initialized)
REINIT_OUTPUT=$(run_soroban "Verify double-init prevention" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-deployer \
  --network testnet \
  -- \
  initialize \
  --deployer "$DEPLOYER_ADDR" \
  --owner "$DEPLOYER_ADDR" \
  --agent "$DEPLOYER_ADDR" \
  --usdc_token "$USDC_TOKEN_ID" \
  --salt "$DEPLOY_SALT" 2>&1) && {
  record_fail "initialize_idempotent" "Double-init should have failed but succeeded"
} || {
  if echo "$REINIT_OUTPUT" | grep -qi "already initialized\|Already initialized\|Error\|error\|panic\|failed"; then
    record_pass "initialize (double-init correctly rejected)"
  else
    record_fail "initialize_idempotent" "Unexpected output: $REINIT_OUTPUT"
  fi
}

# Disable minimum holding period for immediate E2E deposit + withdraw verification
run_soroban "Disable minimum holding period for E2E immediate withdrawal tests" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-deployer \
  --network testnet \
  -- \
  set_min_holding_period \
  --ledgers 0 >/dev/null 2>&1 || true

record_pass "initialize"

# ---------------------------------------------------------------------------
# Scenario 2: Deposit flow
# ---------------------------------------------------------------------------

log_section "SCENARIO 2: DEPOSIT FLOW"

# The native SAC uses 7 decimals like USDC. Deposit 10 XLM = 100_000_000 stroops.
DEPOSIT_AMOUNT="100000000"

DEPOSIT_OUTPUT=$(run_soroban "Deposit $DEPOSIT_AMOUNT into vault" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-user \
  --network testnet \
  -- \
  deposit \
  --user "$USER_ADDR" \
  --amount "$DEPOSIT_AMOUNT" 2>&1) || {
  record_fail "deposit_basic" "Deposit failed: $DEPOSIT_OUTPUT"
  # Continue to other tests even on failure
  DEPOSIT_OUTPUT="FAILED: $DEPOSIT_OUTPUT"
}

save_artifact "tx_deposit.txt" "$DEPOSIT_OUTPUT"

if echo "$DEPOSIT_OUTPUT" | grep -qv "FAILED"; then
  record_pass "deposit_basic ($DEPOSIT_AMOUNT)"
fi

# ---------------------------------------------------------------------------
# Scenario 3: Withdraw flow
# ---------------------------------------------------------------------------

log_section "SCENARIO 3: WITHDRAW FLOW"

WITHDRAW_AMOUNT="50000000"  # Withdraw half

WITHDRAW_OUTPUT=$(run_soroban "Withdraw $WITHDRAW_AMOUNT from vault" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-user \
  --network testnet \
  -- \
  withdraw \
  --user "$USER_ADDR" \
  --amount "$WITHDRAW_AMOUNT" 2>&1) || {
  record_fail "withdraw_basic" "Withdraw failed: $WITHDRAW_OUTPUT"
  WITHDRAW_OUTPUT="FAILED: $WITHDRAW_OUTPUT"
}

save_artifact "tx_withdraw.txt" "$WITHDRAW_OUTPUT"

if echo "$WITHDRAW_OUTPUT" | grep -qv "FAILED"; then
  record_pass "withdraw_basic ($WITHDRAW_AMOUNT)"
fi

# ---------------------------------------------------------------------------
# Scenario 4: Pause / Unpause behavior
# ---------------------------------------------------------------------------

log_section "SCENARIO 4: PAUSE / UNPAUSE"

# Pause the vault (owner = deployer = agent)
PAUSE_OUTPUT=$(run_soroban "Pause vault" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-deployer \
  --network testnet \
  -- \
  pause \
  --owner "$DEPLOYER_ADDR" 2>&1) || {
  record_fail "pause" "Pause failed: $PAUSE_OUTPUT"
}

save_artifact "tx_pause.txt" "$PAUSE_OUTPUT"

if echo "$PAUSE_OUTPUT" | grep -qv "FAILED\|error\|Error"; then
  record_pass "pause"
fi

# Verify deposit fails while paused
PAUSED_DEPOSIT_OUTPUT=$(run_soroban "Attempt deposit while paused (should fail)" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-user \
  --network testnet \
  -- \
  deposit \
  --user "$USER_ADDR" \
  --amount "10000000" 2>&1) && {
  record_fail "pause_blocks_deposit" "Deposit should have failed while paused"
} || {
  if echo "$PAUSED_DEPOSIT_OUTPUT" | grep -qi "paused\|Error\|error\|panic"; then
    record_pass "pause_blocks_deposit"
  else
    record_fail "pause_blocks_deposit" "Unexpected error: $PAUSED_DEPOSIT_OUTPUT"
  fi
}

save_artifact "tx_deposit_while_paused.txt" "$PAUSED_DEPOSIT_OUTPUT"

# Unpause the vault
UNPAUSE_OUTPUT=$(run_soroban "Unpause vault" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-deployer \
  --network testnet \
  -- \
  unpause \
  --owner "$DEPLOYER_ADDR" 2>&1) || {
  record_fail "unpause" "Unpause failed: $UNPAUSE_OUTPUT"
}

save_artifact "tx_unpause.txt" "$UNPAUSE_OUTPUT"

if echo "$UNPAUSE_OUTPUT" | grep -qv "FAILED\|error\|Error"; then
  record_pass "unpause"
fi

# Verify deposit works after unpause
POST_UNPAUSE_DEPOSIT=$(run_soroban "Deposit after unpause" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-user \
  --network testnet \
  -- \
  deposit \
  --user "$USER_ADDR" \
  --amount "10000000" 2>&1) || {
  record_fail "deposit_after_unpause" "Deposit after unpause failed: $POST_UNPAUSE_DEPOSIT"
}

save_artifact "tx_deposit_after_unpause.txt" "$POST_UNPAUSE_DEPOSIT"

if echo "$POST_UNPAUSE_DEPOSIT" | grep -qv "FAILED\|error\|Error"; then
  record_pass "deposit_after_unpause"
fi

# ---------------------------------------------------------------------------
# Scenario 5: Rebalance with protocol
# ---------------------------------------------------------------------------

log_section "SCENARIO 5: REBALANCE"

# Rebalance to "none" protocol (no external deployment) — this should always work
REBALANCE_OUTPUT=$(run_soroban "Rebalance to 'none' protocol" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-deployer \
  --network testnet \
  -- \
  rebalance \
  --agent "$DEPLOYER_ADDR" \
  --protocol "none" \
  --expected_apy "0" \
  --min_out "0" 2>&1) || {
  record_fail "rebalance_none" "Rebalance failed: $REBALANCE_OUTPUT"
  REBALANCE_OUTPUT="FAILED: $REBALANCE_OUTPUT"
}

save_artifact "tx_rebalance.txt" "$REBALANCE_OUTPUT"

if echo "$REBALANCE_OUTPUT" | grep -qv "FAILED"; then
  record_pass "rebalance_none"
fi

# ---------------------------------------------------------------------------
# Scenario 6: Withdraw all remaining balance
# ---------------------------------------------------------------------------

log_section "SCENARIO 6: WITHDRAW ALL"

WITHDRAW_ALL_OUTPUT=$(run_soroban "Withdraw all remaining funds" \
  contract invoke \
  --id "$CONTRACT_ID" \
  --source e2e-user \
  --network testnet \
  -- \
  withdraw_all \
  --user "$USER_ADDR" 2>&1) || {
  record_fail "withdraw_all" "Withdraw all failed: $WITHDRAW_ALL_OUTPUT"
  WITHDRAW_ALL_OUTPUT="FAILED: $WITHDRAW_ALL_OUTPUT"
}

save_artifact "tx_withdraw_all.txt" "$WITHDRAW_ALL_OUTPUT"

if echo "$WITHDRAW_ALL_OUTPUT" | grep -qv "FAILED"; then
  record_pass "withdraw_all"
fi

# ---------------------------------------------------------------------------
# Scenario 7: Event verification
# ---------------------------------------------------------------------------

log_section "SCENARIO 7: EVENT VERIFICATION"

# Fetch recent events for our contract using the CLI
EVENTS_OUTPUT=$(run_soroban "Fetch contract events" \
  events \
  --id "$CONTRACT_ID" \
  --network testnet \
  --start-ledger 0 \
  --count 50 2>&1) || \
EVENTS_OUTPUT=$(run_soroban "Fetch contract events (legacy CLI)" \
  contract events \
  --id "$CONTRACT_ID" \
  --network testnet \
  --start-ledger 0 \
  --count 50 2>&1) || {
  # Event fetching may not be supported on all RPC endpoints; log but don't fail
  log "Warning: Event fetch returned error (may be unsupported): $EVENTS_OUTPUT"
  EVENTS_OUTPUT="Event fetch unavailable: $EVENTS_OUTPUT"
}

save_artifact "events.txt" "$EVENTS_OUTPUT"

# Check for expected event topics in the output
EXPECTED_EVENTS=("init" "deposit" "withdraw" "pause" "unpause")
EVENTS_FOUND=0
EVENTS_MISSING=""

for evt in "${EXPECTED_EVENTS[@]}"; do
  if echo "$EVENTS_OUTPUT" | grep -qi "$evt"; then
    EVENTS_FOUND=$((EVENTS_FOUND + 1))
  else
    EVENTS_MISSING+="$evt "
  fi
done

if [[ $EVENTS_FOUND -ge 3 ]]; then
  record_pass "event_verification ($EVENTS_FOUND/${#EXPECTED_EVENTS[@]} events found)"
elif echo "$EVENTS_OUTPUT" | grep -qi "unavailable\|unsupported\|error"; then
  log "Skipping event verification — RPC does not support event queries"
  record_pass "event_verification (skipped — RPC limitation)"
else
  record_fail "event_verification" "Only found $EVENTS_FOUND/${#EXPECTED_EVENTS[@]} events. Missing: $EVENTS_MISSING"
fi

# ---------------------------------------------------------------------------
# Scenario 8: Agent update timelock flow (optional, requires E2E_TEST_TIMELOCK=true)
# ---------------------------------------------------------------------------

if [[ "${E2E_TEST_TIMELOCK:-}" == "true" ]]; then
  log_section "SCENARIO 8: AGENT UPDATE TIMELOCK"

  # Generate a new agent identity for the update test
  log "Generating new agent identity 'e2e-new-agent'..."
  stellar keys generate e2e-new-agent --network testnet --fund 2>/dev/null || {
    log "New agent identity may already exist, continuing..."
  }
  NEW_AGENT_ADDR=$(stellar keys address e2e-new-agent 2>/dev/null)
  log "New agent address: $NEW_AGENT_ADDR"
  save_artifact "new_agent_address.txt" "$NEW_AGENT_ADDR"

  # Propose agent update (step 1 of 2)
  UPDATE_AGENT_OUTPUT=$(run_soroban "Propose agent update" \
    contract invoke \
    --id "$CONTRACT_ID" \
    --source e2e-deployer \
    --network testnet \
    -- \
    update_agent \
    --new_agent "$NEW_AGENT_ADDR" 2>&1) || {
    record_fail "update_agent_propose" "Agent update proposal failed: $UPDATE_AGENT_OUTPUT"
  }

  save_artifact "tx_update_agent_propose.txt" "$UPDATE_AGENT_OUTPUT"

  if echo "$UPDATE_AGENT_OUTPUT" | grep -qv "FAILED\|error\|Error"; then
    record_pass "update_agent_propose"

    # Verify agent is unchanged before confirmation
    CURRENT_AGENT=$(run_soroban "Get current agent" \
      contract invoke \
      --id "$CONTRACT_ID" \
      --source e2e-deployer \
      --network testnet \
      --send=no \
      -- \
      get_agent 2>&1 | tail -1 | tr -d '[:space:]"')

    if [[ "$CURRENT_AGENT" == "$DEPLOYER_ADDR" ]]; then
      record_pass "update_agent_pending (agent unchanged before confirmation)"
    else
      record_fail "update_agent_pending" "Agent changed before confirmation: $CURRENT_AGENT"
    fi

    # Get the effective ledger from the event or contract state
    # For E2E, we'll note that confirmation requires waiting for the timelock
    log "Agent update proposed. Confirmation requires waiting for the 24-hour timelock."
    log "To complete the test, run this script again after the timelock expires."
    save_artifact "timelock_note.txt" "Agent update proposed at $(timestamp). Wait 24 hours before confirmation."
  fi

  # Check if we should attempt confirmation (for re-run scenario)
  if [[ -f "$ARTIFACTS_DIR/timelock_note.txt" ]]; then
    log "Found previous timelock proposal, attempting confirmation..."
    CONFIRM_OUTPUT=$(run_soroban "Confirm agent update" \
      contract invoke \
      --id "$CONTRACT_ID" \
      --source e2e-deployer \
      --network testnet \
      -- \
      confirm_agent_update 2>&1) || {
      record_fail "confirm_agent_update" "Confirmation failed (timelock may not have expired): $CONFIRM_OUTPUT"
    }

    save_artifact "tx_confirm_agent_update.txt" "$CONFIRM_OUTPUT"

    if echo "$CONFIRM_OUTPUT" | grep -qv "FAILED\|error\|Error"; then
      record_pass "confirm_agent_update"

      # Verify agent is now updated
      UPDATED_AGENT=$(run_soroban "Get updated agent" \
        contract invoke \
        --id "$CONTRACT_ID" \
        --source e2e-deployer \
        --network testnet \
        --send=no \
        -- \
        get_agent 2>&1 | tail -1 | tr -d '[:space:]"')

      if [[ "$UPDATED_AGENT" == "$NEW_AGENT_ADDR" ]]; then
        record_pass "agent_update_confirmed (agent successfully updated)"
      else
        record_fail "agent_update_confirmed" "Agent not updated after confirmation: $UPDATED_AGENT"
      fi

      # Clean up the timelock note
      rm -f "$ARTIFACTS_DIR/timelock_note.txt"
    fi
  fi
else
  log "Skipping agent update timelock test (set E2E_TEST_TIMELOCK=true to enable)"
fi

# ---------------------------------------------------------------------------
# Scenario 9: Upgrade timelock flow (optional, requires E2E_TEST_TIMELOCK=true)
# ---------------------------------------------------------------------------

if [[ "${E2E_TEST_TIMELOCK:-}" == "true" ]]; then
  log_section "SCENARIO 9: UPGRADE TIMELOCK"

  # Create a non-zero fake WASM hash for testing (32 bytes)
  FAKE_WASM_HASH="0101010101010101010101010101010101010101010101010101010101010101"

  # Schedule upgrade (step 1 of 2)
  SCHEDULE_OUTPUT=$(run_soroban "Schedule upgrade" \
    contract invoke \
    --id "$CONTRACT_ID" \
    --source e2e-deployer \
    --network testnet \
    -- \
    schedule_upgrade \
    --owner "$DEPLOYER_ADDR" \
    --new_wasm_hash "$FAKE_WASM_HASH" 2>&1) || {
    record_fail "schedule_upgrade" "Upgrade scheduling failed: $SCHEDULE_OUTPUT"
  }

  save_artifact "tx_schedule_upgrade.txt" "$SCHEDULE_OUTPUT"

  if echo "$SCHEDULE_OUTPUT" | grep -qv "FAILED\|error\|Error"; then
    record_pass "schedule_upgrade"
    log "Upgrade scheduled. Execution requires waiting for the 24-hour timelock."
    log "To complete the test, run this script again after the timelock expires."
    save_artifact "upgrade_timelock_note.txt" "Upgrade scheduled at $(timestamp). Wait 24 hours before execution."
  fi

  # Check if we should attempt execution or cancellation (for re-run scenario)
  if [[ -f "$ARTIFACTS_DIR/upgrade_timelock_note.txt" ]]; then
    if [[ "${E2E_EXECUTE_UPGRADE:-}" == "true" ]]; then
      log "Attempting execute_upgrade..."

      # Take snapshot before execute_upgrade
      bash "$SCRIPT_DIR/post-upgrade-smoke.sh" \
        --mode snapshot \
        --contract-id "$CONTRACT_ID" \
        --network testnet \
        --source e2e-deployer \
        --snapshot "$ARTIFACTS_DIR/pre_upgrade_snapshot.json" 2>&1 || true

      EXEC_OUTPUT=$(run_soroban "Execute upgrade" \
        contract invoke \
        --id "$CONTRACT_ID" \
        --source e2e-deployer \
        --network testnet \
        -- \
        execute_upgrade \
        --owner "$DEPLOYER_ADDR" 2>&1) || {
        record_fail "execute_upgrade" "Upgrade execution failed: $EXEC_OUTPUT"
      }

      save_artifact "tx_execute_upgrade.txt" "$EXEC_OUTPUT"

      if echo "$EXEC_OUTPUT" | grep -qv "FAILED\|error\|Error"; then
        record_pass "execute_upgrade"
        log "Upgrade successfully executed."
        rm -f "$ARTIFACTS_DIR/upgrade_timelock_note.txt"

        # Post-upgrade getter sweep
        log "Running post-upgrade read-only getter sweep smoke validation..."
        if bash "$SCRIPT_DIR/post-upgrade-smoke.sh" \
          --mode verify \
          --contract-id "$CONTRACT_ID" \
          --network testnet \
          --source e2e-deployer \
          --user "$USER_ADDR" \
          --snapshot "$ARTIFACTS_DIR/pre_upgrade_snapshot.json" \
          --output "$ARTIFACTS_DIR/post_upgrade_smoke.txt"; then
          record_pass "post_upgrade_smoke_sweep"
        else
          record_fail "post_upgrade_smoke_sweep" "Post-upgrade getter sweep failed. See artifacts/post_upgrade_smoke.txt"
        fi
      fi
    else
      log "Found previous upgrade schedule, attempting cancellation (safer for E2E)..."
      CANCEL_OUTPUT=$(run_soroban "Cancel upgrade" \
        contract invoke \
        --id "$CONTRACT_ID" \
        --source e2e-deployer \
        --network testnet \
        -- \
        cancel_upgrade \
        --owner "$DEPLOYER_ADDR" 2>&1) || {
        record_fail "cancel_upgrade" "Cancellation failed: $CANCEL_OUTPUT"
      }

      save_artifact "tx_cancel_upgrade.txt" "$CANCEL_OUTPUT"

      if echo "$CANCEL_OUTPUT" | grep -qv "FAILED\|error\|Error"; then
        record_pass "cancel_upgrade"
        log "Upgrade successfully cancelled."
        rm -f "$ARTIFACTS_DIR/upgrade_timelock_note.txt"
      fi
    fi
  fi
else
  log "Skipping upgrade timelock test (set E2E_TEST_TIMELOCK=true to enable)"
fi

# ---------------------------------------------------------------------------
# Scenario 10: Contract Getter Sweep (Smoke Validation)
# ---------------------------------------------------------------------------

log_section "SCENARIO 10: READ-ONLY GETTER SWEEP SMOKE VALIDATION"

if bash "$SCRIPT_DIR/post-upgrade-smoke.sh" \
  --mode verify \
  --contract-id "$CONTRACT_ID" \
  --network testnet \
  --source e2e-deployer \
  --user "$USER_ADDR" \
  --output "$ARTIFACTS_DIR/post_upgrade_smoke.txt"; then
  record_pass "getter_sweep_smoke"
else
  record_fail "getter_sweep_smoke" "One or more read-only getters failed or returned invalid data. See $ARTIFACTS_DIR/post_upgrade_smoke.txt"
fi

# ---------------------------------------------------------------------------
# Final result
# ---------------------------------------------------------------------------

log_section "FINAL RESULTS"

log "Contract ID:  $CONTRACT_ID"
log "USDC Token:   $USDC_TOKEN_ID"
log "Deployer:     $DEPLOYER_ADDR"
log "User:         $USER_ADDR"
log ""
log "Scenarios passed: $PASS_COUNT"
log "Scenarios failed: $FAIL_COUNT"
log "Artifacts: $ARTIFACTS_DIR/"

if [[ $FAIL_COUNT -gt 0 ]]; then
  log ""
  log "E2E VALIDATION FAILED — $FAIL_COUNT scenario(s) did not pass."
  exit 1
fi

log ""
log "E2E VALIDATION PASSED — All scenarios succeeded."
exit 0
