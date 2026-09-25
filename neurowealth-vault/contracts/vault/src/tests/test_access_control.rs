//! Tests for ownership and agent access control
//!
//! Covers:
//! - Owner-only functions: pause, unpause, emergency_pause, set_* config, upgrade
//! - Agent-only functions: rebalance, update_total_assets
//! - Two-step ownership transfer: initiate, accept, cancel
//! - Paused-state enforcement on user operations

use super::utils::*;
use crate::{AgentUpdatedEvent, TOPIC_AGENT_UPDATED};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, BytesN, Env, TryFromVal};

// ============================================================================
// OWNER-ONLY HAPPY PATHS
// ============================================================================

#[test]
fn test_owner_can_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(!client.is_paused());
    client.pause(&owner);
    assert!(client.is_paused());
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
    assert!(!client.is_paused());
}

#[test]
fn test_owner_can_emergency_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(!client.is_paused());
    client.emergency_pause(&owner);
    assert!(client.is_paused());
}

#[test]
fn test_owner_can_set_tvl_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let cap = 50_000_000_000_i128;
    client.set_tvl_cap(&cap);
    assert_eq!(client.get_tvl_cap(), cap);
}

#[test]
fn test_owner_can_set_user_deposit_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let cap = 25_000_000_000_i128;
    client.set_user_deposit_cap(&cap);
    assert_eq!(client.get_user_deposit_cap(), cap);
}

#[test]
fn test_owner_can_set_deposit_limits() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let min = 2_000_000_i128;
    let max = 50_000_000_000_i128;
    client.set_deposit_limits(&min, &max);
    assert_eq!(client.get_min_deposit(), min);
    assert_eq!(client.get_max_deposit(), max);
}

#[test]
fn test_owner_can_update_agent() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    assert_eq!(client.get_agent(), new_agent);
    assert_ne!(client.get_agent(), old_agent);
}

#[test]
fn test_update_agent_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    let agent_events = find_events_by_topic(env.events().all(), &env, TOPIC_AGENT_UPDATED);
    assert_eq!(
        agent_events.len(),
        1,
        "Exactly one agent event should be emitted"
    );

    let (_, _, data) = &agent_events[0];
    let event =
        AgentUpdatedEvent::try_from_val(&env, data).expect("Should be a valid AgentUpdatedEvent");
    assert_eq!(
        event.old_agent, old_agent,
        "old_agent should match previous agent"
    );
    assert_eq!(
        event.new_agent, new_agent,
        "new_agent should match updated agent"
    );
}

#[test]
fn test_owner_can_set_blend_pool() {
    let env = Env::default();
    env.mock_all_auths();

    // set_blend_pool calls BlendPoolClient::get_balance() internally,
    // so blend_pool must be a deployed contract, not a plain address.
    let (contract_id, _agent, owner, _usdc_token, _blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let blend_pool = env.register_contract(None, MockBlendPool);
    client.set_blend_pool(&owner, &blend_pool);

    assert_eq!(client.get_blend_pool(), Some(blend_pool));
}

// ============================================================================
// OWNER-ONLY NEGATIVE PATHS (non-owner must be rejected)
// ============================================================================

/// `set_deposit_limits` enforces ownership via `require_is_owner`, which calls
/// `owner.require_auth()` on the stored owner address. Because there is no
/// explicit address parameter to compare against, the guard cannot be tested
/// by passing a fake address; instead we revoke all auth with `mock_auths(&[])`
/// and verify the call is rejected through the `try_*` client variant.
#[test]
fn test_non_owner_cannot_set_deposit_limits() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Drop all auth: owner.require_auth() inside require_is_owner cannot be
    // satisfied, so the call must fail regardless of who invokes it.
    let _non_owner = Address::generate(&env);
    env.mock_auths(&[]);

    let min = 2_000_000_i128;
    let max = 50_000_000_000_i128;
    let result = client.try_set_deposit_limits(&min, &max);
    assert!(
        result.is_err(),
        "set_deposit_limits must reject calls without the owner's authorization"
    );
}

