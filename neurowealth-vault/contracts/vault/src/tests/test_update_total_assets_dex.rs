//! Tests for update_total_assets() backing check with funds deployed to DEX (Issue #393).
//!
//! Prior to this fix, test coverage for the solvency/backing check when funds are deployed
//! to DEX pool was missing.
//!
//! These tests verify:
//!   1. Backing check passes when ALL funds are in DEX (idle balance = 0).
//!   2. Backing check passes when funds are PARTIALLY in DEX.
//!   3. Yield accrued inside DEX is counted towards available backing.
//!   4. The security check still rejects inflation beyond idle + deployed.
//!   5. No regression: idle-only (no DEX) path continues to work.

extern crate std;

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

// ============================================================================
// HELPERS
// ============================================================================

/// Deposit `amount`, configure the DEX pool, and rebalance everything into
/// DEX in one step. Returns the vault client, token client, and dex pool address.
fn setup_all_in_dex(
    env: &Env,
    amount: i128,
) -> (
    Address, // contract_id
    Address, // agent
    Address, // usdc_token
    Address, // dex_pool
    NeuroWealthVaultClient<'_>,
    TestTokenClient<'_>,
) {
    let (contract_id, agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(env);
    let client = NeuroWealthVaultClient::new(env, &contract_id);
    let token_client = TestTokenClient::new(env, &usdc_token);

    client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(env);
    mint_and_deposit(env, &client, &usdc_token, &user, amount);

    // Rebalance: vault idle → DEX
    client.rebalance(&agent, &symbol_short!("dex"), &700_i128, &0_i128);

    // Sanity: all USDC is now in the pool
    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(token_client.balance(&dex_pool), amount);

    (
        contract_id,
        agent,
        usdc_token,
        dex_pool,
        client,
        token_client,
    )
}

// ============================================================================
// 1. ALL FUNDS IN DEX — SAME TOTAL (no-op report)
// ============================================================================

/// When all USDC is in DEX (vault idle balance = 0) the backing check must
/// include the deployed position. Reporting the same total should succeed.
#[test]
fn test_update_total_assets_all_in_dex_same_total_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 10_000_000_i128;
    let (_, agent, _, _, client, _) = setup_all_in_dex(&env, deposit);

    // Idle = 0, deployed = 10 USDC → total_available = 10 USDC
    // Reporting the same total should pass.
    client.update_total_assets(&agent, &deposit, &false, &0);
    assert_eq!(client.get_total_assets(), deposit);
}

// ============================================================================
// 2. ALL FUNDS IN DEX — YIELD ACCRUAL
// ============================================================================

/// DEX earns yield → pool balance grows. Agent should be able to report
/// the new (higher) total including the yield without being rejected.
#[test]
fn test_update_total_assets_dex_yield_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 20_000_000_i128;
    let yield_amount = 2_000_000_i128; // 10% yield

    let (_, agent, usdc_token, dex_pool, client, token_client) = setup_all_in_dex(&env, deposit);

    // Simulate DEX accruing yield by minting directly to pool
    token_client.mint(&dex_pool, &yield_amount);

    let new_total = deposit + yield_amount;
    client.update_total_assets(&agent, &new_total, &false, &0);

    assert_eq!(client.get_total_assets(), new_total);
    assert_eq!(token_client.balance(&usdc_token), 0_i128);
    drop(token_client);
}

// ============================================================================
// 3. PARTIAL DEPLOYMENT — IDLE + DEX
// ============================================================================

/// When only part of the vault's USDC is in DEX (the rest is idle), the
/// backing check must sum both portions.
#[test]
fn test_update_total_assets_partial_dex_deployment_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    client.set_dex_pool(&owner, &dex_pool);

    // Deposit 30 USDC total
    let deposit_total = 30_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_total);

    // Limit the DEX pool to accepting only 20 USDC → 10 stays idle
    let dex_limit = 20_000_000_i128;
    dex_client.set_max_supply_limit(&dex_limit);

    client.rebalance(&agent, &symbol_short!("dex"), &700_i128, &0_i128);

    let idle = token_client.balance(&contract_id);
    let deployed = token_client.balance(&dex_pool);

    // Partial deployment: 20 in DEX, 10 idle
    assert_eq!(deployed, dex_limit, "DEX pool should have 20 USDC");
    assert_eq!(
        idle,
        deposit_total - dex_limit,
        "vault should have 10 USDC idle"
    );

    // total_available = 10 + 20 = 30 → reporting 30 must succeed
    client.update_total_assets(&agent, &deposit_total, &false, &0);
    assert_eq!(client.get_total_assets(), deposit_total);
}

