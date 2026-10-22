//! Tests for rebalance cooldown (Issue #59)
//!
//! Acceptance criteria:
//!   1. Configurable minimum ledgers between rebalances (owner-set).
//!   2. Agent call within cooldown panics with clear error (Error(Contract, #43)).

use super::utils::*;
use crate::DataKey;
use crate::{RebalanceCooldownUpdatedEvent, TOPIC_REBALANCE_COOLDOWN_UPDATED};
use soroban_sdk::{symbol_short, testutils::Ledger, Env, TryFromVal};

// ============================================================================
// AC-1: Configurable minimum ledgers between rebalances (owner-set)
// ============================================================================

/// Default state: no cooldown is configured; `get_rebalance_cooldown` returns 0.
#[test]
fn test_no_cooldown_by_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(
        client.get_rebalance_cooldown(),
        0,
        "Default cooldown should be 0 (disabled)"
    );
}

/// Owner can set a non-zero interval; getter reflects the new value.
#[test]
fn test_owner_can_set_rebalance_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&100_u32);

    assert_eq!(
        client.get_rebalance_cooldown(),
        100,
        "Cooldown should be 100 ledgers after owner sets it"
    );
}

/// Owner can update the interval to a different value.
#[test]
fn test_owner_can_update_rebalance_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&50_u32);
    assert_eq!(client.get_rebalance_cooldown(), 50);

    client.set_rebalance_cooldown(&200_u32);
    assert_eq!(
        client.get_rebalance_cooldown(),
        200,
        "Cooldown should update to 200 after second set"
    );
}

/// Setting cooldown to 0 disables it; subsequent rebalances are unrestricted.
#[test]
fn test_set_cooldown_zero_disables_it() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // First enable a cooldown
    client.set_rebalance_cooldown(&50_u32);
    assert_eq!(client.get_rebalance_cooldown(), 50);

    // Then disable it
    client.set_rebalance_cooldown(&0_u32);
    assert_eq!(
        client.get_rebalance_cooldown(),
        0,
        "Setting interval to 0 should disable the cooldown"
    );
}

/// Rebalance succeeds when no cooldown is configured (default state).
#[test]
fn test_rebalance_succeeds_without_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Should succeed: no cooldown set
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
}

/// LastRebalanceLedger is 0 before the first call.
#[test]
fn test_last_rebalance_ledger_is_zero_before_first_call() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(
        client.get_last_rebalance_ledger(),
        0,
        "LastRebalanceLedger should be 0 before any rebalance"
    );
}

/// After a successful rebalance, LastRebalanceLedger is updated to the current ledger.
#[test]
fn test_last_rebalance_ledger_updated_after_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let before = env.ledger().sequence();
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);

    let stored = client.get_last_rebalance_ledger();
    assert!(
        stored >= before,
        "LastRebalanceLedger ({stored}) should be >= ledger at call time ({before})"
    );
}

/// Owner can set cooldown to any positive value (e.g. 1, which is the minimum
/// meaningful throttle: two consecutive rebalances in the same ledger are blocked).
#[test]
fn test_owner_can_set_cooldown_to_one_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&1_u32);
    assert_eq!(client.get_rebalance_cooldown(), 1);
}

/// `set_rebalance_cooldown` emits `RebalanceCooldownUpdatedEvent` with the
/// old and new interval so indexers can track cooldown changes without
/// polling storage (Issue #436).
#[test]
fn test_set_rebalance_cooldown_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&720_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE_COOLDOWN_UPDATED);
    assert_eq!(
        events.len(),
        1,
        "exactly one cooldown-updated event expected"
    );

    let (_, _, data) = &events[0];
    let event = RebalanceCooldownUpdatedEvent::try_from_val(&env, data)
        .expect("should be a valid RebalanceCooldownUpdatedEvent");
    assert_eq!(
        event.old_interval, 0,
        "default cooldown before the change is 0"
    );
    assert_eq!(event.new_interval, 720);
}

/// A second call to `set_rebalance_cooldown` reports the previous interval
/// as `old_interval`, not the all-time default.
#[test]
fn test_set_rebalance_cooldown_event_reflects_previous_value() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&50_u32);
    client.set_rebalance_cooldown(&200_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE_COOLDOWN_UPDATED);
    assert_eq!(events.len(), 2, "two cooldown-updated events expected");

    let (_, _, data) = &events[1];
    let event = RebalanceCooldownUpdatedEvent::try_from_val(&env, data)
        .expect("should be a valid RebalanceCooldownUpdatedEvent");
    assert_eq!(event.old_interval, 50);
    assert_eq!(event.new_interval, 200);
}

