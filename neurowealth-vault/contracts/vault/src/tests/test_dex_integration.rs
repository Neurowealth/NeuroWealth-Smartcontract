#![cfg(test)]

//! DEX liquidity pool integration tests (Issue #228).
//!
//! Mirrors `test_blend_integration.rs` but exercises the `dex` protocol path:
//! supplying to / withdrawing from a configured DEX pool via `rebalance`,
//! `CurrentProtocol`/`ProtocolChangedEvent` tracking, `min_out` slippage
//! enforcement, protocol switching, and user withdrawals that pull from the DEX.

use super::utils::*;
use soroban_sdk::{
    symbol_short, testutils::Address as _, vec, Address, Env, IntoVal, Symbol, Val, Vec,
};

#[test]
fn test_dex_integration_supply_via_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // 1. Give the vault some USDC.
    let deposit_amount = 100_000_000; // 100 USDC
    token_client.mint(&vault_id, &deposit_amount);
    assert_eq!(token_client.balance(&vault_id), deposit_amount);

    // 2. Configure the DEX pool.
    vault_client.set_dex_pool(&owner, &dex_pool);
    assert_eq!(vault_client.get_dex_pool(), Some(dex_pool.clone()));

    // 3. Rebalance into the DEX pool.
    let protocol = Symbol::new(&env, "dex");
    vault_client.rebalance(&protocol, &850, &0_i128);

    // 4. Funds moved to the DEX, protocol tracked as "dex".
    assert_eq!(token_client.balance(&vault_id), 0);
    assert_eq!(token_client.balance(&dex_pool), deposit_amount);
    assert_eq!(vault_client.get_current_protocol(), protocol);

    // 5. Events: dex_sup and proto_chg.
    let events = env.events().all();
    let dex_sup_events = find_events_by_topic(events.clone(), &env, Symbol::new(&env, "dex_sup"));
    assert_eq!(dex_sup_events.len(), 1);
    let proto_chg_events = find_events_by_topic(events, &env, Symbol::new(&env, "proto_chg"));
    assert_eq!(proto_chg_events.len(), 1);
}

#[test]
fn test_dex_integration_withdraw_via_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    vault_client.set_dex_pool(&owner, &dex_pool);

    // 1. Supply to the DEX.
    let amount = 50_000_000;
    token_client.mint(&vault_id, &amount);
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);
    assert_eq!(token_client.balance(&dex_pool), amount);

    // 2. Exit the DEX by rebalancing to "none".
    vault_client.rebalance(&Symbol::new(&env, "none"), &0, &0_i128);

    // 3. Funds restored, protocol back to "none".
    assert_eq!(token_client.balance(&vault_id), amount);
    assert_eq!(token_client.balance(&dex_pool), 0);
    assert_eq!(
        vault_client.get_current_protocol(),
        Symbol::new(&env, "none")
    );

    // 4. Event: dex_wd.
    let events = env.events().all();
    let dex_wd_events = find_events_by_topic(events, &env, Symbol::new(&env, "dex_wd"));
    assert_eq!(dex_wd_events.len(), 1);
}

#[test]
fn test_dex_integration_balance_read() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    vault_client.set_dex_pool(&owner, &dex_pool);

    let amount = 75_000_000;
    token_client.mint(&vault_id, &amount);
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);

    // The pool reports the vault's supplied position via `balance(asset, user)`.
    let args: Vec<Val> = vec![&env, usdc_token.into_val(&env), vault_id.into_val(&env)];
    let balance: i128 = env.invoke_contract(&dex_pool, &Symbol::new(&env, "balance"), args);
    assert_eq!(balance, amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #46)")]
fn test_rebalance_dex_without_pool_configured_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, _owner, usdc_token, _dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    token_client.mint(&vault_id, &10_000_000);
    // No set_dex_pool — DexPoolNotConfigured (#46).
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #28)")]
fn test_set_dex_pool_non_owner_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, _owner, _usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    let stranger = Address::generate(&env);
    // OnlyOwnerCanConfigurePool (#28).
    vault_client.set_dex_pool(&stranger, &dex_pool);
}

#[test]
#[should_panic(expected = "Error(Contract, #42)")]
fn test_dex_supply_min_out_enforced() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    vault_client.set_dex_pool(&owner, &dex_pool);

    // Mint 100 USDC but cap the pool to accept only 40 — a partial fill.
    token_client.mint(&vault_id, &100_000_000);
    dex_client.set_max_supply_limit(&40_000_000);

    // Require at least 50 USDC supplied: the 40 USDC fill trips MinOutNotMet (#42).
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &50_000_000);
}

