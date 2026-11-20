#!/usr/bin/env bash
# =============================================================================
# NeuroWealth Vault — Mainnet Deployment Script (Dry-Run Preview Only)
# =============================================================================
#
# Configuration validation and deployment preview for mainnet. Currently
# supports dry-run mode only (--dry-run). Production deployment is not yet
# implemented. Use this script to validate all configuration parameters before
# manual deployment via Stellar CLI.
#
# Usage:
#   ./scripts/deploy-mainnet.sh --dry-run
#
# Options:
#   --dry-run              Validate configuration and simulate deployment steps
#   --help                 Show this help message
#
# Required environment variables:
#   MAINNET_DEPLOYER_SECRET_KEY     Temporary deployer key (will be discarded)
#   MAINNET_OWNER_ADDRESS           Cold/multisig owner address
#   MAINNET_AGENT_ADDRESS           AI agent address
#   MAINNET_USDC_TOKEN_ADDRESS      Official USDC token contract
#   MAINNET_WASM_HASH               Pre-computed SHA256 hash of vault WASM
#   MAINNET_TVL_CAP                 TVL cap in base units (e.g., 100000000000)
#   MAINNET_USER_DEPOSIT_CAP        Per-user cap in base units (e.g., 5000000000)
#   MAINNET_MIN_DEPOSIT             Minimum per-tx deposit (e.g., 1000000)
#   MAINNET_MAX_DEPOSIT             Maximum per-tx deposit (e.g., 5000000000)
#   MAINNET_BLEND_POOL_ADDRESS      Blend pool contract address
#   MAINNET_DEX_POOL_ADDRESS        DEX pool contract address (optional)
#
# Optional:
#   MAINNET_REBALANCE_COOLDOWN      Cooldown in ledgers (default: 0)
#   MAINNET_APPROVAL_TTL            Token approval TTL (default: 52560)
#   MAINNET_SALT                    Deployment salt (auto-generated if omitted)
#
# Network (always mainnet):
#   SOROBAN_RPC_URL                 Override mainnet RPC URL
#
# Exit codes:
#   0  — Configuration validation passed
#   1  — Validation failed or aborted
#   2  — Invalid configuration or missing variables
#   3  — Safety check failed
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration & Initialization
# ---------------------------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CONTRACTS_DIR="$REPO_ROOT/neurowealth-vault/contracts"
WASM_PATH="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/neurowealth_vault.wasm"

# Mainnet settings (hardcoded for safety)
SOROBAN_NETWORK_PASSPHRASE="${SOROBAN_NETWORK_PASSPHRASE:-Public Global Stellar Network ; Stellar Development Foundation}"
SOROBAN_RPC_URL="${SOROBAN_RPC_URL:-https://soroban.stellar.org}"

# Default values
DRY_RUN=false
DEPLOYMENT_LOG="$SCRIPT_DIR/mainnet-deployment.log"
DEPLOYMENT_ARTIFACTS="$SCRIPT_DIR/mainnet-deployment-artifacts.json"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# ---------------------------------------------------------------------------
# Helper Functions
# ---------------------------------------------------------------------------

timestamp() { date -u +"%Y-%m-%dT%H:%M:%SZ"; }

log() { echo -e "[$(timestamp)] $*"; }

log_section() {
  echo ""
  echo -e "${BLUE}=================================================================${NC}"
  echo -e "${BLUE}  $*${NC}"
  echo -e "${BLUE}=================================================================${NC}"
  echo ""
}

success() { echo -e "${GREEN}✓ $*${NC}"; }
error() { echo -e "${RED}✗ $*${NC}"; }
warning() { echo -e "${YELLOW}⚠ $*${NC}"; }
info() { echo -e "${BLUE}ℹ $*${NC}"; }

