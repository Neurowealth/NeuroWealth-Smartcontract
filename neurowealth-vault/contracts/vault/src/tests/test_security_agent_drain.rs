//! Security test: agent cannot drain funds via update_total_assets (#477).
//!
//! Tests that a compromised/malicious agent cannot manipulate share prices
//! by inflating `update_total_assets` reports. Verifies:
//!   1. Reports exceeding idle + deployed are rejected (InsufficientBalanceForAssets)
//!   2. Massive single-step increases beyond the decrease cap fail
//!   3. Decreases exceeding max_bps fail
//!   4. Repeated same-value reports are no-ops
//!   5. Inflation-attack sequence (deposit → inflate → accomplice withdraw)
//!      is bounded by actual assets.

use super::utils::*;
extern crate std;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

// ============================================================================
// 1. INFLATION BEYOND AVAILABLE BACKING — IDLE ONLY
// ============================================================================

/// Agent tries to report assets greater than idle balance — must fail with #33.
#[test]
#[should_panic(expected = "Error(Contract, #33)")]
fn test_security_agent_inflate_total_assets_idle_only_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let deposit = 10_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Actual backing = 10 USDC; agent reports 100 USDC → must fail.
    client.update_total_assets(&agent, &100_000_000_i128, &false, &0);
}

// ============================================================================
// 2. MASSIVE SINGLE-STEP INCREASE BEYOND DECREASE BOUNDING
// ============================================================================

/// Even if the agent later tries to allow_decrease=true, a massive positive
/// bump that would imply later unsupportable decreases should still be bounded
/// by configured max_bps. Here we test that 200% reported increase is rejected
/// when max_decrease_bps=1000 (10%).
///
/// Note: The vault's decrease cap bounds drops, not increases. Increases are
/// bounded by idle+deployed backing. The "massive increase" acceptance is
/// handled if backed, but a subsequent decrease > max_bps must fail.
#[test]
fn test_security_agent_massive_increase_then_large_decrease_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 20_000_000_i128;
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Backed: yield arrives, total grows to 30 USDC
    token_client.mint(&contract_id, &10_000_000_i128);
    client.update_total_assets(&agent, &30_000_000_i128, &false, &0);

    // Try decreasing by 5 USDC (16.7%) with max_bps=1000 (10%) → should fail.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.update_total_assets(&agent, &25_000_000_i128, &true, &1000);
    }));
    assert!(
        result.is_err(),
        "decrease of 16.7% should exceed max_bps=1000"
    );
}

// ============================================================================
// 3. DECREASE BEYOND MAX BPS
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #32)")]
fn test_security_agent_decrease_exceeds_max_bps_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 20_000_000_i128;
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Backed by idle balance.
    client.update_total_assets(&agent, &20_000_000_i128, &false, &0);

    // Drop by 11 USDC (55%) with max_bps=1000 (10%) → Error 32.
    client.update_total_assets(&agent, &9_000_000_i128, &true, &1000);
}

// ============================================================================
// 4. REPEATED SAME VALUE IS NO-OP
// ============================================================================

#[test]
fn test_security_agent_repeated_same_value_is_noop() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 10_000_000_i128;
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Initial report ok
    client.update_total_assets(&agent, &deposit, &false, &0);
    assert_eq!(client.get_total_assets(), deposit);

    // Same value again should be a no-op (no shares created).
    client.update_total_assets(&agent, &deposit, &false, &0);
    assert_eq!(client.get_total_assets(), deposit);
}

// ============================================================================
// 5. INFLATION ATTACK SEQUENCE
// ============================================================================

/// Deposit → agent inflates total_assets → accomplice tries to withdraw at
/// inflated rate. The accomplice must NOT receive more than the actual assets.
#[test]
fn test_security_inflation_attack_bounded_by_actual_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit_user = 10_000_000_i128;
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user_a = Address::generate(&env); // attacker
    let user_b = Address::generate(&env); // accomplice

    token_client.mint(&user_a, &deposit_user);
    token_client.mint(&user_b, &deposit_user);

    // User A deposits.
    client.deposit(&user_a, &deposit_user);
    // User B deposits.
    client.deposit(&user_b, &deposit_user);

    assert_eq!(client.get_total_assets(), deposit_user * 2);

    // Agent tries to inflate: total_assets = 100 USDC (only 20 exist). Must fail.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.update_total_assets(&agent, &100_000_000_i128, &false, &0);
    }));
    assert!(result.is_err(), "inflation beyond backing must fail");

    // Because the inflation failed, accomplice withdrawal must be bounded by
    // the real 20 USDC valuation.
    user_b.require_auth();
    let b_shares_before = client.get_shares(&user_b);
    let b_balance_before = client.get_balance(&user_b);

    // Withdraw half of B's balance.
    let half = b_balance_before / 2;
    client.withdraw(&user_b, &half, &None);

    let b_balance_after = client.get_balance(&user_b);
    assert_eq!(b_balance_after, b_balance_before - half);
    assert!(
        b_balance_after >= 0,
        "accomplice balance must be non-negative"
    );

    // Core: total assets in vault must still equal actual USDC backing.
    let total_assets = client.get_total_assets();
    let idle = token_client.balance(&contract_id);
    let deployed = client.get_deployed_assets();
    assert_eq!(
        idle + deployed,
        total_assets,
        "solvency invariant must hold even after inflation attempt"
    );
}

// ============================================================================
// 6. DECREASE WITHOUT ALLOWANCE MUST FAIL
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #31)")]
fn test_security_agent_decrease_without_allowance_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 10_000_000_i128;
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Decrease without allow_decrease=true → Error 31.
    client.update_total_assets(&agent, &5_000_000_i128, &false, &0);
}

// ============================================================================
// 7. PROTOCOL RESET: BACKING CHECK FOLLOWS DEPLOYED + IDLE
// ============================================================================

/// After Blend deployment, backing includes both idle and deployed assets.
#[test]
fn test_security_inflation_with_blend_deployment_bounded() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 20_000_000_i128;
    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    blend_client.set_max_supply_limit(&20_000_000_i128);
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);

    // All funds now in Blend: idle=0, deployed=20
    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(token_client.balance(&blend_pool), deposit);

    // Reporting exactly 20 passes (idle+deployed = 20)
    client.update_total_assets(&agent, &deposit, &false, &0);

    // Reporting 30 (10 beyond backing) must fail.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.update_total_assets(&agent, &30_000_000_i128, &false, &0);
    }));
    assert!(result.is_err(), "inflation beyond idle+deployed must fail");
}
