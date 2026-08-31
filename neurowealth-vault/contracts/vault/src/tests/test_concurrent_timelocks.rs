//! Tests for concurrent upgrade and agent update timelocks (Issues #316, #317).
//!
//! Verify that pending upgrades and pending agent updates can coexist without
//! state corruption, and each can be independently confirmed/cancelled. Both
//! use independent two-step timelocks on separate storage keys.

use super::utils::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};

/// Helper to create a fake WASM hash for testing.
fn fake_wasm_hash(env: &Env, seed: u8) -> soroban_sdk::BytesN<32> {
    let mut hash = [seed; 32];
    soroban_sdk::BytesN::from_array(env, &hash)
}

/// Test: Both upgrades and agent updates pending simultaneously.
#[test]
fn test_both_timelocks_pending_simultaneously() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Schedule an upgrade
    let upgrade_hash = fake_wasm_hash(&env, 1);
    client.schedule_upgrade(&owner, &upgrade_hash);
    let (pending_hash, upgrade_expiry) = client
        .get_pending_upgrade()
        .expect("pending upgrade should exist");
    assert_eq!(pending_hash, upgrade_hash);

    // While upgrade is pending, propose an agent update
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);
    let (pending_agent, agent_expiry) = client
        .get_pending_agent_update()
        .expect("pending agent should exist");
    assert_eq!(pending_agent, new_agent);

    // Both should be pending simultaneously
    assert!(client.get_pending_upgrade().is_some());
    assert!(client.get_pending_agent_update().is_some());

    // Expirations should be different (each has its own timelock)
    assert_ne!(upgrade_expiry, agent_expiry);
}

/// Test: Cancel upgrade while agent update remains pending.
#[test]
fn test_cancel_upgrade_agent_update_remains() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Schedule upgrade and propose agent update
    let upgrade_hash = fake_wasm_hash(&env, 2);
    client.schedule_upgrade(&owner, &upgrade_hash);
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // Verify both pending
    assert!(client.get_pending_upgrade().is_some());
    assert!(client.get_pending_agent_update().is_some());

    // Cancel upgrade
    client.cancel_upgrade(&owner);
    assert!(
        client.get_pending_upgrade().is_none(),
        "upgrade should be cancelled"
    );

    // Agent update should still be pending
    let (remaining_agent, _) = client
        .get_pending_agent_update()
        .expect("agent update should still be pending");
    assert_eq!(remaining_agent, new_agent);
}

/// Test: Cancel agent update while upgrade remains pending.
#[test]
fn test_cancel_agent_update_upgrade_remains() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Schedule upgrade and propose agent update
    let upgrade_hash = fake_wasm_hash(&env, 3);
    client.schedule_upgrade(&owner, &upgrade_hash);
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // Verify both pending
    assert!(client.get_pending_upgrade().is_some());
    assert!(client.get_pending_agent_update().is_some());

    // Cancel agent update
    client.cancel_agent_update();
    assert!(
        client.get_pending_agent_update().is_none(),
        "agent update should be cancelled"
    );

    // Upgrade should still be pending
    let (remaining_hash, _) = client
        .get_pending_upgrade()
        .expect("upgrade should still be pending");
    assert_eq!(remaining_hash, upgrade_hash);
}

/// Test: Execute upgrade while agent update is still pending.
#[test]
fn test_execute_upgrade_agent_remains_pending() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Schedule upgrade
    let upgrade_hash = fake_wasm_hash(&env, 4);
    client.schedule_upgrade(&owner, &upgrade_hash);

    // Propose agent update
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // Get expiry for upgrade
    let (_, upgrade_expiry) = client.get_pending_upgrade().unwrap();

    // Extend TTL before advancing ledger (to prevent storage archival)
    env.as_contract(&contract_id, || {
        env.storage().instance().extend_ttl(20000, 20000);
    });

    // Advance past upgrade expiry
    env.ledger().set_sequence_number(upgrade_expiry);

    // Execute upgrade (will fail on fake hash, but that's after the timelock gate)
    let upgrade_result = client.try_execute_upgrade(&owner);
    // We expect it to fail on the WASM hash not being available, not on timelock
    assert!(
        upgrade_result.is_err(),
        "upgrade should fail on WASM hash (after timelock passes)"
    );

    // Agent update should still be pending (independent timelock)
    let (remaining_agent, agent_expiry) = client
        .get_pending_agent_update()
        .expect("agent update should still be pending");
    assert_eq!(remaining_agent, new_agent);
    assert!(
        agent_expiry > upgrade_expiry,
        "agent expiry should be later than upgrade expiry"
    );
}