show_help() {
  cat << EOF
NeuroWealth Vault Mainnet Deployment Configuration Validator

USAGE:
    $0 --dry-run

OPTIONS:
    --dry-run         Validate configuration and show deployment preview
    --help            Show this help message

REQUIRED ENVIRONMENT VARIABLES:
    MAINNET_DEPLOYER_SECRET_KEY      Temporary deployer key (single-use)
    MAINNET_OWNER_ADDRESS            Cold/multisig owner address
    MAINNET_AGENT_ADDRESS            AI agent address
    MAINNET_USDC_TOKEN_ADDRESS       Official mainnet USDC token
    MAINNET_WASM_HASH                SHA256 hash of vault WASM binary
    MAINNET_TVL_CAP                  Max total value locked (base units)
    MAINNET_USER_DEPOSIT_CAP         Max per-user deposit
    MAINNET_MIN_DEPOSIT              Minimum per-transaction deposit
    MAINNET_MAX_DEPOSIT              Maximum per-transaction deposit
    MAINNET_BLEND_POOL_ADDRESS       Blend protocol pool address
    MAINNET_DEX_POOL_ADDRESS         DEX pool address (optional)

OPTIONAL:
    MAINNET_REBALANCE_COOLDOWN       Rebalance cooldown ledgers (default: 0)
    MAINNET_APPROVAL_TTL             Token approval TTL (default: 52560)
    MAINNET_SALT                     Deployment salt (auto-generated)

NOTE: This script validates configuration only. For production deployment,
follow the MAINNET_DEPLOYMENT_RUNBOOK.txt manual procedures.

EXAMPLE:
    # Validate configuration
    source .env.mainnet && ./scripts/deploy-mainnet.sh --dry-run

EOF
}

redact() {
  echo "$1" | sed -E 's/(S[A-Za-z0-9]{55})/[REDACTED_SECRET]/g' | sed -E 's/(G[A-Za-z0-9]{55})/\1/g'
}

# Generate secure random salt
generate_salt() {
  openssl rand -hex 32
}

# Validate Stellar address format
validate_address() {
  local addr="$1"
  local type="$2"
  
  if [[ ! "$addr" =~ ^G[A-Z0-9]{55}$ ]]; then
    error "Invalid Stellar address for $type: $addr"
    return 1
  fi
  return 0
}

# Validate contract address format (Soroban)
validate_contract_address() {
  local addr="$1"
  local type="$2"
  
  if [[ ! "$addr" =~ ^C[A-Z0-9]{55}$ ]]; then
    error "Invalid contract address for $type: $addr"
    return 1
  fi
  return 0
}

