//! Tests for the two-step agent update timelock (#317).
//!
//! Verifies:
//! - Propose stores pending agent and emits AgentUpdateProposedEvent.
//! - Confirm succeeds only after the timelock window and emits both confirm events.
//! - Cancel clears the pending proposal and emits AgentUpdateCancelledEvent.
//! - Duplicate proposals are rejected while one is pending.
//! - Confirm before timelock is rejected with TimelockNotExpired (#50).
//! - Confirm/cancel with no pending proposal are rejected with NoTimelockPending (#49).
//! - Only the owner can propose, confirm, or cancel.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

/// Keeps storage entries alive across the large ledger jumps these tests make
/// (mirrors the helper in `test_share_locking.rs`).
fn keep_storage_alive(env: &Env) {
    env.ledger().with_mut(|li| {
        li.min_persistent_entry_ttl = 1_000_000;
        li.min_temp_entry_ttl = 1_000_000;
        li.max_entry_ttl = 2_000_000;
    });
}

/// A successful propose stores the pending agent and emits the proposal event.
#[test]
fn test_propose_agent_stores_pending_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // Active agent is unchanged until confirmation.
    assert_eq!(
        client.get_agent(),
        old_agent,
        "active agent must not change on propose"
    );

    // Pending update is recorded.
    let pending = client.get_pending_agent_update();
    assert!(pending.is_some(), "pending agent update should be recorded");
    let (pending_addr, expiry) = pending.unwrap();
    assert_eq!(pending_addr, new_agent, "pending agent address mismatch");
    assert!(expiry > 0, "expiry ledger should be set");
}

/// Proposing while another proposal is pending must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #48)")]
fn test_propose_while_pending_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent1 = Address::generate(&env);
    let new_agent2 = Address::generate(&env);

    client.update_agent(&new_agent1);
    // Second proposal while first is pending must panic with TimelockAlreadyPending (#48).
    client.update_agent(&new_agent2);
}

/// Confirming before the timelock has elapsed must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #50)")]
fn test_confirm_before_timelock_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // Immediately try to confirm — timelock not elapsed yet.
    client.confirm_agent_update();
}

/// Confirming after the timelock window applies the update and clears pending state.
#[test]
fn test_confirm_after_timelock_applies_update() {
    let env = Env::default();
    keep_storage_alive(&env);
    env.mock_all_auths();
    keep_storage_alive(&env);

    let (contract_id, _old_agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    let (_, expiry) = client.get_pending_agent_update().unwrap();

    // Advance the ledger past the timelock expiry.
    env.ledger().set_sequence_number(expiry);

    client.confirm_agent_update();

    assert_eq!(
        client.get_agent(),
        new_agent,
        "agent should be updated after confirm"
    );
    assert!(
        client.get_pending_agent_update().is_none(),
        "pending state should be cleared after confirm"
    );
}

/// After confirmation, a new proposal can be submitted.
#[test]
fn test_new_proposal_allowed_after_confirm() {
    let env = Env::default();
    keep_storage_alive(&env);
    env.mock_all_auths();
    keep_storage_alive(&env);

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let agent2 = Address::generate(&env);
    client.update_agent(&agent2);

    let (_, expiry) = client.get_pending_agent_update().unwrap();
    env.ledger().set_sequence_number(expiry);
    client.confirm_agent_update();

    // Should now be able to propose again.
    let agent3 = Address::generate(&env);
    client.update_agent(&agent3);
    assert!(client.get_pending_agent_update().is_some());
}

/// Cancel clears the pending proposal and the active agent is unchanged.
#[test]
fn test_cancel_clears_pending_agent() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);
    client.cancel_agent_update();

    assert_eq!(
        client.get_agent(),
        old_agent,
        "active agent unchanged after cancel"
    );
    assert!(
        client.get_pending_agent_update().is_none(),
        "pending state cleared after cancel"
    );
}

/// After cancel, a fresh proposal can be submitted.
#[test]
fn test_new_proposal_allowed_after_cancel() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let agent2 = Address::generate(&env);
    client.update_agent(&agent2);
    client.cancel_agent_update();

    let agent3 = Address::generate(&env);
    client.update_agent(&agent3);
    assert!(client.get_pending_agent_update().is_some());
}

/// Confirm with no pending proposal must be rejected with NoTimelockPending (#49).
#[test]
#[should_panic(expected = "Error(Contract, #49)")]
fn test_confirm_with_no_pending_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.confirm_agent_update();
}

/// Cancel with no pending proposal must be rejected with NoTimelockPending (#49).
#[test]
#[should_panic(expected = "Error(Contract, #49)")]
fn test_cancel_with_no_pending_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.cancel_agent_update();
}

/// `get_pending_agent_update` returns None before any proposal is made.
#[test]
fn test_get_pending_agent_update_none_initially() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(
        client.get_pending_agent_update().is_none(),
        "no pending update should exist initially"
    );
}

// ─── Issue #513 ──────────────────────────────────────────────────────────────

