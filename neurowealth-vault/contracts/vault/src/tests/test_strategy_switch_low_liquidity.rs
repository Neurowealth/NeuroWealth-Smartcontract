//! Stress tests for strategy switching under low-liquidity conditions (Issue #263).
//!
//! These tests exercise edge cases in routing and withdrawal behaviour when the
//! underlying Blend or DEX pool has constrained liquidity:
//!
//! - Partial supply when Blend/DEX pool cap is below the vault balance.
//! - Partial withdrawal when the pool drains slowly.
//! - Multiple consecutive strategy switches with thin pool liquidity.
//! - Withdrawal by a user when the active protocol has limited available funds.
//! - Deterministic, reproducible outcomes on every run.

#![cfg(test)]

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol};

// ============================================================================
// Helpers
// ============================================================================

fn vault_usdc_balance(env: &Env, token: &Address, vault: &Address) -> i128 {
    TestTokenClient::new(env, token).balance(vault)
}

// ============================================================================
// 1. BLEND — partial supply under supply cap
// ============================================================================

/// Blend pool accepts only half the vault balance.
/// The vault records the partial supply and total assets remain consistent.
#[test]
fn test_blend_partial_supply_low_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit = 20_000_000_i128; // 20 USDC
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Cap the pool: it will only absorb half of what we send.
    blend_client.set_max_supply_limit(&(deposit / 2));

    // Rebalance — succeeds with partial fill (no min_out enforced).
    client.rebalance(&symbol_short!("blend"), &600_i128, &0_i128);

    let half = deposit / 2;
    // Half is in Blend, half remains idle in the vault.
    assert_eq!(
        TestTokenClient::new(&env, &usdc_token).balance(&blend_pool),
        half,
        "Blend should hold the capped half"
    );
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &vault_id),
        deposit - half,
        "Vault should retain the un-supplied half"
    );

    // Total assets must remain unchanged (no value lost or gained).
    assert_eq!(
        client.get_total_assets(),
        deposit,
        "Total assets must equal original deposit despite partial supply"
    );
}

// ============================================================================
// 2. BLEND — partial withdrawal under withdraw cap
// ============================================================================

/// Blend pool can only release half its liquidity per call.
/// The vault pulls what it can; total assets remain consistent.
#[test]
fn test_blend_partial_withdrawal_low_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit = 30_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Move all funds to Blend.
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(vault_usdc_balance(&env, &usdc_token, &vault_id), 0);

    // Now cap withdrawals to half the supplied amount.
    blend_client.set_max_withdraw_limit(&(deposit / 2));

    // Switch back to none — pool only returns half.
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    let half = deposit / 2;
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &vault_id),
        half,
        "Vault should receive only the capped withdrawal amount"
    );

    // Total assets must be consistent with what was recovered.
    assert_eq!(
        client.get_total_assets(),
        deposit,
        "Total assets should reflect the full deposit; Blend still holds remainder"
    );
}

// ============================================================================
// 3. DEX — partial supply under supply cap
// ============================================================================

/// DEX pool accepts only a fraction of the vault balance.
/// Outcome is deterministic regardless of how constrained the pool is.
#[test]
fn test_dex_partial_supply_low_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    client.set_dex_pool(&owner, &dex_pool);

    let deposit = 40_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // DEX absorbs only a quarter.
    let cap = deposit / 4;
    dex_client.set_max_supply_limit(&cap);

    client.rebalance(&Symbol::new(&env, "dex"), &900_i128, &0_i128);

    assert_eq!(
        TestTokenClient::new(&env, &usdc_token).balance(&dex_pool),
        cap,
        "DEX should hold only the capped amount"
    );
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &vault_id),
        deposit - cap,
        "Vault retains the rest"
    );
    assert_eq!(
        client.get_total_assets(),
        deposit,
        "Total assets must be unchanged"
    );
}

// ============================================================================
// 4. Multiple strategy switches with thin Blend liquidity
// ============================================================================

/// none → blend (thin) → none → blend (thin): each round-trip is
/// deterministic and total assets are consistent at every step.
#[test]
fn test_multiple_strategy_switches_thin_blend_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit = 50_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    let supply_cap = 10_000_000_i128; // only 10 USDC accepted per rebalance
    let withdraw_cap = 10_000_000_i128;

    blend_client.set_max_supply_limit(&supply_cap);
    blend_client.set_max_withdraw_limit(&withdraw_cap);

    // Switch 1: none → blend (partial fill)
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);
    assert_eq!(client.get_total_assets(), deposit, "Round 1 assets");

    // Switch 2: blend → none (partial withdrawal)
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert_eq!(client.get_total_assets(), deposit, "Round 2 assets");

    // Switch 3: none → blend again
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);
    assert_eq!(client.get_total_assets(), deposit, "Round 3 assets");

    // Switch 4: blend → none
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert_eq!(client.get_total_assets(), deposit, "Round 4 assets");

    // Protocol ends on "none" — consistent with the last rebalance.
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
}

// ============================================================================
// 5. User withdrawal when Blend has limited available funds
// ============================================================================

/// When the protocol is "blend" and the pool is almost dry, a user requesting
/// a full withdrawal should succeed if the vault can pull back sufficient funds
/// (or should return only what is available without corrupting state).
#[test]
fn test_user_withdraw_blend_low_liquidity_sufficient_idle() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    // Two users deposit; half the total goes to Blend.
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let half = 15_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user_a, half);
    mint_and_deposit(&env, &client, &usdc_token, &user_b, half);

    // Supply only half to Blend, leaving the other half idle.
    blend_client.set_max_supply_limit(&half);
    client.rebalance(&symbol_short!("blend"), &600_i128, &0_i128);

    // Blend holds `half`; vault holds the other `half` idle.
    assert_eq!(vault_usdc_balance(&env, &usdc_token, &vault_id), half);

    // user_a withdraws their share — fully covered by idle vault funds.
    client.withdraw(&user_a, &half, &None);
    assert_eq!(
        TestTokenClient::new(&env, &usdc_token).balance(&user_a),
        half,
        "user_a should receive their full balance from idle vault funds"
    );

    // Total assets drop by user_a's share; user_b's position is intact.
    assert_eq!(
        client.get_total_assets(),
        half,
        "Only user_b's share remains after user_a withdraws"
    );
}

// ============================================================================
// 6. Blend → DEX switch with thin Blend withdrawal liquidity
// ============================================================================

/// Switching from Blend to DEX when Blend can only release partial funds:
/// the vault moves whatever Blend releases, then supplies that to the DEX.
/// Total assets are preserved throughout.
#[test]
fn test_blend_to_dex_switch_thin_blend_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let blend_pool = env.register_contract(None, MockBlendPool);
    let dex_pool = env.register_contract(None, MockDexPool);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);
    client.set_dex_pool(&owner, &dex_pool);

    let deposit = 60_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Move all funds to Blend.
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);
    assert_eq!(vault_usdc_balance(&env, &usdc_token, &vault_id), 0);

    // Constrain Blend withdrawal — only 20 USDC can come out per call.
    blend_client.set_max_withdraw_limit(&20_000_000_i128);

    // Switch blend → dex; vault pulls whatever Blend allows, then supplies DEX.
    client.rebalance(&symbol_short!("dex"), &900_i128, &0_i128);

    // The vault's total tracked assets must equal the original deposit regardless
    // of how many funds each pool currently holds.
    assert_eq!(
        client.get_total_assets(),
        deposit,
        "Total assets must equal original deposit after thin-liquidity blend→dex switch"
    );

    // Protocol should now be "dex".
    assert_eq!(client.get_current_protocol(), symbol_short!("dex"));
}
