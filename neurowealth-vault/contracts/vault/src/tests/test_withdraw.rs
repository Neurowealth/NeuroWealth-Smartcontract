//! Tests for withdrawal functionality

use super::utils::*;
use crate::{DataKey, WithdrawEvent, TOPIC_WITHDRAW};
use soroban_sdk::{testutils::Address as _, Address, Env, TryFromVal};

/// Reads the raw `Shares(user)` value persisted in storage.
fn stored_shares(env: &Env, contract_id: &Address, user: &Address) -> Option<i128> {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
    })
}

/// Asserts the raw `Shares(user)` storage value is exactly zero (no orphan/dust).
fn assert_stored_shares_zero(env: &Env, contract_id: &Address, user: &Address) {
    assert_eq!(
        stored_shares(env, contract_id, user),
        Some(0_i128),
        "Stored Shares(user) must be exactly zero after a full withdrawal"
    );
}

#[test]
fn test_full_withdrawal_burns_all_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let shares_before = client.get_shares(&user);
    assert!(shares_before > 0);

    let balance = client.get_balance(&user);
    client.withdraw(&user, &balance, &None);

    assert_eq!(client.get_shares(&user), 0, "All shares should be burned");
    assert_eq!(client.get_balance(&user), 0, "Balance should be zero");
}

#[test]
fn test_withdraw_all_after_yield_burns_all_shares_to_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Yield accrual: share price rises 1.0 -> 1.5 (15M assets / 10M shares).
    token_client.mint(&contract_id, &5_000_000_i128);
    client.update_total_assets(&agent, &15_000_000_i128, &false, &0);

    let balance_before = client.get_balance(&user);
    assert_eq!(balance_before, 15_000_000_i128);

    client.withdraw_all(&user, &None);

    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_balance(&user), 0);
    assert_stored_shares_zero(&env, &contract_id, &user);
}

#[test]
fn test_partial_withdraw_then_yield_then_full_drain_leaves_zero_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Partial withdrawal before yield.
    client.withdraw(&user, &3_000_000_i128, &None);
    assert!(client.get_shares(&user) > 0);

    // Yield accrual: mint 5M into the vault (vault holds 12M, share price rises).
    token_client.mint(&contract_id, &5_000_000_i128);
    client.update_total_assets(&agent, &12_000_000_i128, &false, &0);

    // Withdraw the remaining balance; no dust/orphan shares may remain.
    client.withdraw_all(&user, &None);

    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_balance(&user), 0);
    assert_stored_shares_zero(&env, &contract_id, &user);
}

#[test]
fn test_multiple_deposits_then_withdraw_all_leaves_zero_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    for amount in [5_000_000_i128, 3_000_000_i128, 2_500_000_i128] {
        mint_and_deposit(&env, &client, &usdc_token, &user, amount);
    }
    assert_eq!(client.get_shares(&user), 10_500_000_i128);

    client.withdraw_all(&user, &None);

    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_balance(&user), 0);
    assert_stored_shares_zero(&env, &contract_id, &user);
}

#[test]
fn test_partial_withdrawal_reduces_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let initial_shares = client.get_shares(&user);
    let withdraw_amount = 3_000_000_i128;

    client.withdraw(&user, &withdraw_amount, &None);

    let remaining_shares = client.get_shares(&user);
    assert!(
        remaining_shares < initial_shares,
        "Shares should decrease after partial withdraw"
    );
}

#[test]
#[should_panic]
fn test_withdraw_more_than_balance_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let excessive_amount = deposit_amount + 1_000_000_i128;
    client.withdraw(&user, &excessive_amount, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #37)")]
fn test_withdraw_zero_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.withdraw(&user, &0, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_withdraw_while_paused_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Pause the vault
    client.pause(&owner);
    assert!(client.is_paused());

    let balance = client.get_balance(&user);
    client.withdraw(&user, &balance, &None);
}

#[test]
#[should_panic]
fn test_withdraw_with_no_balance_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    assert_eq!(client.get_balance(&user), 0);

    client.withdraw(&user, &1_000_000_i128, &None);
}

#[test]
fn test_withdraw_all_detailed_invariants() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let original_shares = client.get_shares(&user);
    let original_total_shares = client.get_total_shares();
    let original_total_assets = client.get_total_assets();

    let expected_usdc = client.convert_to_assets(&original_shares);

    let withdrawn = client.withdraw_all(&user, &None);

    assert_eq!(
        client.get_shares(&user),
        0,
        "After withdraw_all, user shares are exactly 0"
    );
    assert_eq!(
        client.get_balance(&user),
        0,
        "After withdraw_all, user balance is exactly 0"
    );
    assert_eq!(
        withdrawn, expected_usdc,
        "USDC returned equals convert_to_assets(original_shares)"
    );
    assert_eq!(
        client.get_total_shares(),
        original_total_shares - original_shares,
        "total_shares decreases by exactly original shares"
    );
    assert_eq!(
        client.get_total_assets(),
        original_total_assets - withdrawn,
        "total_assets decreases by exactly USDC returned"
    );
}

#[test]
fn test_withdraw_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let withdraw_amount = 3_000_000_i128;
    client.withdraw(&user, &withdraw_amount, &None);

    let withdraw_events = find_events_by_topic(env.events().all(), &env, TOPIC_WITHDRAW);
    assert_eq!(
        withdraw_events.len(),
        1,
        "Exactly one withdraw event should be emitted"
    );

    let (_, _, data) = &withdraw_events[0];
    let event = WithdrawEvent::try_from_val(&env, data).expect("Should be a valid WithdrawEvent");
    assert_eq!(event.user, user, "Event user should match withdrawer");
    assert_eq!(
        event.amount, withdraw_amount,
        "Event amount should match withdrawal"
    );
}