/// Disabling the cooldown (interval = 0) still emits an event with the
/// correct old/new pair.
#[test]
fn test_set_rebalance_cooldown_to_zero_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&100_u32);
    client.set_rebalance_cooldown(&0_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE_COOLDOWN_UPDATED);
    assert_eq!(events.len(), 2);

    let (_, _, data) = &events[1];
    let event = RebalanceCooldownUpdatedEvent::try_from_val(&env, data)
        .expect("should be a valid RebalanceCooldownUpdatedEvent");
    assert_eq!(event.old_interval, 100);
    assert_eq!(event.new_interval, 0);
}

// ============================================================================
// AC-2: Agent call within cooldown panics with clear error (Error #43)
// ============================================================================

/// Calling rebalance twice in the same ledger when cooldown == 1 panics with
/// `RebalanceCooldownActive` (Error(Contract, #43)).
#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_rebalance_within_cooldown_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set cooldown to 10 ledgers
    client.set_rebalance_cooldown(&10_u32);

    // First rebalance succeeds and stores the current ledger
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);

    // Immediately attempt a second rebalance in the same ledger — must panic
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
}

/// Calling harvest twice in the same ledger when cooldown == 1 panics with
/// `RebalanceCooldownActive` (Error(Contract, #43)).
///
/// Harvest requires a configured pool and non-"none" protocol, so this test
/// first sets up a Blend pool and rebalances into it, then advances the
/// ledger past the cooldown before running the two-harvest sequence.
#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_harvest_within_cooldown_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, _usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set up Blend pool so harvest has a valid protocol to work with.
    client.set_blend_pool(&owner, &blend_pool);
    client.rebalance(&agent, &symbol_short!("blend"), &850, &0_i128);

    // Advance ledger past the cooldown we're about to set, so the first
    // harvest below is not blocked by the rebalance's LastRebalanceLedger.
    let last = client.get_last_rebalance_ledger();
    env.ledger().with_mut(|li| {
        li.sequence_number = last + 20;
    });

    // Set cooldown to 10 ledgers
    client.set_rebalance_cooldown(&10_u32);

    // First harvest succeeds and stores the current ledger
    client.harvest(&agent, &0_i128);

    // Immediately attempt a second harvest in the same ledger — must panic
    client.harvest(&agent, &0_i128);
}

/// `try_rebalance` returns the cooldown error without unwinding the test.
#[test]
fn test_rebalance_within_cooldown_returns_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&10_u32);

    // First call: must succeed
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);

    // Second call: must fail with RebalanceCooldownActive (#43)
    let result = client.try_rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    assert!(
        result.is_err(),
        "Second rebalance within cooldown should fail"
    );
}

/// After cooldown elapses (ledger advances past the interval), rebalance
/// succeeds again.
#[test]
fn test_rebalance_succeeds_after_cooldown_elapses() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set cooldown to 5 ledgers
    let interval = 5_u32;
    client.set_rebalance_cooldown(&interval);

    // First rebalance succeeds
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let last_ledger = client.get_last_rebalance_ledger();

    // Advance ledger past the cooldown window
    env.ledger().with_mut(|li| {
        li.sequence_number = last_ledger + interval;
    });

    // Second rebalance: cooldown has elapsed, should succeed
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);

    // LastRebalanceLedger should be updated again
    let new_last = client.get_last_rebalance_ledger();
    assert!(
        new_last >= last_ledger + interval,
        "LastRebalanceLedger should advance after second successful rebalance"
    );
}

/// Rebalance that fails the cooldown check does NOT update `LastRebalanceLedger`.
#[test]
fn test_failed_cooldown_does_not_update_last_rebalance_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&10_u32);

    // First rebalance succeeds
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let ledger_after_first = client.get_last_rebalance_ledger();

    // Second call fails (cooldown still active)
    let _ = client.try_rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);

    // LastRebalanceLedger must NOT have changed
    assert_eq!(
        client.get_last_rebalance_ledger(),
        ledger_after_first,
        "LastRebalanceLedger must not change when cooldown check fails"
    );
}

/// Cooldown is enforced even when the same ledger as the initial one is used.
#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_cooldown_enforced_on_same_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set cooldown to 1 ledger — two calls in the same ledger should fail
    client.set_rebalance_cooldown(&1_u32);

    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    // elapsed == 0, which is < 1 → panic
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
}

/// Disabling cooldown (interval = 0) after a previous rebalance lets the agent
/// call again immediately.
#[test]
fn test_disabled_cooldown_allows_immediate_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Enable cooldown
    client.set_rebalance_cooldown(&100_u32);
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);

    // Verify it's blocked
    let result = client.try_rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    assert!(result.is_err(), "Should be blocked by cooldown");

    // Disable cooldown
    client.set_rebalance_cooldown(&0_u32);

    // Now should succeed immediately
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
}