/// `set_tvl_cap` is owner-only via `require_is_owner`. Removing that guard
/// would cause this test to fail: valid inputs would return `Ok` and
/// `result.is_err()` would be false.
#[test]
fn test_non_owner_cannot_set_tvl_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let _non_owner = Address::generate(&env);
    env.mock_auths(&[]);

    let result = client.try_set_tvl_cap(&50_000_000_000_i128);
    assert!(
        result.is_err(),
        "set_tvl_cap must reject calls without the owner's authorization"
    );
}

/// `set_limits` is owner-only via `require_is_owner`. Removing that guard
/// would cause this test to fail: valid inputs would return `Ok(Ok(()))` and
/// the outer `result.is_err()` would be false.
#[test]
fn test_non_owner_cannot_set_limits() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let _non_owner = Address::generate(&env);
    env.mock_auths(&[]);

    let result = client.try_set_limits(&1_000_000_i128, &50_000_000_000_i128);
    assert!(
        result.is_err(),
        "set_limits must reject calls without the owner's authorization"
    );
}

/// `set_user_deposit_cap` must reject non-owner callers. Mirrors the
/// `set_deposit_limits` pattern: revoke all auth and assert the try_* call
/// returns an error.
#[test]
fn test_non_owner_cannot_set_user_deposit_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let _non_owner = Address::generate(&env);
    env.mock_auths(&[]);

    let result = client.try_set_user_deposit_cap(&10_000_000_000_i128);
    assert!(
        result.is_err(),
        "set_user_deposit_cap must reject calls without the owner's authorization"
    );
}

/// `set_caps` is owner-only. Revoke all auth and verify the call fails.
#[test]
fn test_non_owner_cannot_set_caps() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let _non_owner = Address::generate(&env);
    env.mock_auths(&[]);

    let result = client.try_set_caps(&1_000_000_i128, &50_000_000_000_i128);
    assert!(
        result.is_err(),
        "set_caps must reject calls without the owner's authorization"
    );
}

/// `set_rebalance_cooldown` is owner-only via `require_is_owner`.
#[test]
fn test_non_owner_cannot_set_rebalance_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let _non_owner = Address::generate(&env);
    env.mock_auths(&[]);

    let result = client.try_set_rebalance_cooldown(&10_u32);
    assert!(
        result.is_err(),
        "set_rebalance_cooldown must reject calls without the owner's authorization"
    );
}

/// `set_approval_ttl` must reject non-owner callers (owner guard via
/// `require_is_owner`). Use the try_ wrapper to check the error path.
#[test]
fn test_non_owner_cannot_set_approval_ttl() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let _non_owner = Address::generate(&env);
    env.mock_auths(&[]);

    let result = client.try_set_approval_ttl(&2_000_u32);
    assert!(
        result.is_err(),
        "set_approval_ttl must reject calls without the owner's authorization"
    );
}

#[test]
#[should_panic(expected = "vault: only owner can set blend approval ttl")]
fn test_non_owner_cannot_set_blend_approval_ttl() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let non_owner = Address::generate(&env);
    client.set_blend_approval_ttl(&non_owner, &42_u32);
}

#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_non_owner_cannot_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let non_owner = Address::generate(&env);
    client.pause(&non_owner);
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_non_owner_cannot_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    let non_owner = Address::generate(&env);
    client.unpause(&non_owner);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_non_owner_cannot_emergency_pause() {
    let env = Env::default();
    env.mock_all_auths();

    // Create vault where agent != owner so we can use a true non-owner
    let deployer = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);
    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    // owner and agent are distinct; use a fresh address as non-owner
    let non_owner = Address::generate(&env);
    client.emergency_pause(&non_owner);
}

#[test]
#[should_panic(expected = "Error(Contract, #28)")]
fn test_non_owner_cannot_set_blend_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let blend_pool = Address::generate(&env);
    let non_owner = Address::generate(&env);
    // set_blend_pool checks owner == stored_owner explicitly
    client.set_blend_pool(&non_owner, &blend_pool);
}

