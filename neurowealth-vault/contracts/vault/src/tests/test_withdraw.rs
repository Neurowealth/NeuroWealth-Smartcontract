//! Tests for withdrawal functionality

use super::utils::*;
use crate::{WithdrawEvent, TOPIC_WITHDRAW};
use soroban_sdk::{testutils::Address as _, Address, Env, TryFromVal};

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
fn test_withdraw_all_returns_correct_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let expected_balance = client.get_balance(&user);
    let withdrawn = client.withdraw_all(&user, &None);

    assert_eq!(withdrawn, expected_balance);
    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_balance(&user), 0);
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
    assert_eq!(withdraw_events.len(), 1, "Exactly one withdraw event should be emitted");

    let (_, _, data) = &withdraw_events[0];
    let event = WithdrawEvent::try_from_val(&env, data)
        .expect("Should be a valid WithdrawEvent");
    assert_eq!(event.user, user, "Event user should match withdrawer");
    assert_eq!(event.amount, withdraw_amount, "Event amount should match withdrawal");
}

#[test]
fn test_withdraw_with_slippage_protection_no_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    let balance = client.get_balance(&user);

    // Passing a min_amount_out equal to the expected balance should succeed
    // because the vault has idle USDC (no DEX involvement).
    client.withdraw(&user, &balance, &Some(0_i128));
    assert_eq!(client.get_shares(&user), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #42)")]
fn test_withdraw_with_slippage_protection_excessive_min_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    let balance = client.get_balance(&user);

    // Passing a min_amount_out larger than the actual balance should panic
    // with MinOutNotMet (#42).
    client.withdraw(&user, &balance, &Some(balance + 1));
}

#[test]
fn test_withdraw_all_with_slippage_protection_no_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Passing min_amount_out = 0 should succeed (no active slippage check).
    let withdrawn = client.withdraw_all(&user, &Some(0_i128));
    assert_eq!(withdrawn, 10_000_000_i128);
    assert_eq!(client.get_shares(&user), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #42)")]
fn test_withdraw_all_with_slippage_protection_excessive_min_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Expecting more than the deposit should panic.
    client.withdraw_all(&user, &Some(10_000_001_i128));
}
