//! Tests for the timelocked migration target change (#969).
//!
//! `set_migration_target` used to take effect immediately, so a compromised
//! owner key could point `migrate_shares` at an attacker contract. It is now a
//! two-step flow with the same delay as `schedule_upgrade`:
//!
//!   1. `schedule_migration_target(owner, target)`
//!   2. `execute_migration_target(owner)` once the timelock has passed
//!   3. `cancel_migration_target(owner)` any time before execution

use super::utils::*;
use crate::{
    MigrationTargetCancelledEvent, MigrationTargetScheduledEvent, MigrationTargetUpdatedEvent,
};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, TryFromVal};

fn setup(env: &Env) -> (NeuroWealthVaultClient<'_>, Address) {
    env.mock_all_auths();
    // Keep instance storage alive across the timelock jumps.
    env.ledger().with_mut(|li| {
        li.min_persistent_entry_ttl = 100_000;
        li.min_temp_entry_ttl = 100_000;
        li.max_entry_ttl = 200_000;
    });
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(env);
    (NeuroWealthVaultClient::new(env, &contract_id), owner)
}

#[test]
fn test_no_pending_migration_target_initially() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    assert_eq!(client.get_pending_migration_target(), None);
}

#[test]
fn test_schedule_records_pending_target() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let target = Address::generate(&env);

    let now = env.ledger().sequence();
    client.schedule_migration_target(&owner, &target);

    let (pending, effective_ledger) = client.get_pending_migration_target().unwrap();
    assert_eq!(pending, target);
    assert_eq!(effective_ledger, now + 17_280);
}

#[test]
#[should_panic(expected = "Error(Contract, #50)")]
fn test_execute_before_timelock_rejected() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let target = Address::generate(&env);

    client.schedule_migration_target(&owner, &target);
    client.execute_migration_target(&owner);
}

#[test]
#[should_panic(expected = "Error(Contract, #50)")]
fn test_execute_one_ledger_early_rejected() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let target = Address::generate(&env);

    client.schedule_migration_target(&owner, &target);
    let (_, effective_ledger) = client.get_pending_migration_target().unwrap();
    env.ledger().set_sequence_number(effective_ledger - 1);
    client.execute_migration_target(&owner);
}

#[test]
#[should_panic(expected = "Error(Contract, #49)")]
fn test_execute_without_schedule_rejected() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    client.execute_migration_target(&owner);
}

#[test]
#[should_panic(expected = "Error(Contract, #48)")]
fn test_double_schedule_rejected() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    client.schedule_migration_target(&owner, &Address::generate(&env));
    client.schedule_migration_target(&owner, &Address::generate(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #34)")]
fn test_schedule_by_non_owner_rejected() {
    let env = Env::default();
    let (client, _owner) = setup(&env);
    let stranger = Address::generate(&env);
    client.schedule_migration_target(&stranger, &Address::generate(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #34)")]
fn test_execute_by_non_owner_rejected() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    client.schedule_migration_target(&owner, &Address::generate(&env));
    let (_, effective_ledger) = client.get_pending_migration_target().unwrap();
    env.ledger().set_sequence_number(effective_ledger);
    client.execute_migration_target(&Address::generate(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #34)")]
fn test_cancel_by_non_owner_rejected() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    client.schedule_migration_target(&owner, &Address::generate(&env));
    client.cancel_migration_target(&Address::generate(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #49)")]
fn test_cancel_without_schedule_rejected() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    client.cancel_migration_target(&owner);
}

#[test]
fn test_cancel_clears_pending_and_blocks_execute() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let target = Address::generate(&env);

    client.schedule_migration_target(&owner, &target);
    client.cancel_migration_target(&owner);
    assert_eq!(client.get_pending_migration_target(), None);

    let found = find_events_by_topic(
        env.events().all(),
        &env,
        crate::TOPIC_MIGRATION_TARGET_CANCELLED,
    );
    assert_eq!(found.len(), 1);
    let (_, _, data) = &found[0];
    let ev = MigrationTargetCancelledEvent::try_from_val(&env, data).unwrap();
    assert_eq!(ev.cancelled_target, target);
    assert_eq!(ev.owner, owner);

    // Even after the original delay, there is nothing left to execute.
    env.ledger()
        .set_sequence_number(env.ledger().sequence() + 17_280);
    assert!(client.try_execute_migration_target(&owner).is_err());
}

#[test]
fn test_full_lifecycle() {
    let env = Env::default();
    let (client, owner) = setup(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    // Schedule
    client.schedule_migration_target(&owner, &first);
    let found = find_events_by_topic(
        env.events().all(),
        &env,
        crate::TOPIC_MIGRATION_TARGET_SCHEDULED,
    );
    assert_eq!(found.len(), 1);
    let (_, _, data) = &found[0];
    let ev = MigrationTargetScheduledEvent::try_from_val(&env, data).unwrap();
    assert_eq!(ev.new_target, first);
    let effective_ledger = ev.effective_ledger;

    // Execute once the timelock has passed
    env.ledger().set_sequence_number(effective_ledger);
    client.execute_migration_target(&owner);
    assert_eq!(client.get_pending_migration_target(), None);

    let found = find_events_by_topic(
        env.events().all(),
        &env,
        crate::TOPIC_MIGRATION_TARGET_UPDATED,
    );
    assert_eq!(found.len(), 1);
    let (_, _, data) = &found[0];
    let ev = MigrationTargetUpdatedEvent::try_from_val(&env, data).unwrap();
    assert_eq!(ev.old_target, None);
    assert_eq!(ev.new_target, first);

    // A second change goes through the same delay and reports the old target.
    client.schedule_migration_target(&owner, &second);
    let (_, effective_ledger) = client.get_pending_migration_target().unwrap();
    env.ledger().set_sequence_number(effective_ledger);
    client.execute_migration_target(&owner);

    let found = find_events_by_topic(
        env.events().all(),
        &env,
        crate::TOPIC_MIGRATION_TARGET_UPDATED,
    );
    let (_, _, data) = &found[found.len() - 1];
    let ev = MigrationTargetUpdatedEvent::try_from_val(&env, data).unwrap();
    assert_eq!(ev.old_target, Some(first));
    assert_eq!(ev.new_target, second);
}
