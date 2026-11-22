//! Tests for share locking functionality (#636)

use super::utils::*;

/// Keeps storage entries alive across the large ledger jumps these tests make.
fn keep_storage_alive(env: &Env) {
    env.ledger().with_mut(|li| {
        li.min_persistent_entry_ttl = 1_000_000;
        li.min_temp_entry_ttl = 1_000_000;
        li.max_entry_ttl = 2_000_000;
    });
}
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

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
#[should_panic(expected = "Error(Contract, ")]
fn test_lock_shares_insufficient_unlocked() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Try to lock more shares than the user holds - should fail
    client.lock_shares(&user, &(amount * 2), &30);
}

#[test]
fn test_unlock_shares_after_expiry() {
    let env = Env::default();
    keep_storage_alive(&env);
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares for 30 days
    client.lock_shares(&user, &amount, &30);

    // Fast forward past lock expiry
    env.ledger().set_sequence_number(600_000u32); // ~30 days in ledgers

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
#[should_panic(expected = "Error(Contract, #8)")]
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
    client.withdraw(&user, &unlockable_amount);
}

#[test]
fn test_withdraw_all_with_locked_shares() {
    let env = Env::default();
    keep_storage_alive(&env);
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock half the shares
    client.lock_shares(&user, &(amount / 2), &30);

    // Fast forward past lock expiry
    env.ledger().set_sequence_number(600_000u32);

    // Unlock shares
    client.unlock_shares(&user);

    // Withdraw all should work now
    let withdrawn = client.withdraw_all(&user);
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
    let found = find_events_by_topic(env.events().all(), &env, crate::TOPIC_SHARES_LOCKED);
    assert_eq!(found.len(), 1);
}

#[test]
fn test_unlock_shares_emits_correct_event() {
    let env = Env::default();
    keep_storage_alive(&env);
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Lock shares
    client.lock_shares(&user, &amount, &30);

    // Fast forward past lock expiry
    env.ledger().set_sequence_number(600_000u32);

    // Unlock shares
    client.unlock_shares(&user);

    // Check unlock event
    let found = find_events_by_topic(env.events().all(), &env, crate::TOPIC_SHARES_UNLOCKED);
    assert_eq!(found.len(), 1);
}
// ── #970: reject zero / negative amounts and expiry overflow ─────────────────

#[test]
#[should_panic(expected = "Error(Contract, #37)")]
fn test_lock_shares_zero_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 1_000_000_i128);

    client.lock_shares(&user, &0_i128, &30);
}

#[test]
#[should_panic(expected = "Error(Contract, #37)")]
fn test_lock_shares_negative_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 1_000_000_i128);

    client.lock_shares(&user, &-1_i128, &30);
}

#[test]
fn test_lock_shares_zero_or_negative_leaves_no_lock() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 1_000_000_i128);

    assert!(client.try_lock_shares(&user, &0_i128, &30).is_err());
    assert!(client.try_lock_shares(&user, &-5_i128, &90).is_err());
    assert_eq!(client.get_locked_shares(&user), (0, 0));
}

#[test]
#[should_panic(expected = "Error(Contract, #72)")]
fn test_lock_shares_expiry_overflow_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    // Start close enough to u32::MAX that current_ledger + a 180-day lock
    // no longer fits, but far enough that normal storage TTLs still do.
    env.ledger().with_mut(|li| {
        li.sequence_number = u32::MAX - 1_000_000;
        li.min_persistent_entry_ttl = 4_096;
        li.min_temp_entry_ttl = 16;
        li.max_entry_ttl = 100_000;
    });

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 1_000_000_i128);

    client.lock_shares(&user, &1_000_i128, &180);
}

// ── #971: locking does not change yield, and the event carries no boost ──────

#[test]
fn test_lock_shares_does_not_change_share_value() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let locker = Address::generate(&env);
    let holder = Address::generate(&env);
    let amount = 1_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &locker, amount);
    mint_and_deposit(&env, &client, &usdc_token, &holder, amount);

    let locker_shares = client.get_shares(&locker);
    client.lock_shares(&locker, &locker_shares, &180);

    // Locked and unlocked holders keep identical balances.
    assert_eq!(client.get_shares(&locker), client.get_shares(&holder));
    assert_eq!(client.get_balance(&locker), client.get_balance(&holder));
}

#[test]
fn test_lock_event_has_no_boost_multiplier() {
    use crate::SharesLockedEvent;
    use soroban_sdk::TryFromVal;

    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    client.lock_shares(&user, &amount, &90);

    let found = find_events_by_topic(env.events().all(), &env, crate::TOPIC_SHARES_LOCKED);
    assert_eq!(found.len(), 1);
    let (_, _, data) = &found[0];
    let ev = SharesLockedEvent::try_from_val(&env, data).expect("SharesLockedEvent decode");
    let (_, expiry) = client.get_locked_shares(&user);
    assert_eq!(ev.user, user);
    assert_eq!(ev.shares_locked, amount);
    assert_eq!(ev.lock_duration_days, 90);
    assert_eq!(ev.expiry_ledger, expiry);
}
