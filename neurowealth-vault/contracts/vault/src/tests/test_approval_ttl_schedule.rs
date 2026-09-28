//! Tests for the approval-expiry readiness event (Issue #847).
//!
//! `set_approval_ttl` and the legacy `set_blend_approval_ttl` now publish
//! [`ProtocolApprovalScheduledEvent`] (`ttl_sch`) alongside the existing
//! `ApprovalTtlUpdatedEvent`. The event lets operators compute a renewal
//! schedule from the payload alone, without replaying transaction history.

use super::utils::*;
use crate::{
    APPROVAL_RENEWAL_LEAD_LEDGERS, ProtocolApprovalScheduledEvent, TOPIC_APPROVAL_TTL_SCHEDULED,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Env, TryFromVal,
};

/// Reads the single `ttl_sch` event emitted so far, failing if there is not
/// exactly one.
fn only_schedule_event(env: &Env) -> ProtocolApprovalScheduledEvent {
    let events = find_events_by_topic(env.events().all(), env, TOPIC_APPROVAL_TTL_SCHEDULED);
    assert_eq!(
        events.len(),
        1,
        "expected exactly one approval-schedule event, got {}",
        events.len()
    );
    let (_, _, data) = &events[0];
    ProtocolApprovalScheduledEvent::try_from_val(env, data)
        .expect("should be a valid ProtocolApprovalScheduledEvent")
}

/// The payload describes the window the next approval will get: expiry,
/// usable window and renewal deadline are all derivable from the event.
#[test]
fn test_set_approval_ttl_emits_expiry_schedule() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let ttl = 50_000_u32;
    let current_ledger = env.ledger().sequence();
    client.set_approval_ttl(&ttl);

    let event = only_schedule_event(&env);

    assert_eq!(
        event.protocol,
        symbol_short!("both"),
        "the shared setter governs every protocol approval"
    );
    assert_eq!(event.current_ledger, current_ledger);
    assert_eq!(event.approval_ttl, ttl);
    assert_eq!(event.expiry_ledger, current_ledger + ttl);
    assert_eq!(
        event.available_window, ttl,
        "the full TTL is usable from the emitting ledger onwards"
    );
    assert_eq!(event.lead_time, APPROVAL_RENEWAL_LEAD_LEDGERS);
    assert_eq!(
        event.renewal_deadline_ledger,
        current_ledger + ttl - APPROVAL_RENEWAL_LEAD_LEDGERS
    );
}

/// The announced expiry ledger must equal the ledger the approve paths will
/// actually use, i.e. `sequence + get_approval_ttl()` (#847 acceptance).
#[test]
fn test_schedule_expiry_matches_persisted_approval_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    for ttl in [17_280_u32, 120_000_u32, 500_000_u32] {
        client.set_approval_ttl(&ttl);

        let event = only_schedule_event(&env);
        assert_eq!(
            event.expiry_ledger,
            env.ledger().sequence() + client.get_approval_ttl(),
            "event expiry must match the persisted approval window"
        );
        assert_eq!(event.approval_ttl, client.get_approval_ttl());
    }
}

/// Renewal lead time must stay inside the window, so a renewal transaction
/// submitted at the deadline still lands before expiry.
#[test]
fn test_renewal_deadline_leaves_lead_time_headroom() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let ttl = 30_000_u32;
    client.set_approval_ttl(&ttl);

    let event = only_schedule_event(&env);
    assert!(
        event.lead_time < event.approval_ttl,
        "lead time ({}) must be shorter than the window ({})",
        event.lead_time,
        event.approval_ttl
    );
    assert_eq!(
        event.renewal_deadline_ledger + event.lead_time,
        event.expiry_ledger,
        "deadline + lead time must reconstruct the expiry ledger"
    );
    assert!(
        event.renewal_deadline_ledger >= event.current_ledger,
        "the deadline must not be in the past"
    );
}

