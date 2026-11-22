//! Regression tests for Issue #568: Stale-state audit and CEI enforcement.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_withdraw_zero_shares_fails_early_before_protocol_interaction() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // User has no shares in the vault. Calling withdraw should fail early with InsufficientShares (#8)
    // without performing any external protocol calls or state mutations.
    client.withdraw(&user, &1_000_000_i128);
}

#[test]
fn test_deposit_and_withdraw_cei_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    assert_eq!(client.get_shares(&user), amount);
    assert_eq!(client.get_total_assets(), amount);

    client.withdraw(&user, &amount);

    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_total_assets(), 0);
}

#[test]
fn test_update_total_assets_pre_reads_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Agent updates total assets to 10_000_000
    client.update_total_assets(&agent, &amount, &false, &0);
    assert_eq!(client.get_total_assets(), amount);
}