#[test]
#[should_panic(expected = "Error(Contract, #34)")]
fn test_non_owner_cannot_upgrade() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let non_owner = Address::generate(&env);
    let fake_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    client.schedule_upgrade(&non_owner, &fake_wasm_hash);
}

// ============================================================================
// AGENT-ONLY HAPPY PATHS
// ============================================================================

#[test]
fn test_agent_can_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // "none" protocol is always safe — no external pool required
    client.rebalance(&symbol_short!("none"), &500_i128, &0_i128);

    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("none"),
        "CurrentProtocol should be 'none' after rebalance"
    );
}

#[test]
fn test_agent_can_update_total_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let new_total = deposit_amount;
    client.update_total_assets(&agent, &new_total, &false, &0);
    assert_eq!(client.get_total_assets(), new_total);
}

// ============================================================================
// AGENT-ONLY NEGATIVE PATHS (non-agent must be rejected)
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #30)")]
fn test_non_agent_cannot_update_total_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let non_agent = Address::generate(&env);
    client.update_total_assets(&non_agent, &deposit_amount, &false, &0);
}

// ============================================================================
// TWO-STEP OWNERSHIP TRANSFER
// ============================================================================

#[test]
fn test_transfer_ownership_sets_pending_owner() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);

    // Pending owner should be set
    let pending = client.get_pending_owner();
    assert!(pending.is_some(), "Pending owner should be set");
    assert_eq!(pending.unwrap(), new_owner);
}

#[test]
fn test_transfer_ownership_does_not_change_owner_immediately() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);

    // Owner should remain unchanged until accept_ownership is called
    assert_eq!(
        client.get_owner(),
        owner,
        "Owner should not change until acceptance"
    );
}

#[test]
fn test_accept_ownership_completes_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, old_owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);

    // Initiate transfer
    client.transfer_ownership(&new_owner);
    assert_eq!(client.get_pending_owner().unwrap(), new_owner);

    // Complete transfer
    client.accept_ownership(&new_owner);

    assert_eq!(client.get_owner(), new_owner);
    assert_ne!(client.get_owner(), old_owner);
    // Pending owner is cleared after acceptance
    assert!(
        client.get_pending_owner().is_none(),
        "Pending owner should be cleared"
    );
}

#[test]
fn test_new_owner_can_use_owner_functions_after_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _old_owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);
    client.accept_ownership(&new_owner);

    // New owner can now pause (owner-only function)
    client.pause(&new_owner);
    assert!(client.is_paused());

    // New owner can unpause
    client.unpause(&new_owner);
    assert!(!client.is_paused());
}

/// Test that the pending owner cannot cancel an ownership transfer (Issue #573).
/// Only the current owner should be able to cancel, preventing griefing of
/// legitimate ownership rotations.
#[test]
fn test_pending_owner_cannot_cancel_ownership_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let pending_owner = Address::generate(&env);
    client.transfer_ownership(&pending_owner);

    assert_eq!(client.get_pending_owner().unwrap(), pending_owner);

    // The pending owner tries to cancel — should fail with CallerIsNotOwner
    let result = client.try_cancel_ownership_transfer();
    assert!(
        result.is_err(),
        "pending owner should not be able to cancel ownership transfer"
    );
    assert_eq!(
        result,
        Err(Ok(soroban_sdk::Error::from_contract_error(34))),
        "should fail with CallerIsNotOwner error"
    );
}

/// Test that an uninvolved third party cannot cancel an ownership transfer (Issue #573).
/// Only the current owner should be able to cancel.
#[test]
fn test_third_party_cannot_cancel_ownership_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let pending_owner = Address::generate(&env);
    client.transfer_ownership(&pending_owner);

    // An uninvolved third party tries to cancel — should fail with CallerIsNotOwner
    let third_party = Address::generate(&env);
    let result = client.try_cancel_ownership_transfer();
    assert!(
        result.is_err(),
        "third party should not be able to cancel ownership transfer"
    );
    assert_eq!(
        result,
        Err(Ok(soroban_sdk::Error::from_contract_error(34))),
        "should fail with CallerIsNotOwner error"
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_old_owner_cannot_use_owner_functions_after_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, old_owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);
    client.accept_ownership(&new_owner);

    // Old owner can no longer pause
    client.pause(&old_owner);
}