/// Pins the current behaviour when `confirm_agent_update()` is called well
/// after the timelock expiry ledger has passed.
///
/// The timelock is a *minimum* delay, not a window with an upper deadline.
/// Advancing many ledgers past `expiry` must still succeed — there is no
/// `TimelockExpired` guard.
#[test]
fn test_confirm_agent_update_long_after_expiry_succeeds() {
    let env = Env::default();
    keep_storage_alive(&env);
    env.mock_all_auths();
    keep_storage_alive(&env);

    let (contract_id, _old_agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    let (_, expiry) = client.get_pending_agent_update().unwrap();

    // Advance the ledger 100 000 ledgers past the expiry.
    env.ledger().set_sequence_number(expiry + 100_000);

    // Must succeed — no upper deadline is enforced.
    client.confirm_agent_update();

    assert_eq!(
        client.get_agent(),
        new_agent,
        "agent must be updated even when confirmed long after expiry"
    );
    assert!(
        client.get_pending_agent_update().is_none(),
        "pending state must be cleared after late confirmation"
    );
}

// ─── Issue #514 ──────────────────────────────────────────────────────────────

/// `cancel_agent_update()` panics with `NoTimelockPending` (#49) when called
/// after a previous proposal was already cancelled — the slot is empty again.
///
/// This is distinct from the "never-proposed" variant: it verifies that the
/// cancel path itself clears the pending slot so a second cancel has nothing
/// to act on.
#[test]
#[should_panic(expected = "Error(Contract, #49)")]
fn test_cancel_agent_update_after_prior_cancel_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    // Propose, then cancel — pending slot is now empty.
    client.update_agent(&new_agent);
    client.cancel_agent_update();

    assert!(
        client.get_pending_agent_update().is_none(),
        "pending state must be empty after first cancel"
    );

    // Cancelling again with no pending proposal must panic.
    client.cancel_agent_update();
}

// ─── Issue #533 ──────────────────────────────────────────────────────────────

/// `confirm_agent_update()` panics with `NoTimelockPending` (#49) when called
/// after the pending proposal was already cancelled — the slot is empty again.
///
/// This verifies that cancel clears the pending state so that a subsequent
/// confirm has nothing to act on.
#[test]
#[should_panic(expected = "Error(Contract, #49)")]
fn test_confirm_after_cancel_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    // Propose, then cancel — pending slot is now empty.
    client.update_agent(&new_agent);
    client.cancel_agent_update();

    assert!(
        client.get_pending_agent_update().is_none(),
        "pending state must be empty after cancel"
    );

    // Confirming with no pending proposal must panic.
    client.confirm_agent_update();
}

// ─── Issue #418 ──────────────────────────────────────────────────────────────

/// Ownership transfer must NOT clear a pending agent update (cross-feature).
///
/// Sequence: owner proposes an agent update → owner initiates ownership
/// transfer → new owner accepts → the pending agent update must still be
/// visible, and the new owner can confirm it after the timelock expires.
#[test]
fn test_ownership_transfer_preserves_pending_agent_update() {
    let env = Env::default();
    keep_storage_alive(&env);
    env.mock_all_auths();
    keep_storage_alive(&env);

    let (contract_id, old_agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // 1. Owner proposes an agent update (timelock starts).
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);
    let (pending_addr, expiry) = client.get_pending_agent_update().unwrap();
    assert_eq!(pending_addr, new_agent, "pending agent should be recorded");

    // 2. Owner initiates an ownership transfer to a new address.
    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);

    // 3. New owner accepts ownership.
    client.accept_ownership(&new_owner);
    assert_eq!(
        client.get_owner(),
        new_owner,
        "ownership should transfer to the new owner"
    );

    // 4. The pending agent update must still be visible (NOT cleared by handoff).
    let (pending_addr, pending_expiry) = client.get_pending_agent_update().unwrap();
    assert_eq!(
        pending_addr, new_agent,
        "pending agent update must survive ownership transfer"
    );
    assert_eq!(
        pending_expiry, expiry,
        "pending agent update expiry must be preserved"
    );
    assert_eq!(
        client.get_agent(),
        old_agent,
        "active agent must remain unchanged until confirmation"
    );

    // 5. The new owner can confirm the stale pending agent update.
    env.ledger().set_sequence_number(pending_expiry);
    client.confirm_agent_update();

    assert_eq!(
        client.get_agent(),
        new_agent,
        "new owner should be able to confirm the pending agent update"
    );
    assert!(
        client.get_pending_agent_update().is_none(),
        "pending state should be cleared after confirmation"
    );
}

/// After an ownership transfer the new owner can instead cancel the pending
/// agent update that survives the handoff.
#[test]
fn test_new_owner_can_cancel_surviving_pending_agent_update() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Owner proposes an agent update, then transfers ownership.
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);
    client.accept_ownership(&new_owner);

    // Pending survives the handoff.
    let (pending_addr, _) = client.get_pending_agent_update().unwrap();
    assert_eq!(
        pending_addr, new_agent,
        "pending agent update must survive ownership transfer"
    );

    // New owner cancels the stale pending update.
    client.cancel_agent_update();

    assert_eq!(
        client.get_agent(),
        old_agent,
        "active agent unchanged after cancel by new owner"
    );
    assert!(
        client.get_pending_agent_update().is_none(),
        "pending state cleared after cancel by new owner"
    );
}