/// Test: Confirm agent update while upgrade is still pending.
#[test]
fn test_confirm_agent_update_upgrade_remains_pending() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Schedule upgrade
    let upgrade_hash = fake_wasm_hash(&env, 5);
    client.schedule_upgrade(&owner, &upgrade_hash);

    // Propose agent update
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // Get expiry for agent update
    let (_, agent_expiry) = client.get_pending_agent_update().unwrap();

    // Extend TTL before advancing ledger
    env.as_contract(&contract_id, || {
        env.storage().instance().extend_ttl(20000, 20000);
    });

    // Advance past agent expiry
    env.ledger().set_sequence_number(agent_expiry);

    // Confirm agent update
    client.confirm_agent_update();
    assert!(
        client.get_pending_agent_update().is_none(),
        "agent update should be confirmed"
    );
    assert_eq!(client.get_agent(), new_agent, "agent should be updated");

    // Upgrade should still be pending
    let (remaining_hash, _) = client
        .get_pending_upgrade()
        .expect("upgrade should still be pending");
    assert_eq!(remaining_hash, upgrade_hash);
}

/// Test: Independent timelock expiry windows.
/// Schedule upgrade early, then propose agent update later.
/// Advance past upgrade but before agent - only upgrade is executable.
#[test]
fn test_independent_timelock_windows() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Start at ledger 1000
    env.ledger().set_sequence_number(1000);

    // Schedule upgrade (will expire at ~18280)
    let upgrade_hash = fake_wasm_hash(&env, 6);
    client.schedule_upgrade(&owner, &upgrade_hash);
    let (_, upgrade_expiry) = client.get_pending_upgrade().unwrap();
    assert!(upgrade_expiry > 1000);

    // Advance to ledger 10000 (past upgrade expiry but before simulated agent expiry)
    // If we proposed agent now, it would expire around ledger 27280
    env.ledger().set_sequence_number(10000);

    // Now propose agent update
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);
    let (_, agent_expiry) = client.get_pending_agent_update().unwrap();
    assert!(agent_expiry > 10000);
    assert!(
        agent_expiry > upgrade_expiry,
        "agent expiry should be later than upgrade expiry"
    );

    // Extend TTL for safety
    env.as_contract(&contract_id, || {
        env.storage().instance().extend_ttl(30000, 30000);
    });

    // Advance to just past upgrade expiry
    env.ledger().set_sequence_number(upgrade_expiry + 1);

    // Upgrade is now executable (fails on WASM hash)
    let upgrade_result = client.try_execute_upgrade(&owner);
    assert!(
        upgrade_result.is_err(),
        "upgrade should fail on hash (after timelock)"
    );

    // Agent update is NOT yet executable
    let agent_result = client.try_confirm_agent_update();
    assert!(
        agent_result.is_err(),
        "agent update should still be timelocked"
    );

    // Advance to agent expiry
    env.ledger().set_sequence_number(agent_expiry);

    // Now agent is executable
    client.confirm_agent_update();
    assert!(client.get_pending_agent_update().is_none());
    assert_eq!(client.get_agent(), new_agent);
}

/// Test: Duplicate proposals while both pending should fail.
#[test]
#[should_panic(expected = "Error(Contract, #48)")]
fn test_duplicate_upgrade_while_pending_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Schedule upgrade
    let upgrade_hash = fake_wasm_hash(&env, 7);
    client.schedule_upgrade(&owner, &upgrade_hash);

    // Propose agent update
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // Try to schedule another upgrade while one is pending (TimelockAlreadyPending)
    let another_hash = fake_wasm_hash(&env, 8);
    client.schedule_upgrade(&owner, &another_hash);
}

/// Test: Duplicate proposals while both pending should fail (agent).
#[test]
#[should_panic(expected = "Error(Contract, #48)")]
fn test_duplicate_agent_while_pending_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Schedule upgrade
    let upgrade_hash = fake_wasm_hash(&env, 9);
    client.schedule_upgrade(&owner, &upgrade_hash);

    // Propose agent update
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    // Try to propose another agent while one is pending (TimelockAlreadyPending)
    let another_agent = Address::generate(&env);
    client.update_agent(&another_agent);
}

/// Test: Cancel and re-propose independently.
#[test]
fn test_cancel_and_repropse_independently() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Initial proposals
    let upgrade_hash1 = fake_wasm_hash(&env, 10);
    client.schedule_upgrade(&owner, &upgrade_hash1);
    let agent1 = Address::generate(&env);
    client.update_agent(&agent1);

    // Cancel both
    client.cancel_upgrade(&owner);
    client.cancel_agent_update();
    assert!(client.get_pending_upgrade().is_none());
    assert!(client.get_pending_agent_update().is_none());

    // Re-propose different values
    let upgrade_hash2 = fake_wasm_hash(&env, 11);
    client.schedule_upgrade(&owner, &upgrade_hash2);
    let agent2 = Address::generate(&env);
    client.update_agent(&agent2);

    // Verify new proposals are in place
    let (pending_hash, _) = client.get_pending_upgrade().unwrap();
    let (pending_agent, _) = client.get_pending_agent_update().unwrap();
    assert_eq!(pending_hash, upgrade_hash2);
    assert_eq!(pending_agent, agent2);
}