#[test]
fn test_transfer_ownership_can_be_overwritten() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let first_candidate = Address::generate(&env);
    let second_candidate = Address::generate(&env);

    client.transfer_ownership(&first_candidate);
    assert_eq!(client.get_pending_owner().unwrap(), first_candidate);

    // Overwrite with a different candidate
    client.transfer_ownership(&second_candidate);
    assert_eq!(
        client.get_pending_owner().unwrap(),
        second_candidate,
        "Pending owner should be updated to the latest candidate"
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #29)")]
fn test_wrong_address_cannot_accept_ownership() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);

    // A different address tries to accept
    let impostor = Address::generate(&env);
    client.accept_ownership(&impostor);
}

#[test]
// No pending transfer -> accept_ownership rejects with VaultError::CallerIsNotPendingOwner (code 29).
#[should_panic(expected = "Error(Contract, #29)")]
fn test_accept_ownership_without_pending_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // No transfer initiated — accept should panic
    let random = Address::generate(&env);
    client.accept_ownership(&random);
}

#[test]
fn test_cancel_ownership_transfer_clears_pending() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);
    assert!(client.get_pending_owner().is_some());

    client.cancel_ownership_transfer();

    assert!(
        client.get_pending_owner().is_none(),
        "Pending owner should be cleared after cancel"
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #29)")]
fn test_cancel_without_pending_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // No pending transfer started — expects VaultError::CallerIsNotPendingOwner (code 29)
    client.cancel_ownership_transfer();
}

#[test]
// #538: once a transfer is cancelled the pending state is cleared, so a later
// accept_ownership rejects with VaultError::CallerIsNotPendingOwner (code 29).
#[should_panic(expected = "Error(Contract, #29)")]
fn test_accept_after_cancel_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);
    client.cancel_ownership_transfer();

    // Transfer was cancelled — accept should fail
    client.accept_ownership(&new_owner);
}

// ============================================================================
// PAUSED-STATE ENFORCEMENT
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_deposit_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.pause(&owner);

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
    client.withdraw(&user, &amount, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_withdraw_all_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    client.pause(&owner);
    client.withdraw_all(&user, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_rebalance_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    client.rebalance(&symbol_short!("none"), &500_i128, &0_i128);
}

// ============================================================================
// AGENT CANNOT EXECUTE OWNER-ONLY FUNCTIONS
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_agent_cannot_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // agent is distinct from owner, so this should panic
    client.pause(&agent);
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_agent_cannot_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    // agent is distinct from owner, so this should panic
    client.unpause(&agent);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_agent_cannot_emergency_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // agent is distinct from owner, so this should panic
    client.emergency_pause(&agent);
}

#[test]
#[should_panic(expected = "Error(Contract, #28)")]
fn test_agent_cannot_set_blend_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let blend_pool = Address::generate(&env);
    // agent is distinct from owner, so this should panic
    client.set_blend_pool(&agent, &blend_pool);
}

#[test]
#[should_panic(expected = "Error(Contract, #34)")]
fn test_agent_cannot_upgrade() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let fake_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    // agent is distinct from owner, so this should panic
    client.schedule_upgrade(&agent, &fake_wasm_hash);
}

// ============================================================================
// ISSUE #118 — OWNER CANNOT EXECUTE AGENT-ONLY FUNCTIONS
// ============================================================================

/// Verifies that the owner cannot impersonate the agent in update_total_assets,
/// which performs an explicit assert_eq!(agent, stored_agent) guard.
#[test]
#[should_panic(expected = "Error(Contract, #30)")]
fn test_owner_cannot_update_total_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);

    // owner passed where agent is expected — must be rejected
    client.update_total_assets(&owner, &5_000_000_i128, &false, &0);
}