// ============================================================================
// 4. PARTIAL DEPLOYMENT WITH YIELD
// ============================================================================

/// Partial deployment: 20 in DEX earns 1 USDC yield.
/// Agent reports new total (31 USDC). Available = 10 idle + 21 in DEX = 31.
#[test]
fn test_update_total_assets_partial_dex_plus_yield_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    client.set_dex_pool(&owner, &dex_pool);

    let deposit_total = 30_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_total);

    dex_client.set_max_supply_limit(&20_000_000_i128);
    client.rebalance(&agent, &symbol_short!("dex"), &700_i128, &0_i128);

    // Yield: 1 USDC minted to DEX pool
    let yield_amount = 1_000_000_i128;
    token_client.mint(&dex_pool, &yield_amount);

    // Available = 10 (idle) + 21 (DEX) = 31
    let new_total = deposit_total + yield_amount;
    client.update_total_assets(&agent, &new_total, &false, &0);
    assert_eq!(client.get_total_assets(), new_total);
}

// ============================================================================
// 5. SECURITY: REJECT INFLATION BEYOND AVAILABLE BACKING
// ============================================================================

/// The security check must still reject a report where new_total exceeds the
/// sum of idle balance + deployed DEX position.
#[test]
#[should_panic(expected = "Error(Contract, #33)")]
fn test_update_total_assets_rejects_inflation_beyond_idle_plus_dex() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 10_000_000_i128;
    let (_, agent, _, _, client, _) = setup_all_in_dex(&env, deposit);

    // Try to report 200 USDC when only 10 USDC is available (all in DEX)
    let inflated_total = 200_000_000_i128;
    client.update_total_assets(&agent, &inflated_total, &false, &0);
}

/// Partial deployment: total available = 10 idle + 20 DEX = 30.
/// Attempting to report 31 must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #33)")]
fn test_update_total_assets_rejects_inflation_beyond_partial_dex_deployment() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    client.set_dex_pool(&owner, &dex_pool);

    let deposit_total = 30_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_total);

    dex_client.set_max_supply_limit(&20_000_000_i128);
    client.rebalance(&agent, &symbol_short!("dex"), &700_i128, &0_i128);

    // Available = 10 + 20 = 30; reporting 31 must fail
    let over_total = deposit_total + 1_i128;
    client.update_total_assets(&agent, &over_total, &false, &0);

    drop(token_client);
}

// ============================================================================
// 6. REGRESSION: IDLE-ONLY (NO DEX) PATH STILL WORKS
// ============================================================================

/// No DEX configured. The original idle-only backing check must continue
/// to work for vaults that never deploy to a protocol.
#[test]
fn test_update_total_assets_idle_only_no_dex_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let deposit = 10_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Simulate yield minted directly to vault (no DEX)
    let yield_amount = 1_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);

    let new_total = deposit + yield_amount;
    client.update_total_assets(&agent, &new_total, &false, &0);
    assert_eq!(client.get_total_assets(), new_total);
}

/// Idle-only: attempting to report more than the vault holds must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #33)")]
fn test_update_total_assets_idle_only_rejects_over_balance_dex_context() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let deposit = 10_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // No yield minted; vault holds exactly 10 USDC.
    // Reporting 11 must fail.
    client.update_total_assets(&agent, &(deposit + 1_i128), &false, &0);
}

// ============================================================================
// 7. PROTOCOL RESET: AFTER REBALANCE TO NONE BACKING IS IDLE ONLY
// ============================================================================

/// After rebalancing back to "none" (all funds returned from DEX),
/// the backing check reverts to idle-only and must correctly accept/reject.
#[test]
fn test_update_total_assets_after_rebalance_to_none_uses_idle_dex_context() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 10_000_000_i128;
    let (contract_id, agent, usdc_token, _dex_pool, client, token_client) =
        setup_all_in_dex(&env, deposit);

    // Rebalance back to none (all funds return to vault)
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);

    assert_eq!(token_client.balance(&contract_id), deposit);
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));

    // Idle = deposit, deployed = 0 → reporting deposit must pass
    client.update_total_assets(&agent, &deposit, &false, &0);
    assert_eq!(client.get_total_assets(), deposit);

    drop(usdc_token);
}