/// A TTL shorter than the configured lead time (the 1,000-ledger minimum is
/// such a case) reports the whole window as lead time and a deadline of "now".
#[test]
fn test_ttl_shorter_than_lead_time_reports_full_window() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let short_ttl = 1_000_u32;
    assert!(
        short_ttl < APPROVAL_RENEWAL_LEAD_LEDGERS,
        "this test relies on the minimum TTL being shorter than the lead time"
    );
    client.set_approval_ttl(&short_ttl);

    let event = only_schedule_event(&env);
    assert_eq!(event.lead_time, short_ttl);
    assert_eq!(event.renewal_deadline_ledger, event.current_ledger);
    assert_eq!(event.expiry_ledger, event.current_ledger + short_ttl);
}

/// The legacy Blend-only setter announces the same schedule, scoped to the
/// Blend approval.
#[test]
fn test_blend_approval_ttl_emits_schedule_scoped_to_blend() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let ttl = 50_000_u32;
    client.set_blend_approval_ttl(&owner, &ttl);

    let event = only_schedule_event(&env);
    assert_eq!(event.protocol, symbol_short!("blend"));
    assert_eq!(event.approval_ttl, ttl);
    assert_eq!(event.expiry_ledger, env.ledger().sequence() + ttl);
    assert_eq!(event.renewal_deadline_ledger, event.expiry_ledger - event.lead_time);
}

/// Publishing the schedule must not extend or renew anything: no pool
/// allowance is created and the persisted TTL is exactly what the owner set.
#[test]
fn test_schedule_event_has_no_approval_side_effect() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    let ttl = 60_000_u32;
    client.set_approval_ttl(&ttl);

    let _event = only_schedule_event(&env);

    assert_eq!(
        client.get_approval_ttl(),
        ttl,
        "the event must not renew or extend the configured window"
    );
    assert_eq!(
        token_client.allowance_expiration(&contract_id, &blend_pool),
        0,
        "announcing the schedule must not create or extend a pool approval"
    );
}

/// Each configuration call publishes its own schedule, so a monitor can
/// recompute the renewal deadline after every change.
#[test]
fn test_each_ttl_change_publishes_its_own_schedule() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_approval_ttl(&20_000_u32);
    env.ledger().set_sequence_number(500);
    client.set_approval_ttl(&40_000_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_APPROVAL_TTL_SCHEDULED);
    assert_eq!(events.len(), 2, "one schedule event per configuration call");

    let (_, _, data) = &events[1];
    let second = ProtocolApprovalScheduledEvent::try_from_val(&env, data)
        .expect("should be a valid ProtocolApprovalScheduledEvent");

    assert_eq!(second.current_ledger, 500);
    assert_eq!(second.approval_ttl, 40_000);
    assert_eq!(second.expiry_ledger, 500 + 40_000);
    assert_eq!(second.renewal_deadline_ledger, 500 + 35_000);
}

/// A rejected configuration call changes nothing and must not announce a
/// schedule.
#[test]
fn test_rejected_ttl_change_emits_no_schedule() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let _ = client.try_set_approval_ttl(&10_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_APPROVAL_TTL_SCHEDULED);
    assert!(
        events.is_empty(),
        "a rejected set_approval_ttl must not publish a schedule"
    );
}

/// The renewal deadline is what an operator acts on, so it must move with the
/// ledger: monitoring can schedule renewals from the event alone.
#[test]
fn test_monitor_can_schedule_renewal_from_event_alone() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let ttl = 100_000_u32;
    client.set_approval_ttl(&ttl);

    let event = only_schedule_event(&env);

    // Everything a scheduler needs is present in the payload.
    assert_eq!(event.approval_ttl, client.get_approval_ttl());
    assert_eq!(
        event.available_window,
        event.expiry_ledger - event.current_ledger
    );

    // Lead time in ledgers (~7 h at ~5 s per ledger) is published so operators
    // do not have to hard-code it.
    assert_eq!(event.lead_time, 5_000);
    assert_eq!(event.lead_time, APPROVAL_RENEWAL_LEAD_LEDGERS);
}
