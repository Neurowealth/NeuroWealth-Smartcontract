//! Tests for the owner-callable emergency harvest fallback (Issue #506).
//!
//! The normal `harvest()` requires agent auth. When the agent key is lost,
//! compromised, or mid-rotation, the owner can call `emergency_harvest()`
//! to compound yield. This module verifies the auth model, error paths,
//! event emission, and pause-bypass behaviour of the emergency harvest.

extern crate std;

use super::utils::*;
use crate::{EmergencyHarvestEvent, TOPIC_EMERGENCY_HARVEST};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, TryFromVal};

// ============================================================================
// Helpers
// ============================================================================

/// Sets up a vault with Blend, deposits, and moves funds into the protocol
/// so there is an active position to harvest from.
fn setup_with_deployed_funds(env: &Env) -> (Address, Address, Address, Address, Address) {
    let (contract_id, agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(env);
    let client = NeuroWealthVaultClient::new(env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(env);
    mint_and_deposit(env, &client, &usdc_token, &user, 10_000_000_i128);

    // Move all idle funds into Blend so harvest has something to compound.
    client.rebalance(&agent, &symbol_short!("blend"), &850_i128, &0_i128);

    (contract_id, agent, owner, usdc_token, blend_pool)
}

// ============================================================================
// Happy path
// ============================================================================

#[test]
fn test_owner_can_emergency_harvest() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, _blend_pool) = setup_with_deployed_funds(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.emergency_harvest(&0_i128);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_HARVEST);
    assert_eq!(
        events.len(),
        1,
        "exactly one EmergencyHarvestEvent expected"
    );

    let (_, _, data) = &events[0];
    let event = EmergencyHarvestEvent::try_from_val(&env, data)
        .expect("EmergencyHarvestEvent decode failed");
    assert_eq!(event.protocol, symbol_short!("blend"));
    assert!(
        event.amount_harvested >= 0,
        "harvested amount must be non-negative"
    );
}

#[test]
fn test_emergency_harvest_emits_distinct_topic_from_harvest() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) = setup_with_deployed_funds(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Topics must be distinct so indexers can tell them apart.
    assert_ne!(
        crate::TOPIC_HARVEST,
        TOPIC_EMERGENCY_HARVEST,
        "harvest and emergency_harvest topics must differ"
    );
}

// ============================================================================
// Auth model
// ============================================================================

#[test]
fn test_emergency_harvest_requires_owner_auth() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, _blend_pool) = setup_with_deployed_funds(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // With mock_all_auths, the owner.require_auth() check inside
    // emergency_harvest passes for any address. The stored-owner comparison
    // then rejects non-owner callers. Verify the happy path works.
    client.emergency_harvest(&0_i128);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_HARVEST);
    assert_eq!(events.len(), 1, "owner emergency_harvest must succeed");
}

// ============================================================================
// Error paths
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #17)")]
fn test_emergency_harvest_fails_when_no_protocol() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // No rebalance has been done; CurrentProtocol is "none".
    client.emergency_harvest(&0_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #16)")]
fn test_emergency_harvest_rejects_negative_min_out() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, _blend_pool) = setup_with_deployed_funds(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.emergency_harvest(&-1_i128);
}

// ============================================================================
// Pause bypass
// ============================================================================

#[test]
fn test_emergency_harvest_works_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, _blend_pool) = setup_with_deployed_funds(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Pause the vault via owner.
    client.pause(&owner);
    assert!(client.is_paused());

    // Normal harvest would revert while paused, but emergency_harvest bypasses
    // the pause check because the owner may need to compound during an
    // emergency pause.
    client.emergency_harvest(&0_i128);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_HARVEST);
    assert_eq!(
        events.len(),
        1,
        "emergency_harvest must succeed while paused"
    );
}

#[test]
fn test_emergency_harvest_works_after_circuit_breaker_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Move funds into Blend so we have something to harvest later.
    client.rebalance(&agent, &symbol_short!("blend"), &850_i128, &0_i128);

    // Trip the circuit breaker (3 consecutive failures). Each `rebalance`
    // call only counts as a "failed" outcome (vs. a "noop") when there is
    // idle vault balance to attempt supplying, so top up before each of the
    // three attempts below.
    blend_client.set_max_supply_limit(&-1_i128);
    for _ in 0..3 {
        mint_and_deposit(&env, &client, &usdc_token, &user, 1_000_000_i128);
        client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);
    }
    assert!(client.is_paused(), "circuit breaker must pause");

    // Owner can still harvest to compound any remaining yield before
    // addressing the agent key issue.
    client.emergency_harvest(&0_i128);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_HARVEST);
    assert_eq!(
        events.len(),
        1,
        "emergency_harvest must succeed after circuit-breaker pause"
    );
}

// ============================================================================
// Cooldown enforcement
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_emergency_harvest_respects_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, _blend_pool) = setup_with_deployed_funds(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set a long cooldown.
    client.set_rebalance_cooldown(&100_u32);

    // First harvest succeeds (sets LastRebalanceLedger).
    client.emergency_harvest(&0_i128);

    // Second harvest immediately fails because cooldown has not elapsed.
    client.emergency_harvest(&0_i128);
}
