//! Tests for pause/unpause functionality

extern crate std;

use super::utils::*;
use crate::{EmergencyPausedEvent, VaultPausedEvent, TOPIC_EMERGENCY_PAUSED, TOPIC_PAUSED};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, TryFromVal};

#[test]
fn test_owner_can_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(!client.is_paused(), "Vault should start unpaused");

    client.pause(&owner);

    assert!(client.is_paused(), "Vault should be paused");
}

#[test]
fn test_owner_can_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    client.unpause(&owner);
    assert!(!client.is_paused(), "Vault should be unpaused");
}

#[test]
fn test_owner_can_emergency_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(!client.is_paused());

    client.emergency_pause(&owner);

    assert!(client.is_paused(), "Vault should be emergency paused");
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_non_owner_cannot_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Owner pauses
    client.pause(&owner);
    assert!(client.is_paused());

    // A fresh address that is NOT the owner tries to unpause
    let non_owner = Address::generate(&env);
    client.unpause(&non_owner);
}

#[test]
#[should_panic]
fn test_unauthorized_users_cannot_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let unauthorized = Address::generate(&env);

    client.emergency_pause(&owner);
    // Fails because unauthorized != stored_owner
    client.pause(&unauthorized);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_deposit_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.pause(&owner);
    assert!(client.is_paused());

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;

    token_client.mint(&user, &amount);
    client.deposit(&user, &amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_withdraw_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    client.pause(&owner);
    assert!(client.is_paused());

    let balance = client.get_balance(&user);
    client.withdraw(&user, &balance, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_rebalance_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    // require_not_paused fires before any blend check
    client.rebalance(&soroban_sdk::symbol_short!("blend"), &500_i128, &0_i128);
}

#[test]
fn test_pause_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    let pause_events = find_events_by_topic(env.events().all(), &env, TOPIC_PAUSED);
    assert_eq!(pause_events.len(), 1, "Exactly one paused event should be emitted");

    let (_, _, data) = &pause_events[0];
    let event = VaultPausedEvent::try_from_val(&env, data)
        .expect("Should be a valid VaultPausedEvent");
    assert_eq!(event.owner, owner, "Event owner should match caller");
}

#[test]
fn test_emergency_pause_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.emergency_pause(&owner);

    let emergency_events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED);
    assert_eq!(
        emergency_events.len(), 1,
        "Exactly one emergency paused event should be emitted"
    );

    let (_, _, data) = &emergency_events[0];
    let event = EmergencyPausedEvent::try_from_val(&env, data)
        .expect("Should be a valid EmergencyPausedEvent");
    assert_eq!(event.owner, owner, "Event owner should match caller");
}

// ============================================================================
// ISSUE #189: Block upgrade while paused
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_upgrade_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    let fake_hash = BytesN::from_array(&env, &[0u8; 32]);
    client.schedule_upgrade(&owner, &fake_hash);
}

#[test]
fn test_upgrade_unpaused_vault_clears_pause_guard() {
    // Verifies that require_not_paused does not block upgrade on a healthy vault:
    // pause then unpause, and confirm the vault is no longer paused.
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());
    client.unpause(&owner);
    assert!(
        !client.is_paused(),
        "vault must be unpaused before upgrade is allowed"
    );
}
