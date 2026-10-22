#![cfg(test)]

use super::utils::*;
use soroban_sdk::{testutils::Events, Env, Symbol};

#[test]
fn test_blend_integration_supply_via_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup: Vault + Token + Blend Pool
    let (vault_id, agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // 1. Give vault some USDC
    let deposit_amount = 100_000_000; // 100 USDC
    token_client.mint(&vault_id, &deposit_amount);
    assert_eq!(token_client.balance(&vault_id), deposit_amount);

    // 2. Set Blend Pool address in Vault
    // Note: setup_vault_with_token_and_blend already initializes the vault,
    // but we need to set the blend pool.
    // In setup_vault_with_token_and_blend, agent is the owner.
    vault_client.set_blend_pool(&owner, &blend_pool);

    // 3. Trigger rebalance to Blend
    let protocol = Symbol::new(&env, "blend");
    vault_client.rebalance(&agent, &protocol, &850, &0_i128); // 8.5% expected APY

    // 4. Verify results
    // - Vault USDC balance should be 0 (transferred to Blend)
    // - Blend Pool USDC balance should be deposit_amount
    // - Vault CurrentProtocol should be "blend"
    assert_eq!(token_client.balance(&vault_id), 0);
    assert_eq!(token_client.balance(&blend_pool), deposit_amount);
    assert_eq!(vault_client.get_current_protocol(), protocol);

    // 5. Verify events
    let events = env.events().all();
    let blend_sup_events = find_events_by_topic(events, &env, Symbol::new(&env, "blend_sup"));
    assert_eq!(blend_sup_events.len(), 1);
}

#[test]
fn test_blend_integration_withdraw_via_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup: Vault + Token + Blend Pool
    let (vault_id, agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    vault_client.set_blend_pool(&owner, &blend_pool);

    // 1. Supply to Blend first
    let amount = 50_000_000;
    token_client.mint(&vault_id, &amount);
    vault_client.rebalance(&agent, &Symbol::new(&env, "blend"), &850, &0_i128);
    assert_eq!(token_client.balance(&blend_pool), amount);

    // 2. Withdraw from Blend by rebalancing to "none"
    vault_client.rebalance(&agent, &Symbol::new(&env, "none"), &0, &0_i128);

    // 3. Verify results
    // - Vault USDC balance should be restored
    // - Blend Pool USDC balance should be 0
    // - Vault CurrentProtocol should be "none"
    assert_eq!(token_client.balance(&vault_id), amount);
    assert_eq!(token_client.balance(&blend_pool), 0);
    assert_eq!(
        vault_client.get_current_protocol(),
        Symbol::new(&env, "none")
    );

    // 4. Verify events
    let events = env.events().all();
    let blend_wd_events = find_events_by_topic(events, &env, Symbol::new(&env, "blend_wd"));
    assert_eq!(blend_wd_events.len(), 1);
}

/// The vault measures supply outcome via USDC balance delta, not the pool's
/// reported return value. A Blend pool that lies about its return value from
/// `submit_with_allowance` cannot inflate vault accounting.
#[test]
fn test_blend_balance_delta_against_lying_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    vault_client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 50_000_000_i128;
    token_client.mint(&vault_id, &deposit_amount);

    // Configure the pool to lie: it will actually transfer `deposit_amount`
    // but report double that amount as its return value.
    let lying_reported = deposit_amount * 2;
    blend_client.set_reported_supply_amount(&lying_reported);

    vault_client.rebalance(&agent, &Symbol::new(&env, "blend"), &850, &0_i128);

    // This test funds the vault directly via `mint` (bypassing `deposit`),
    // so `TotalAssets` is never touched by anything in this flow:
    // `rebalance` only moves already-accounted funds between idle and
    // deployed, and `TotalAssets` is written only by `deposit`/`withdraw`/
    // `update_total_assets`. The locally tracked per-venue deployment figure
    // (`get_deployed_to_blend`, synced from the vault's own balance-delta
    // measurement in `supply_to_blend`) is the right place to observe that
    // the pool's lying return value was not trusted.
    let deployed_to_blend = vault_client.get_deployed_to_blend();
    assert_eq!(
        deployed_to_blend, deposit_amount,
        "vault must record actual balance delta, not the pool's reported amount"
    );

    assert_eq!(token_client.balance(&blend_pool), deposit_amount);
    assert_eq!(token_client.balance(&vault_id), 0);
}

#[test]
fn test_blend_integration_balance_read() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup
    let (vault_id, agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    vault_client.set_blend_pool(&owner, &blend_pool);

    // 1. Supply some funds
    let amount = 75_000_000;
    token_client.mint(&vault_id, &amount);
    vault_client.rebalance(&agent, &Symbol::new(&env, "blend"), &850, &0_i128);

    // 2. Check balance via the shared get_reserve + get_positions helper
    // (#986 — Blend has no plain `balance(asset, user)` function; the mock
    // mirrors the real pool's two-call shape).
    let balance = blend_pool_balance(&env, &blend_pool, &usdc_token, &vault_id);

    assert_eq!(balance, amount);
}
