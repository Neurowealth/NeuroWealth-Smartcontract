//! Tests for the consecutive-failure circuit breaker (Issue #439).
//!
//! A rebalance that completes with a `"failed"` status increments a counter;
//! any `"success"` resets it. When the counter reaches
//! `max_consecutive_failures` the vault auto-pauses (reusing the emergency-pause
//! flag) and emits an `EmergencyPausedEvent`.
//!
//! A `"failed"` status is produced by configuring the Blend pool to accept zero
//! supply (`set_max_supply_limit(-1)`), which makes the supply leg move nothing
//! while idle funds remain in the vault.

use super::utils::*;
use crate::{
    EmergencyPausedEvent, MaxConsecutiveFailuresUpdatedEvent, DEFAULT_MAX_CONSECUTIVE_FAILURES,
    TOPIC_EMERGENCY_PAUSED, TOPIC_MAX_FAILURES_UPDATED,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, TryFromVal};

/// Deploys a vault, configures Blend, deposits `amount`, and forces the Blend
/// pool to reject all supply so every `rebalance("blend")` fails.
fn setup_failing_blend(env: &Env, amount: i128) -> (NeuroWealthVaultClient<'_>, Address, Address) {
    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(env);
    let client = NeuroWealthVaultClient::new(env, &contract_id);
    let blend_client = MockBlendPoolClient::new(env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);
    blend_client.set_max_supply_limit(&-1_i128); // supply moves 0 -> "failed"

    let user = Address::generate(env);
    mint_and_deposit(env, &client, &usdc_token, &user, amount);

    (client, owner, blend_pool)
}

#[test]
fn test_default_threshold_is_three() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(
        client.get_max_consecutive_failures(),
        3,
        "default circuit-breaker threshold should be 3"
    );
    assert_eq!(client.get_consecutive_failures(), 0);
}

#[test]
fn test_auto_pause_after_default_threshold_failures() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner, _blend_pool) = setup_failing_blend(&env, 10_000_000_i128);

    let emerg_before = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED).len();

    // Failure 1 and 2 must not pause.
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 1);
    assert!(!client.is_paused(), "one failure must not pause");

    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 2);
    assert!(!client.is_paused(), "two failures must not pause");

    // Failure 3 trips the breaker.
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 3);
    assert!(client.is_paused(), "threshold reached -> auto-paused");

    // Exactly one new EmergencyPausedEvent, carrying the stored owner.
    let emerg_events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED);
    assert_eq!(
        emerg_events.len(),
        emerg_before + 1,
        "auto-pause must emit one EmergencyPausedEvent"
    );
    let (_, _, data) = emerg_events.last().unwrap();
    let event =
        EmergencyPausedEvent::try_from_val(&env, data).expect("EmergencyPausedEvent decode failed");
    assert_eq!(event.owner, owner, "event owner must be the stored owner");

    // Once paused, further rebalances revert.
    let res = client.try_rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert!(
        res.is_err(),
        "rebalance must fail while the vault is paused"
    );
}

#[test]
fn test_custom_threshold_via_setter() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _blend_pool) = setup_failing_blend(&env, 10_000_000_i128);

    // Tighten the breaker to 2 consecutive failures.
    client.set_max_consecutive_failures(&2);
    assert_eq!(client.get_max_consecutive_failures(), 2);

    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert!(!client.is_paused(), "one failure below custom threshold");

    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert!(
        client.is_paused(),
        "second failure trips the custom threshold"
    );
}

#[test]
fn test_success_between_failures_resets_counter() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Two failures.
    blend_client.set_max_supply_limit(&-1_i128);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 2);
    assert!(!client.is_paused());

    // A success resets the counter to zero.
    blend_client.set_max_supply_limit(&0_i128); // no limit -> full supply succeeds
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(
        client.get_consecutive_failures(),
        0,
        "a successful rebalance must reset the failure counter"
    );

    // Re-arm failures: deposit fresh idle funds and fail twice more. Because the
    // counter was reset, two failures leave it at 2 (< threshold 3) and the vault
    // stays unpaused. Without the reset it would have reached 4 and paused.
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);
    blend_client.set_max_supply_limit(&-1_i128);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 2);
    assert!(
        !client.is_paused(),
        "reset must prevent the pause that would occur without it"
    );
}

#[test]
fn test_set_max_consecutive_failures_rejects_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let res = client.try_set_max_consecutive_failures(&0);
    assert!(res.is_err(), "threshold of 0 must be rejected");
}

// ============================================================================
// Issue #507 – lowering threshold below current count triggers pause
// ============================================================================

#[test]
fn test_lowering_threshold_triggers_pause_on_next_failure() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, _blend_pool) = setup_failing_blend(&env, 10_000_000_i128);

    // Default threshold is 3. Accumulate 2 failures (below threshold).
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 1);
    assert!(!client.is_paused());

    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 2);
    assert!(!client.is_paused());

    // Lower the threshold to 2, which equals the current count.
    client.set_max_consecutive_failures(&2);
    assert_eq!(client.get_max_consecutive_failures(), 2);

    // The very next failure pushes the counter to 3, which is >= the new
    // threshold of 2, so the vault must auto-pause on this call.
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 3);
    assert!(
        client.is_paused(),
        "lowering threshold to at/below current count must trigger auto-pause on next failure"
    );
}

/// `set_max_consecutive_failures` must leave an audit trail: the first
/// configuration reports the built-in default as `old_threshold`.
#[test]
fn test_set_max_consecutive_failures_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_max_consecutive_failures(&5);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_MAX_FAILURES_UPDATED);
    assert_eq!(
        events.len(),
        1,
        "exactly one max-failures-updated event expected"
    );

    let (_, _, data) = &events[0];
    let event = MaxConsecutiveFailuresUpdatedEvent::try_from_val(&env, data)
        .expect("should be a valid MaxConsecutiveFailuresUpdatedEvent");
    assert_eq!(
        event.old_threshold, DEFAULT_MAX_CONSECUTIVE_FAILURES,
        "old_threshold before any explicit configuration is the default"
    );
    assert_eq!(event.new_threshold, 5);
}

/// A second call reports the previously configured threshold as
/// `old_threshold`, not the all-time default.
#[test]
fn test_set_max_consecutive_failures_event_reflects_previous_value() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_max_consecutive_failures(&5);
    client.set_max_consecutive_failures(&2);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_MAX_FAILURES_UPDATED);
    assert_eq!(events.len(), 2);

    let (_, _, data) = &events[1];
    let event = MaxConsecutiveFailuresUpdatedEvent::try_from_val(&env, data)
        .expect("should be a valid MaxConsecutiveFailuresUpdatedEvent");
    assert_eq!(event.old_threshold, 5);
    assert_eq!(event.new_threshold, 2);
}
