//! Tests for share locking functionality (#636)

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_lock_shares_30_days() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares for 30 days
    client.lock_shares(&user, &amount, &30);

    // Verify locked shares
    let (locked_shares, expiry) = client.get_locked_shares(&user);
    assert_eq!(locked_shares, amount);
    assert!(expiry > 0);
}

#[test]
fn test_lock_shares_90_days() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares for 90 days
    client.lock_shares(&user, &amount, &90);

    // Verify locked shares
    let (locked_shares, expiry) = client.get_locked_shares(&user);
    assert_eq!(locked_shares, amount);
    assert!(expiry > 0);
}

#[test]
fn test_lock_shares_180_days() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares for 180 days
    client.lock_shares(&user, &amount, &180);

    // Verify locked shares
    let (locked_shares, expiry) = client.get_locked_shares(&user);
    assert_eq!(locked_shares, amount);
    assert!(expiry > 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #72)")]
fn test_lock_shares_invalid_duration() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Invalid lock duration (not 30, 90, or 180)
    client.lock_shares(&user, &amount, &60);
}

#[test]
#[should_panic(expected = "Error(Contract, #70)")]
fn test_lock_shares_already_locked() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares
    client.lock_shares(&user, &amount, &30);

    // Try to lock again - should fail
    client.lock_shares(&user, &amount, &30);
}

#[test]
#[should_panic(expected = "Error(Contract, #73)")]
fn test_lock_shares_insufficient_unlocked() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock half the shares
    client.lock_shares(&user, &(amount / 2), &30);

    // Try to lock more than available - should fail
    client.lock_shares(&user, &amount, &30);
}

#[test]
fn test_unlock_shares_after_expiry() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares for 30 days
    client.lock_shares(&user, &amount, &30);

    // Fast forward past lock expiry
    env.ledger().set(600_000u32); // ~30 days in ledgers

    // Unlock shares
    client.unlock_shares(&user);

    // Verify shares are unlocked
    let (locked_shares, expiry) = client.get_locked_shares(&user);
    assert_eq!(locked_shares, 0);
    assert_eq!(expiry, 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #71)")]
fn test_unlock_shares_before_expiry() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares for 30 days
    client.lock_shares(&user, &amount, &30);

    // Try to unlock immediately - should fail
    client.unlock_shares(&user);
}

#[test]
#[should_panic(expected = "Error(Contract, #69)")]
fn test_unlock_shares_without_locked() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Try to unlock without locked shares - should fail
    client.unlock_shares(&user);
}

#[test]
fn test_withdraw_locked_shares_restricted() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock half the shares
    client.lock_shares(&user, &(amount / 2), &30);

    // Try to withdraw more than unlocked shares - should fail
    let unlockable_amount = (amount / 2) - 1_000_i128; // Less than unlocked amount
    client.withdraw(&user, &unlockable_amount, &None);
}

#[test]
fn test_withdraw_all_with_locked_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock half the shares
    client.lock_shares(&user, &(amount / 2), &30);

    // Fast forward past lock expiry
    env.ledger().set(600_000u32);

    // Unlock shares
    client.unlock_shares(&user);

    // Withdraw all should work now
    let withdrawn = client.withdraw_all(&user, &None);
    assert!(withdrawn > 0);
}

#[test]
fn test_get_locked_shares_no_lock() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Get locked shares for user with no lock
    let (locked_shares, expiry) = client.get_locked_shares(&user);
    assert_eq!(locked_shares, 0);
    assert_eq!(expiry, 0);
}

#[test]
fn test_lock_shares_emits_correct_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares
    client.lock_shares(&user, &amount, &30);

    // Check lock event
    let events = env.events().all();
    let lock_event = events.iter().find(|e| {
        if let Some(topic) = e.topics.get(0) {
            topic.to_string() == "lock"
        } else {
            false
        }
    });
    assert!(lock_event.is_some());
}

#[test]
fn test_unlock_shares_emits_correct_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares
    client.lock_shares(&user, &amount, &30);

    // Fast forward past lock expiry
    env.ledger().set(600_000u32);

    // Unlock shares
    client.unlock_shares(&user);

    // Check unlock event
    let events = env.events().all();
    let unlock_event = events.iter().find(|e| {
        if let Some(topic) = e.topics.get(0) {
            topic.to_string() == "unlock"
        } else {
            false
        }
    });
    assert!(unlock_event.is_some());
}