/// Verifies that a completely unrelated address cannot impersonate the agent.
#[test]
#[should_panic(expected = "Error(Contract, #30)")]
fn test_stranger_cannot_update_total_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);

    let stranger = Address::generate(&env);
    client.update_total_assets(&stranger, &5_000_000_i128, &false, &0);
}

/// Verifies that the agent cannot pause the vault (owner-only via explicit address check).
/// Complements existing test_agent_cannot_pause with a clear issue-#118 label.
#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_agent_cannot_pause_owner_role_is_exclusive() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // agent != owner — the explicit assert_eq! in pause() must reject this
    client.pause(&agent);
}

// ============================================================================
// ISSUE #571 — ADVERSARIAL SWEEP: AGENT CANNOT CALL OWNER-ONLY SETTERS
//              DURING TIMELOCK WINDOWS (NORMAL AND PENDING STATES)
// ============================================================================

/// Comprehensive adversarial test: exhaustively verify that the agent cannot
/// invoke any owner-only setter during normal (no-timelock) or pending-timelock
/// states. Each setter is a critical security boundary.
#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_agent_cannot_pause_during_normal_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&agent);
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_agent_cannot_unpause_during_normal_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    client.unpause(&agent);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_agent_cannot_emergency_pause_during_normal_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.emergency_pause(&agent);
}

#[test]
fn test_agent_cannot_set_tvl_cap_during_normal_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let result = client.try_set_tvl_cap(&50_000_000_000_i128);
    assert!(
        result.is_err(),
        "agent must not be able to set TVL cap during normal state"
    );
}

#[test]
fn test_agent_cannot_set_user_deposit_cap_during_normal_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let result = client.try_set_user_deposit_cap(&10_000_000_000_i128);
    assert!(
        result.is_err(),
        "agent must not be able to set user deposit cap during normal state"
    );
}

#[test]
fn test_agent_cannot_set_deposit_limits_during_normal_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let result = client.try_set_deposit_limits(&2_000_000_i128, &50_000_000_000_i128);
    assert!(
        result.is_err(),
        "agent must not be able to set deposit limits during normal state"
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #34)")]
fn test_agent_cannot_schedule_upgrade_during_normal_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let fake_wasm_hash = BytesN::from_array(&env, &[0u8; 32]);
    client.schedule_upgrade(&agent, &fake_wasm_hash);
}

#[test]
fn test_agent_cannot_set_rebalance_cooldown_during_normal_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let result = client.try_set_rebalance_cooldown(&10_u32);
    assert!(
        result.is_err(),
        "agent must not be able to set rebalance cooldown during normal state"
    );
}

#[test]
fn test_agent_cannot_set_approval_ttl_during_normal_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let result = client.try_set_approval_ttl(&2_000_u32);
    assert!(
        result.is_err(),
        "agent must not be able to set approval TTL during normal state"
    );
}

/// After an agent update is proposed (pending timelock), the agent STILL cannot
/// call owner-only setters during the timelock window.
#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_agent_cannot_pause_during_pending_agent_timelock() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // Pending timelock is now active (agent update proposed but not confirmed).
    // The old agent should still not be able to pause.
    client.pause(&agent);
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_agent_cannot_unpause_during_pending_agent_timelock() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // During pending timelock, agent should not be able to unpause.
    client.unpause(&agent);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_agent_cannot_emergency_pause_during_pending_agent_timelock() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // During pending timelock, agent should not be able to emergency pause.
    client.emergency_pause(&agent);
}

#[test]
fn test_agent_cannot_set_tvl_cap_during_pending_agent_timelock() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // During pending timelock, agent cannot set TVL cap.
    let result = client.try_set_tvl_cap(&50_000_000_000_i128);
    assert!(
        result.is_err(),
        "agent must not be able to set TVL cap during pending agent timelock"
    );
}

#[test]
fn test_agent_cannot_set_deposit_limits_during_pending_agent_timelock() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // During pending timelock, agent cannot set deposit limits.
    let result = client.try_set_deposit_limits(&2_000_000_i128, &50_000_000_000_i128);
    assert!(
        result.is_err(),
        "agent must not be able to set deposit limits during pending agent timelock"
    );
}