/// Exactly-at-boundary: elapsed == interval is allowed (>= means pass).
#[test]
fn test_rebalance_allowed_at_exact_interval_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 5_u32;
    client.set_rebalance_cooldown(&interval);

    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let last = client.get_last_rebalance_ledger();

    // Advance ledger to exactly `last + interval`
    env.ledger().with_mut(|li| {
        li.sequence_number = last + interval;
    });

    // elapsed == interval, which is NOT < interval → should pass
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
}

/// One ledger before the boundary: elapsed == interval - 1 is still blocked.
#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_rebalance_blocked_one_ledger_before_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 5_u32;
    client.set_rebalance_cooldown(&interval);

    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let last = client.get_last_rebalance_ledger();

    // Advance to one ledger short of the interval
    env.ledger().with_mut(|li| {
        li.sequence_number = last + interval - 1;
    });

    // elapsed == interval - 1 < interval → must panic
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
}

// ============================================================================
// AC-3: Additional boundary timing (#250)
// ============================================================================

/// Multiple repeated failed attempts within cooldown must ALL be rejected and
/// must NEVER advance LastRebalanceLedger beyond the first successful call.
#[test]
fn test_multiple_repeated_failures_preserve_last_rebalance_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&50_u32);
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let ledger_after_first = client.get_last_rebalance_ledger();

    for _ in 0..3 {
        let _ = client.try_rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
        assert_eq!(
            client.get_last_rebalance_ledger(),
            ledger_after_first,
            "LastRebalanceLedger must not change on cooldown rejection"
        );
    }
}

/// Advancing to the boundary then calling again starts a new cooldown window;
/// an immediate third call must be blocked by the new window.
#[test]
fn test_sliding_cooldown_window_after_successful_second_call() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 10_u32;
    client.set_rebalance_cooldown(&interval);
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let first_ledger = client.get_last_rebalance_ledger();

    env.ledger().with_mut(|li| {
        li.sequence_number = first_ledger + interval;
    });

    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let second_ledger = client.get_last_rebalance_ledger();
    assert!(second_ledger >= first_ledger + interval);

    // Immediately after the second call the new window is active — must be blocked
    let result = client.try_rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    assert!(
        result.is_err(),
        "third call must be blocked by new cooldown window"
    );
}

// ============================================================================
// AC-4: Concurrent / repeated attempts (#250)
// ============================================================================

/// A very large cooldown blocks rebalances that advance the ledger by a
/// large but still insufficient amount.
#[test]
fn test_very_large_cooldown_blocks_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let huge_interval: u32 = 1_000;
    client.set_rebalance_cooldown(&huge_interval);
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);

    env.ledger().with_mut(|li| {
        li.sequence_number += 500;
    });

    let result = client.try_rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    assert!(
        result.is_err(),
        "must be blocked when elapsed < huge_interval"
    );
}

/// Owner lowering the cooldown mid-window allows the next call sooner.
#[test]
fn test_changing_cooldown_mid_window_applies_to_next_call() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&100_u32);
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let last = client.get_last_rebalance_ledger();

    // Lower the cooldown to 5 while still inside the 100-ledger window
    client.set_rebalance_cooldown(&5_u32);
    assert_eq!(client.get_rebalance_cooldown(), 5);

    env.ledger().with_mut(|li| {
        li.sequence_number = last + 5;
    });

    // elapsed (5) >= new_interval (5) → must succeed
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
}

// ============================================================================
// AC-5: Pause / unpause interaction (#250)
// ============================================================================

/// Pausing does not reset LastRebalanceLedger or the cooldown interval.
/// After unpausing the same cooldown window is still in effect.
#[test]
fn test_cooldown_survives_pause_unpause_cycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 20_u32;
    client.set_rebalance_cooldown(&interval);
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let last_before_pause = client.get_last_rebalance_ledger();

    client.pause(&owner);

    assert_eq!(client.get_last_rebalance_ledger(), last_before_pause);
    assert_eq!(client.get_rebalance_cooldown(), interval);

    client.unpause(&owner);

    // Still within the original window — must be blocked
    let result = client.try_rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    assert!(
        result.is_err(),
        "cooldown must still be active after unpause"
    );
}

/// After pause/unpause the rebalance succeeds once the cooldown elapses.
#[test]
fn test_rebalance_allowed_after_unpause_and_cooldown_elapsed() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 15_u32;
    client.set_rebalance_cooldown(&interval);
    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    let last = client.get_last_rebalance_ledger();

    client.pause(&owner);
    client.unpause(&owner);

    env.ledger().with_mut(|li| {
        li.sequence_number = last + interval;
    });

    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    assert!(client.get_last_rebalance_ledger() >= last + interval);
}
