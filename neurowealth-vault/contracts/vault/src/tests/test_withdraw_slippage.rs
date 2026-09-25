//! Issue #463 — slippage protection on user withdrawals from DEX pools.
//!
//! `withdraw` and `withdraw_all` accept an optional `min_amount_out`
//! (`Option<i128>`). When set, the reconciled withdrawal (idle balance +
//! DEX/Blend pull) must meet the floor or the whole withdrawal reverts with
//! [`VaultError::MinOutNotMet`]. Passing `None` preserves the legacy
//! partial-fill behavior.
//!
//! Uses the shared mock DEX pool (`set_max_withdraw_limit`) to simulate
//! thin-pool liquidity — the same mechanism as the #516 rebalance tests.

use super::utils::*;
use crate::NeuroWealthVaultClient;
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

const USDC_FLOOR: i128 = 20_000_000; // 20 USDC

#[test]
fn withdraw_with_none_min_amount_out_preserves_partial_fill_behavior() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    vault_client.set_dex_pool(&owner, &dex_pool);

    // Deploy everything to the DEX, then cap what the pool can return so the
    // reconciled withdrawal is a partial fill.
    let user = Address::generate(&env);
    let deposit = 80_000_000_i128;
    mint_and_deposit(&env, &vault_client, &usdc_token, &user, deposit);
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);
    assert_eq!(token_client.balance(&dex_pool), deposit);

    // Partial fill capped to 30 USDC.
    dex_client.set_max_withdraw_limit(&30_000_000);

    // `None` = legacy behavior: partial fill succeeds (no slippage check).
    vault_client.withdraw(&user, &50_000_000, &None);

    assert_eq!(token_client.balance(&user), 30_000_000);
    assert_eq!(token_client.balance(&dex_pool), deposit - 30_000_000);
}

#[test]
fn withdraw_meets_min_amount_out_when_liquidity_is_available() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    vault_client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(&env);
    let deposit = 80_000_000_i128;
    mint_and_deposit(&env, &vault_client, &usdc_token, &user, deposit);
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);

    // Full liquidity: the requested 50 USDC meets the 20 USDC floor.
    vault_client.withdraw(&user, &50_000_000, &Some(USDC_FLOOR));

    assert_eq!(token_client.balance(&user), 50_000_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #42)")]
fn withdraw_reverts_when_reconciled_amount_falls_below_the_floor() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    vault_client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(&env);
    let deposit = 80_000_000_i128;
    mint_and_deposit(&env, &vault_client, &usdc_token, &user, deposit);
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);
    assert_eq!(token_client.balance(&vault_id), 0);

    // Thin pool: capped at 10 USDC while the user requests 50.
    dex_client.set_max_withdraw_limit(&10_000_000);

    // Floor of 20 USDC cannot be met by the reconciled 10 USDC → MinOutNotMet.
    vault_client.withdraw(&user, &50_000_000, &Some(USDC_FLOOR));
}

#[test]
fn withdraw_all_reverts_below_the_floor() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    vault_client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(&env);
    let deposit = 80_000_000_i128;
    mint_and_deposit(&env, &vault_client, &usdc_token, &user, deposit);
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);

    // Thin pool: capped at 10 USDC, full entitlement is 80 USDC.
    dex_client.set_max_withdraw_limit(&10_000_000);

    let floor: Option<i128> = Some(USDC_FLOOR);
    let result = vault_client.try_withdraw_all(&user, &floor);
    assert!(result.is_err());

    // Nothing moved, nothing burned — the failed withdrawal is atomic.
    assert_eq!(token_client.balance(&user), 0);
    assert_eq!(vault_client.get_shares(&user), deposit);
}

#[test]
fn withdraw_all_meets_min_amount_out() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    vault_client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(&env);
    let deposit = 80_000_000_i128;
    mint_and_deposit(&env, &vault_client, &usdc_token, &user, deposit);
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);

    // Pool returns everything — the 20 USDC floor is met.
    let floor: Option<i128> = Some(USDC_FLOOR);
    let returned = vault_client.withdraw_all(&user, &floor);

    assert!(returned >= USDC_FLOOR);
    assert_eq!(token_client.balance(&user), deposit);
}

#[test]
fn withdraw_rejects_a_negative_min_amount_out() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, _owner, usdc_token, _dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    let user = Address::generate(&env);
    let deposit = 80_000_000_i128;
    mint_and_deposit(&env, &vault_client, &usdc_token, &user, deposit);

    let negative: Option<i128> = Some(-1);
    let result = vault_client.try_withdraw(&user, &5_000_000, &negative);
    assert!(result.is_err());
}

#[test]
fn withdraw_floor_zero_is_equivalent_to_none() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    vault_client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(&env);
    let deposit = 80_000_000_i128;
    mint_and_deposit(&env, &vault_client, &usdc_token, &user, deposit);
    vault_client.rebalance(&Symbol::new(&env, "dex"), &850, &0_i128);

    // Thin pool partial fill: an explicit 0 floor accepts it.
    dex_client.set_max_withdraw_limit(&30_000_000);

    let zero_floor: Option<i128> = Some(0);
    vault_client.withdraw(&user, &50_000_000, &zero_floor);

    assert_eq!(token_client.balance(&user), 30_000_000);
}

#[test]
fn idle_only_withdrawals_always_meet_the_floor() {
    let env = Env::default();
    env.mock_all_auths();

    // No protocol deployment: the vault is idle, so any floor up to the
    // idle balance is trivially met.
    let (vault_id, _agent, _owner, usdc_token, _dex_pool) = setup_vault_with_token_and_dex(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    let user = Address::generate(&env);
    let deposit = 80_000_000_i128;
    mint_and_deposit(&env, &vault_client, &usdc_token, &user, deposit);

    let floor: Option<i128> = Some(50_000_000);
    vault_client.withdraw(&user, &50_000_000, &floor);

    let token_client = TestTokenClient::new(&env, &usdc_token);
    assert_eq!(token_client.balance(&user), 50_000_000);
}