# Validate hex string
validate_hex() {
  local value="$1"
  local type="$2"
  local expected_len="${3:-64}"
  
  if [[ ! "$value" =~ ^[0-9a-fA-F]+$ ]]; then
    error "$type must be hex: $value"
    return 1
  fi
  
  if [[ ${#value} -ne $expected_len ]]; then
    error "$type invalid length (expected $expected_len, got ${#value})"
    return 1
  fi
  
  return 0
}

# Validate numeric value
validate_number() {
  local value="$1"
  local type="$2"
  
  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    error "$type must be a number: $value"
    return 1
  fi
  return 0
}

# Check if WASM file exists and compute hash
verify_wasm_hash() {
  local wasm_path="$1"
  local expected_hash="$2"
  
  if [[ ! -f "$wasm_path" ]]; then
    error "WASM file not found: $wasm_path"
    error "Build the contract first: cd neurowealth-vault && cargo build --release --target wasm32-unknown-unknown"
    return 1
  fi
  
  local computed_hash
  computed_hash=$(sha256sum "$wasm_path" | awk '{print $1}')
  
  if [[ "$computed_hash" != "$expected_hash" ]]; then
    error "WASM hash mismatch!"
    error "  Expected: $expected_hash"
    error "  Computed: $computed_hash"
    return 1
  fi
  
  success "WASM hash verified"
  return 0
}

# Check stellar CLI
check_stellar_cli() {
  local pinned_version
  pinned_version=$(cat "$REPO_ROOT/.stellar-version" | tr -d '[:space:]')
  
  if ! command -v stellar &> /dev/null; then
    error "stellar CLI not found. Install version $pinned_version:"
    error "  cargo install --locked stellar-cli --version $pinned_version --features opt"
    return 1
  fi
  
  local installed_version
  installed_version=$(stellar --version 2>/dev/null | awk '{print $2}' || echo "unknown")
  
  if [[ "$installed_version" != "$pinned_version" ]]; then
    warning "Stellar CLI version mismatch: installed=$installed_version, pinned=$pinned_version"
  fi
  
  success "Stellar CLI available"
  return 0
}

# Validate RPC connectivity
check_rpc_connectivity() {
  local rpc_url="$1"
  
  info "Checking RPC connectivity: $rpc_url"
  
  if curl -s --max-time 5 "$rpc_url" > /dev/null 2>&1; then
    success "RPC endpoint is reachable"
    return 0
  else
    error "Cannot reach RPC endpoint: $rpc_url"
    return 1
  fi
}

# Validate key security (ensure not logged/exposed)
check_key_security() {
  local key="$1"
  local key_type="$2"
  
  if [[ -z "$key" ]]; then
    error "$key_type is empty"
    return 1
  fi
  
  if [[ ! "$key" =~ ^S[A-Za-z0-9]{55}$ ]]; then
    error "Invalid $key_type format (must start with S)"
    return 1
  fi
  
  success "$key_type format validated (won't be logged)"
  return 0
}

# Verify all configuration parameters
check_all_config() {
  log_section "CONFIGURATION VALIDATION"
  
  local failed=0
  
  # Check stellar CLI
  if ! check_stellar_cli; then failed=1; fi
  
  # Check RPC connectivity
  if ! check_rpc_connectivity "$SOROBAN_RPC_URL"; then failed=1; fi
  
  # Validate keys
  if ! check_key_security "$MAINNET_DEPLOYER_SECRET_KEY" "DEPLOYER_SECRET_KEY"; then failed=1; fi
  if ! check_key_security "$MAINNET_OWNER_ADDRESS" "OWNER_ADDRESS"; then failed=1; fi
  if ! check_key_security "$MAINNET_AGENT_ADDRESS" "AGENT_ADDRESS"; then failed=1; fi
  
  # Validate addresses
  if ! validate_address "$MAINNET_OWNER_ADDRESS" "OWNER"; then failed=1; fi
  if ! validate_address "$MAINNET_AGENT_ADDRESS" "AGENT"; then failed=1; fi
  if ! validate_contract_address "$MAINNET_USDC_TOKEN_ADDRESS" "USDC_TOKEN"; then failed=1; fi
  if ! validate_contract_address "$MAINNET_BLEND_POOL_ADDRESS" "BLEND_POOL"; then failed=1; fi
  
  # Validate hex values
  if ! validate_hex "$MAINNET_WASM_HASH" "WASM_HASH" 64; then failed=1; fi
  if [[ -n "${MAINNET_SALT:-}" ]] && ! validate_hex "$MAINNET_SALT" "SALT" 64; then failed=1; fi
  
  # Validate numeric values
  if ! validate_number "$MAINNET_TVL_CAP" "TVL_CAP"; then failed=1; fi
  if ! validate_number "$MAINNET_USER_DEPOSIT_CAP" "USER_DEPOSIT_CAP"; then failed=1; fi
  if ! validate_number "$MAINNET_MIN_DEPOSIT" "MIN_DEPOSIT"; then failed=1; fi
  if ! validate_number "$MAINNET_MAX_DEPOSIT" "MAX_DEPOSIT"; then failed=1; fi
  
  # Validate cap ordering
  if (( MAINNET_MIN_DEPOSIT > MAINNET_MAX_DEPOSIT )); then
    error "MIN_DEPOSIT > MAX_DEPOSIT (invalid range)"
    failed=1
  fi
  
  if (( MAINNET_MAX_DEPOSIT > MAINNET_USER_DEPOSIT_CAP )); then
    warning "MAX_DEPOSIT > USER_DEPOSIT_CAP (may be intentional)"
  fi
  
  if (( MAINNET_USER_DEPOSIT_CAP > MAINNET_TVL_CAP )); then
    error "USER_DEPOSIT_CAP > TVL_CAP (impossible configuration)"
    failed=1
  fi
  
  # Verify WASM hash
  if ! verify_wasm_hash "$WASM_PATH" "$MAINNET_WASM_HASH"; then failed=1; fi
  
  if [[ $failed -eq 1 ]]; then
    error "Configuration validation failed"
    return 1
  fi
  
  success "All configuration checks passed"
  return 0
}

# Display deployment summary
show_deployment_summary() {
  log_section "DEPLOYMENT SUMMARY"
  
  echo "Deployment Mode:        $([ "$DRY_RUN" = true ] && echo 'DRY-RUN (simulated)' || echo 'PRODUCTION')"
  echo "Network:                Mainnet"
  echo "RPC URL:                $SOROBAN_RPC_URL"
  echo ""
  echo "Key Configuration:"
  echo "  Owner Address:        $MAINNET_OWNER_ADDRESS"
  echo "  Agent Address:        $MAINNET_AGENT_ADDRESS"
  echo "  Deployer:             (temporary, will be discarded)"
  echo ""
  echo "USDC Configuration:"
  echo "  Token Address:        $MAINNET_USDC_TOKEN_ADDRESS"
  echo ""
  echo "Operational Caps:"
  echo "  TVL Cap:              $MAINNET_TVL_CAP base units"
  echo "  User Deposit Cap:     $MAINNET_USER_DEPOSIT_CAP base units"
  echo "  Min Deposit:          $MAINNET_MIN_DEPOSIT base units"
  echo "  Max Deposit:          $MAINNET_MAX_DEPOSIT base units"
  echo ""
  echo "Pool Integration:"
  echo "  Blend Pool:           $MAINNET_BLEND_POOL_ADDRESS"
  echo "  DEX Pool:             ${MAINNET_DEX_POOL_ADDRESS:-not configured}"
  echo ""
  echo "Optional Settings:"
  echo "  Rebalance Cooldown:   ${MAINNET_REBALANCE_COOLDOWN:-0} ledgers"
  echo "  Approval TTL:         ${MAINNET_APPROVAL_TTL:-52560} ledgers"
  echo ""
}

# Final confirmation before deployment
confirm_deployment() {
  if [[ "$DRY_RUN" == "true" ]]; then
    info "Validating configuration: simulation only, no real transactions"
    return 0
  else
    error "Non-dry-run mode is not supported. Use: ./scripts/deploy-mainnet.sh --dry-run"
    error "For manual production deployment, follow MAINNET_DEPLOYMENT_RUNBOOK.txt"
    return 1
  fi
}

# Simulate deployment
simulate_deployment() {
  log_section "DEPLOYMENT SIMULATION (DRY-RUN)"
  
  info "Step 1: Generate deployment salt"
  local salt="${MAINNET_SALT:-$(generate_salt)}"
  info "  Salt: $salt"
  
  info "Step 2: Deploy vault contract"
  info "  WASM Hash: $MAINNET_WASM_HASH"
  
  info "Step 3: Initialize vault"
  info "  Owner:   $MAINNET_OWNER_ADDRESS"
  info "  Agent:   $MAINNET_AGENT_ADDRESS"
  info "  USDC:    $MAINNET_USDC_TOKEN_ADDRESS"
  
  info "Step 4: Configure caps"
  info "  TVL Cap:       $MAINNET_TVL_CAP"
  info "  User Cap:      $MAINNET_USER_DEPOSIT_CAP"
  info "  Min Deposit:   $MAINNET_MIN_DEPOSIT"
  info "  Max Deposit:   $MAINNET_MAX_DEPOSIT"
  
  info "Step 5: Configure pools"
  info "  Blend Pool: $MAINNET_BLEND_POOL_ADDRESS"
  [[ -n "${MAINNET_DEX_POOL_ADDRESS:-}" ]] && info "  DEX Pool:   $MAINNET_DEX_POOL_ADDRESS"
  
  info "Step 6: Configure operational settings"
  info "  Rebalance Cooldown: ${MAINNET_REBALANCE_COOLDOWN:-0}"
  info "  Approval TTL:       ${MAINNET_APPROVAL_TTL:-52560}"
  
  success "Dry-run simulation complete (no transactions submitted)"
  return 0
}

# Execute production deployment
execute_deployment() {
  error "Production deployment is not yet implemented in this script"
  error "Please follow manual procedures in MAINNET_DEPLOYMENT_RUNBOOK.txt"
  return 1
}

# Save deployment artifacts
save_artifacts() {
  local vault_address="$1"
  local tx_hash="$2"
  
  cat > "$DEPLOYMENT_ARTIFACTS" << EOF
{
  "deployment_date": "$(timestamp)",
  "network": "mainnet",
  "vault_contract_id": "$vault_address",
  "deployment_tx_hash": "$tx_hash",
  "owner_address": "$MAINNET_OWNER_ADDRESS",
  "agent_address": "$MAINNET_AGENT_ADDRESS",
  "usdc_token_address": "$MAINNET_USDC_TOKEN_ADDRESS",
  "configuration": {
    "tvl_cap": "$MAINNET_TVL_CAP",
    "user_deposit_cap": "$MAINNET_USER_DEPOSIT_CAP",
    "min_deposit": "$MAINNET_MIN_DEPOSIT",
    "max_deposit": "$MAINNET_MAX_DEPOSIT",
    "blend_pool_address": "$MAINNET_BLEND_POOL_ADDRESS",
    "dex_pool_address": "${MAINNET_DEX_POOL_ADDRESS:-null}",
    "rebalance_cooldown": "${MAINNET_REBALANCE_COOLDOWN:-0}",
    "approval_ttl": "${MAINNET_APPROVAL_TTL:-52560}"
  },
  "checklist_reference": "docs/MAINNET_CHECKLIST.md"
}
EOF
  
  success "Deployment artifacts saved to: $DEPLOYMENT_ARTIFACTS"
}

# ---------------------------------------------------------------------------
# Main Execution
# ---------------------------------------------------------------------------

main() {
  # Parse arguments
  while [[ $# -gt 0 ]]; do
    case $1 in
      --dry-run)
        DRY_RUN=true
        shift
        ;;
      --help)
        show_help
        exit 0
        ;;
      *)
        error "Unknown argument: $1"
        show_help
        exit 2
        ;;
    esac
  done
  
  # Start deployment
  log_section "NEUROWEALTH MAINNET DEPLOYMENT VALIDATOR"
  
  # Require --dry-run flag
  if [[ "$DRY_RUN" != "true" ]]; then
    error "This script is configuration-validation only"
    error "Usage: ./scripts/deploy-mainnet.sh --dry-run"
    error ""
    error "For actual mainnet deployment, follow MAINNET_DEPLOYMENT_RUNBOOK.txt"
    show_help
    exit 1
  fi
  
  # Check all prerequisites
  if ! check_all_config; then
    error "Configuration validation failed"
    exit 3
  fi
  
  # Show summary
  show_deployment_summary
  
  # Confirm action
  if ! confirm_deployment; then
    exit 1
  fi
  
  # Execute simulation only
  if ! simulate_deployment; then
    exit 1
  fi
  
  success "Configuration validation complete"
  info "Configuration is ready for production deployment"
  info "Next: Follow MAINNET_DEPLOYMENT_RUNBOOK.txt for manual deployment steps"
  log "Full log saved to: $DEPLOYMENT_LOG"
  
  return 0
}

# Run with logging
main "$@" 2>&1 | tee "$DEPLOYMENT_LOG"
EXIT_CODE=${PIPESTATUS[0]}

# Redact secrets from log file
sed -i 's/S[A-Za-z0-9]\{55\}/[REDACTED_SECRET_KEY]/g' "$DEPLOYMENT_LOG"

exit $EXIT_CODE