#[test]
fn test_rebalance_switch_blend_to_dex() {
    let env = Env::default();
    env.mock_all_auths();

    // Vault with both a Blend pool and a DEX pool.
    let (vault_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_pool = env.register_contract(None, MockBlendPool);
    let dex_pool = env.register_contract(None, MockDexPool);

    vault_client.set_blend_pool(&owner, &blend_pool);
    vault_client.set_dex_pool(&owner, &dex_pool);

    // 1. Deploy to Blend.
    let amount = 60_000_000;
    token_client.mint(&vault_id, &amount);
    vault_client.rebalance(&symbol_short!("blend"), &850, &0_i128);
    assert_eq!(token_client.balance(&blend_pool), amount);
    assert_eq!(vault_client.get_current_protocol(), symbol_short!("blend"));

    // 2. Switch to the DEX — exits Blend, then supplies the DEX.
    vault_client.rebalance(&symbol_short!("dex"), &900, &0_i128);

    assert_eq!(token_client.balance(&blend_pool), 0);
    assert_eq!(token_client.balance(&dex_pool), amount);
    assert_eq!(vault_client.get_current_protocol(), symbol_short!("dex"));
}

#[test]
fn test_user_withdraw_pulls_from_dex() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    vault_client.set_dex_pool(&owner, &dex_pool);

    // User deposits, agent deploys everything to the DEX.
    let user = Address::generate(&env);
    let amount = 80_000_000;
    mint_and_deposit(&env, &vault_client, &usdc_token, &user, amount);
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);
    assert_eq!(token_client.balance(&vault_id), 0);
    assert_eq!(token_client.balance(&dex_pool), amount);

    // User withdraws — the vault pulls the needed liquidity back from the DEX.
    let withdraw_amount = 30_000_000;
    vault_client.withdraw(&user, &withdraw_amount, &None);

    assert_eq!(token_client.balance(&user), withdraw_amount);
    assert_eq!(token_client.balance(&dex_pool), amount - withdraw_amount);
}

// ─── Issue #346: balance-delta accounting vs lying pool ──────────────────────

/// The vault measures supply outcome via USDC balance delta, not the pool's
/// return value. A pool that lies about its return value from `add_liquidity`
/// cannot inflate vault accounting.
#[test]
fn test_dex_balance_delta_against_lying_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    vault_client.set_dex_pool(&owner, &dex_pool);

    let deposit_amount = 50_000_000_i128;
    token_client.mint(&vault_id, &deposit_amount);

    // Configure the pool to lie: it will actually transfer `deposit_amount`
    // but report double that amount as its return value.
    let lying_reported = deposit_amount * 2;
    dex_client.set_reported_supply_amount(&lying_reported);

    // Rebalance into DEX. min_out = 0 so no MinOutNotMet.
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);

    // The vault's recorded total_assets must reflect the actual balance delta
    // (deposit_amount), not the pool's inflated claim (lying_reported).
    let total_assets = vault_client.get_total_assets();
    assert_eq!(
        total_assets, deposit_amount,
        "vault must record actual balance delta, not pool's reported amount"
    );

    // The pool holds exactly the real amount.
    assert_eq!(token_client.balance(&dex_pool), deposit_amount);
    // The vault holds nothing — all USDC was transferred out.
    assert_eq!(token_client.balance(&vault_id), 0);
}

// Issue #342 — Test partial-fill rebalance into DEX pool
//
// Ensures the vault accounts for the *actual* amount accepted by the DEX pool
// (balance-delta) rather than the originally requested amount, when the pool
// accepts only part of the supplied liquidity.
#[test]
fn test_dex_partial_fill_rebalance_accounts_for_actual_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    vault_client.set_dex_pool(&owner, &dex_pool);

    // Mint 100 USDC into the vault.
    let requested_amount: i128 = 100_000_000; // 100 USDC
    token_client.mint(&vault_id, &requested_amount);

    // Cap the pool to accept at most 60 USDC — a partial fill.
    let pool_cap: i128 = 60_000_000; // 60 USDC
    dex_client.set_max_supply_limit(&pool_cap);

    // Rebalance with min_out = 0 so slippage guard does not block a partial fill.
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);

    // The pool accepted only 60 USDC; the vault retains the remaining 40 USDC.
    let actual_supplied = pool_cap;
    let remaining_in_vault = requested_amount - actual_supplied;

    assert_eq!(
        token_client.balance(&dex_pool),
        actual_supplied,
        "DEX pool balance should equal the actual (partial) fill amount"
    );
    assert_eq!(
        token_client.balance(&vault_id),
        remaining_in_vault,
        "Vault should retain the portion not accepted by the DEX pool"
    );

    // The protocol is still tracked as "dex" (rebalance succeeded with partial fill).
    assert_eq!(vault_client.get_current_protocol(), Symbol::new(&env, "dex"));
}
