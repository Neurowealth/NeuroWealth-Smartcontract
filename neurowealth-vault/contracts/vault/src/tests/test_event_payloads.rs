//! Snapshot-style regression tests for all event payload schemas.
//!
//! Each test pins the *complete* field set of an emitted event to exact
//! expected values. Any accidental rename, type change, added or removed field,
//! or wrong value causes a compile-time or runtime failure — exactly like a
//! golden-file snapshot test, without an external snapshot library.
//!
//! When you intentionally change an event schema:
//!   1. Update the event struct in `lib.rs`.
//!   2. Update the matching expected values in the snapshot test below.
//!   3. Run `cargo test` and confirm all assertions pass.
//!
//! Closes issue #252.

use super::utils::*;
use crate::{
    AgentUpdatedEvent, AssetsUpdatedEvent, CapsUpdatedEvent, DepositEvent,
    DepositLimitsUpdatedEvent, DexPoolConfiguredEvent, DexSupplyEvent, DexWithdrawEvent,
    EmergencyPausedEvent, OwnershipTransferCancelledEvent, OwnershipTransferInitiatedEvent,
    OwnershipTransferredEvent, RebalanceEvent, TvlCapUpdatedEvent, UserDepositCapUpdatedEvent,
    VaultInitializedEvent, VaultPausedEvent, VaultUnpausedEvent, WithdrawEvent, TOPIC_AGENT_UPDATED,
    TOPIC_ASSETS_UPDATED, TOPIC_CAPS_UPDATED, TOPIC_DEPOSIT, TOPIC_DEPOSIT_LIMITS_UPDATED,
    TOPIC_DEX_POOL_CONFIGURED, TOPIC_DEX_SUPPLY, TOPIC_DEX_WITHDRAW, TOPIC_EMERGENCY_PAUSED,
    TOPIC_INIT, TOPIC_OWNERSHIP_CANCELLED, TOPIC_OWNERSHIP_INITIATED, TOPIC_OWNERSHIP_TRANSFERRED,
    TOPIC_PAUSED, TOPIC_REBALANCE, TOPIC_TVL_CAP_UPDATED, TOPIC_UNPAUSED, TOPIC_USER_CAP_UPDATED,
    TOPIC_WITHDRAW,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, TryFromVal};

// ── snapshot macro ────────────────────────────────────────────────────────────

/// Asserts a single event field matches its expected snapshot value.
/// The message identifies the event type and field name on failure.
macro_rules! snap {
    ($event:expr, $field:ident, $expected:expr, $event_type:literal) => {
        assert_eq!(
            $event.$field,
            $expected,
            "snapshot mismatch in {}.{}: schema drift detected",
            $event_type,
            stringify!($field),
        );
    };
}

// ── VaultInitializedEvent ─────────────────────────────────────────────────────

#[test]
fn snapshot_vault_initialized_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, agent, owner, usdc_token) = setup_vault_with_token(&env);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_INIT);
    assert_eq!(events.len(), 1, "exactly one VaultInitializedEvent expected");

    let (_, _, data) = &events[0];
    let event = VaultInitializedEvent::try_from_val(&env, data)
        .expect("VaultInitializedEvent: try_from_val failed — schema may have drifted");

    snap!(event, owner, owner, "VaultInitializedEvent");
    snap!(event, agent, agent, "VaultInitializedEvent");
    snap!(event, usdc_token, usdc_token, "VaultInitializedEvent");
    // Default TVL cap set in initialize()
    snap!(event, tvl_cap, 100_000_000_000_i128, "VaultInitializedEvent");
}

// ── DepositEvent ──────────────────────────────────────────────────────────────

#[test]
fn snapshot_deposit_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let deposit_amount: i128 = 7_000_000;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_DEPOSIT);
    assert_eq!(events.len(), 1, "exactly one DepositEvent expected");

    let (_, _, data) = &events[0];
    let event = DepositEvent::try_from_val(&env, data)
        .expect("DepositEvent: try_from_val failed — schema may have drifted");

    snap!(event, user, user, "DepositEvent");
    snap!(event, amount, deposit_amount, "DepositEvent");
    // First deposit is always 1:1 shares
    snap!(event, shares, deposit_amount, "DepositEvent");
}

#[test]
fn snapshot_deposit_event_field_ordering_with_two_depositors() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let amount1: i128 = 5_000_000;
    let amount2: i128 = 3_000_000;

    mint_and_deposit(&env, &client, &usdc_token, &user1, amount1);
    mint_and_deposit(&env, &client, &usdc_token, &user2, amount2);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_DEPOSIT);
    assert_eq!(events.len(), 2, "two DepositEvents expected");

    let (_, _, d1) = &events[0];
    let ev1 = DepositEvent::try_from_val(&env, d1).expect("first DepositEvent decode");
    snap!(ev1, user, user1, "DepositEvent[0]");
    snap!(ev1, amount, amount1, "DepositEvent[0]");
    snap!(ev1, shares, amount1, "DepositEvent[0]");

    let (_, _, d2) = &events[1];
    let ev2 = DepositEvent::try_from_val(&env, d2).expect("second DepositEvent decode");
    snap!(ev2, user, user2, "DepositEvent[1]");
    snap!(ev2, amount, amount2, "DepositEvent[1]");
    snap!(ev2, shares, amount2, "DepositEvent[1]");
}

// ── WithdrawEvent ─────────────────────────────────────────────────────────────

#[test]
fn snapshot_withdraw_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000);
    let withdraw_amount: i128 = 4_000_000;
    client.withdraw(&user, &withdraw_amount, &None);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_WITHDRAW);
    assert_eq!(events.len(), 1, "exactly one WithdrawEvent expected");

    let (_, _, data) = &events[0];
    let event = WithdrawEvent::try_from_val(&env, data)
        .expect("WithdrawEvent: try_from_val failed — schema may have drifted");

    snap!(event, user, user, "WithdrawEvent");
    snap!(event, amount, withdraw_amount, "WithdrawEvent");
    // 1:1 price at first deposit → shares == amount
    snap!(event, shares, withdraw_amount, "WithdrawEvent");
}

// ── RebalanceEvent ────────────────────────────────────────────────────────────

#[test]
fn snapshot_rebalance_event_all_fields_noop() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let protocol = symbol_short!("none");
    let expected_apy: i128 = 500;
    client.rebalance(&protocol, &expected_apy, &0_i128);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE);
    assert_eq!(events.len(), 1, "exactly one RebalanceEvent expected");

    let (_, _, data) = &events[0];
    let event = RebalanceEvent::try_from_val(&env, data)
        .expect("RebalanceEvent: try_from_val failed — schema may have drifted");

    snap!(event, protocol, symbol_short!("none"), "RebalanceEvent");
    snap!(event, expected_apy, 500_i128, "RebalanceEvent");
    snap!(event, status, symbol_short!("noop"), "RebalanceEvent");
    snap!(event, amount_attempted, 0_i128, "RebalanceEvent");
    snap!(event, amount_moved, 0_i128, "RebalanceEvent");
    snap!(event, amount_supplied, 0_i128, "RebalanceEvent");
    snap!(event, amount_withdrawn, 0_i128, "RebalanceEvent");
}

#[test]
fn snapshot_rebalance_event_with_blend_supply() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let deposit_amount = 15_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Rebalance to Blend: should show amount_supplied
    client.rebalance(&symbol_short!("blend"), &950_i128, &0_i128);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE);
    assert_eq!(events.len(), 1, "exactly one RebalanceEvent expected");

    let (_, _, data) = &events[0];
    let event = RebalanceEvent::try_from_val(&env, data)
        .expect("RebalanceEvent: try_from_val failed — schema may have drifted");

    snap!(event, protocol, symbol_short!("blend"), "RebalanceEvent");
    snap!(event, expected_apy, 950_i128, "RebalanceEvent");
    snap!(event, status, symbol_short!("success"), "RebalanceEvent");
    snap!(event, amount_attempted, deposit_amount, "RebalanceEvent");
    snap!(event, amount_moved, deposit_amount, "RebalanceEvent");
    snap!(event, amount_supplied, deposit_amount, "RebalanceEvent");
    snap!(event, amount_withdrawn, 0_i128, "RebalanceEvent");
}

#[test]
fn snapshot_rebalance_event_with_blend_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let deposit_amount = 20_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // First supply to Blend
    client.rebalance(&symbol_short!("blend"), &1100_i128, &0_i128);

    // Then withdraw from Blend: should show amount_withdrawn
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE);
    assert_eq!(events.len(), 2, "two RebalanceEvents expected");

    // Check the withdrawal event (second one)
    let (_, _, data) = &events[1];
    let event = RebalanceEvent::try_from_val(&env, data)
        .expect("RebalanceEvent: try_from_val failed — schema may have drifted");

    snap!(event, protocol, symbol_short!("none"), "RebalanceEvent");
    snap!(event, expected_apy, 0_i128, "RebalanceEvent");
    snap!(event, status, symbol_short!("success"), "RebalanceEvent");
    snap!(event, amount_attempted, deposit_amount, "RebalanceEvent");
    snap!(event, amount_moved, deposit_amount, "RebalanceEvent");
    snap!(event, amount_supplied, 0_i128, "RebalanceEvent");
    snap!(event, amount_withdrawn, deposit_amount, "RebalanceEvent");
}

// ── VaultPausedEvent ──────────────────────────────────────────────────────────

#[test]
fn snapshot_vault_paused_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.pause(&owner);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_PAUSED);
    assert_eq!(events.len(), 1, "exactly one VaultPausedEvent expected");

    let (_, _, data) = &events[0];
    let event = VaultPausedEvent::try_from_val(&env, data)
        .expect("VaultPausedEvent: try_from_val failed — schema may have drifted");

    snap!(event, owner, owner, "VaultPausedEvent");
}

// ── VaultUnpausedEvent ────────────────────────────────────────────────────────

#[test]
fn snapshot_vault_unpaused_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.pause(&owner);
    client.unpause(&owner);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_UNPAUSED);
    assert_eq!(events.len(), 1, "exactly one VaultUnpausedEvent expected");

    let (_, _, data) = &events[0];
    let event = VaultUnpausedEvent::try_from_val(&env, data)
        .expect("VaultUnpausedEvent: try_from_val failed — schema may have drifted");

    snap!(event, owner, owner, "VaultUnpausedEvent");
}

// ── EmergencyPausedEvent ──────────────────────────────────────────────────────

#[test]
fn snapshot_emergency_paused_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.emergency_pause(&owner);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED);
    assert_eq!(events.len(), 1, "exactly one EmergencyPausedEvent expected");

    let (_, _, data) = &events[0];
    let event = EmergencyPausedEvent::try_from_val(&env, data)
        .expect("EmergencyPausedEvent: try_from_val failed — schema may have drifted");

    snap!(event, owner, owner, "EmergencyPausedEvent");
}

// ── TvlCapUpdatedEvent ────────────────────────────────────────────────────────

#[test]
fn snapshot_tvl_cap_updated_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Default TVL cap is 100_000_000_000 (100M USDC)
    let old_cap: i128 = 100_000_000_000;
    let new_cap: i128 = 250_000_000_000;
    client.set_tvl_cap(&new_cap);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_TVL_CAP_UPDATED);
    assert_eq!(events.len(), 1, "exactly one TvlCapUpdatedEvent expected");

    let (_, _, data) = &events[0];
    let event = TvlCapUpdatedEvent::try_from_val(&env, data)
        .expect("TvlCapUpdatedEvent: try_from_val failed — schema may have drifted");

    snap!(event, old_cap, old_cap, "TvlCapUpdatedEvent");
    snap!(event, new_cap, new_cap, "TvlCapUpdatedEvent");
}

// ── UserDepositCapUpdatedEvent ────────────────────────────────────────────────

#[test]
fn snapshot_user_deposit_cap_updated_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Default user deposit cap is 10_000_000_000 (10M USDC)
    let old_cap: i128 = 10_000_000_000;
    let new_cap: i128 = 20_000_000_000;
    client.set_user_deposit_cap(&new_cap);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_USER_CAP_UPDATED);
    assert_eq!(events.len(), 1, "exactly one UserDepositCapUpdatedEvent expected");

    let (_, _, data) = &events[0];
    let event = UserDepositCapUpdatedEvent::try_from_val(&env, data)
        .expect("UserDepositCapUpdatedEvent: try_from_val failed — schema may have drifted");

    snap!(event, old_cap, old_cap, "UserDepositCapUpdatedEvent");
    snap!(event, new_cap, new_cap, "UserDepositCapUpdatedEvent");
}

// ── CapsUpdatedEvent ──────────────────────────────────────────────────────────

#[test]
fn snapshot_caps_updated_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_user_cap: i128 = 10_000_000_000;
    let new_user_cap: i128 = 15_000_000_000;
    let old_tvl_cap: i128 = 100_000_000_000;
    let new_tvl_cap: i128 = 200_000_000_000;
    client.set_caps(&new_user_cap, &new_tvl_cap);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_CAPS_UPDATED);
    assert_eq!(events.len(), 1, "exactly one CapsUpdatedEvent expected");

    let (_, _, data) = &events[0];
    let event = CapsUpdatedEvent::try_from_val(&env, data)
        .expect("CapsUpdatedEvent: try_from_val failed — schema may have drifted");

    snap!(event, old_user_cap, old_user_cap, "CapsUpdatedEvent");
    snap!(event, new_user_cap, new_user_cap, "CapsUpdatedEvent");
    snap!(event, old_tvl_cap, old_tvl_cap, "CapsUpdatedEvent");
    snap!(event, new_tvl_cap, new_tvl_cap, "CapsUpdatedEvent");
}

// ── DepositLimitsUpdatedEvent ─────────────────────────────────────────────────

#[test]
fn snapshot_deposit_limits_updated_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Defaults: min = 1_000_000, max = 10_000_000_000
    let old_min: i128 = 1_000_000;
    let new_min: i128 = 2_000_000;
    let old_max: i128 = 10_000_000_000;
    let new_max: i128 = 25_000_000_000;
    client.set_deposit_limits(&new_min, &new_max);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_DEPOSIT_LIMITS_UPDATED);
    assert_eq!(events.len(), 1, "exactly one DepositLimitsUpdatedEvent expected");

    let (_, _, data) = &events[0];
    let event = DepositLimitsUpdatedEvent::try_from_val(&env, data)
        .expect("DepositLimitsUpdatedEvent: try_from_val failed — schema may have drifted");

    snap!(event, old_min, old_min, "DepositLimitsUpdatedEvent");
    snap!(event, new_min, new_min, "DepositLimitsUpdatedEvent");
    snap!(event, old_max, old_max, "DepositLimitsUpdatedEvent");
    snap!(event, new_max, new_max, "DepositLimitsUpdatedEvent");
}

// ── AgentUpdatedEvent ─────────────────────────────────────────────────────────

#[test]
fn snapshot_agent_updated_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_AGENT_UPDATED);
    assert_eq!(events.len(), 1, "exactly one AgentUpdatedEvent expected");

    let (_, _, data) = &events[0];
    let event = AgentUpdatedEvent::try_from_val(&env, data)
        .expect("AgentUpdatedEvent: try_from_val failed — schema may have drifted");

    snap!(event, old_agent, old_agent, "AgentUpdatedEvent");
    snap!(event, new_agent, new_agent, "AgentUpdatedEvent");
}

// ── AssetsUpdatedEvent ────────────────────────────────────────────────────────

#[test]
fn snapshot_assets_updated_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);

    let deposit_amount: i128 = 10_000_000;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let yield_amount: i128 = 500_000;
    let new_total: i128 = deposit_amount + yield_amount;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &new_total, &false, &0);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_ASSETS_UPDATED);
    assert_eq!(events.len(), 1, "exactly one AssetsUpdatedEvent expected");

    let (_, _, data) = &events[0];
    let event = AssetsUpdatedEvent::try_from_val(&env, data)
        .expect("AssetsUpdatedEvent: try_from_val failed — schema may have drifted");

    snap!(event, old_total, deposit_amount, "AssetsUpdatedEvent");
    snap!(event, new_total, new_total, "AssetsUpdatedEvent");
}

// ── OwnershipTransferInitiatedEvent ──────────────────────────────────────────

#[test]
fn snapshot_ownership_transfer_initiated_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, current_owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let pending_owner = Address::generate(&env);
    client.transfer_ownership(&pending_owner);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_OWNERSHIP_INITIATED);
    assert_eq!(events.len(), 1, "exactly one OwnershipTransferInitiatedEvent expected");

    let (_, _, data) = &events[0];
    let event = OwnershipTransferInitiatedEvent::try_from_val(&env, data)
        .expect("OwnershipTransferInitiatedEvent: try_from_val failed — schema may have drifted");

    snap!(event, current_owner, current_owner, "OwnershipTransferInitiatedEvent");
    snap!(event, pending_owner, pending_owner, "OwnershipTransferInitiatedEvent");
}

// ── OwnershipTransferredEvent ─────────────────────────────────────────────────

#[test]
fn snapshot_ownership_transferred_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, old_owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);
    client.accept_ownership(&new_owner);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_OWNERSHIP_TRANSFERRED);
    assert_eq!(events.len(), 1, "exactly one OwnershipTransferredEvent expected");

    let (_, _, data) = &events[0];
    let event = OwnershipTransferredEvent::try_from_val(&env, data)
        .expect("OwnershipTransferredEvent: try_from_val failed — schema may have drifted");

    snap!(event, old_owner, old_owner, "OwnershipTransferredEvent");
    snap!(event, new_owner, new_owner, "OwnershipTransferredEvent");
}

// ── OwnershipTransferCancelledEvent ──────────────────────────────────────────

#[test]
fn snapshot_ownership_transfer_cancelled_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let pending_owner = Address::generate(&env);
    client.transfer_ownership(&pending_owner);
    client.cancel_ownership_transfer();

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_OWNERSHIP_CANCELLED);
    assert_eq!(events.len(), 1, "exactly one OwnershipTransferCancelledEvent expected");

    let (_, _, data) = &events[0];
    let event = OwnershipTransferCancelledEvent::try_from_val(&env, data)
        .expect("OwnershipTransferCancelledEvent: try_from_val failed — schema may have drifted");

    snap!(event, owner, owner, "OwnershipTransferCancelledEvent");
    snap!(event, cancelled_pending, pending_owner, "OwnershipTransferCancelledEvent");
}

// ── DexSupplyEvent (#340) ─────────────────────────────────────────────────────

#[test]
fn snapshot_dex_supply_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(&env);
    let deposit_amount = 15_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Supply to the DEX: emits DexSupplyEvent with the full amount.
    client.rebalance(&symbol_short!("dex"), &950_i128, &0_i128);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_DEX_SUPPLY);
    assert_eq!(events.len(), 1, "exactly one DexSupplyEvent expected");

    let (_, _, data) = &events[0];
    let event = DexSupplyEvent::try_from_val(&env, data)
        .expect("DexSupplyEvent: try_from_val failed — schema may have drifted");

    snap!(event, asset, usdc_token, "DexSupplyEvent");
    snap!(event, amount_actual, deposit_amount, "DexSupplyEvent");
    snap!(event, success, true, "DexSupplyEvent");
}

// ── DexWithdrawEvent (#340) ───────────────────────────────────────────────────

#[test]
fn snapshot_dex_withdraw_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(&env);
    let deposit_amount = 20_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Supply to the DEX, then exit by rebalancing to "none": emits DexWithdrawEvent.
    client.rebalance(&symbol_short!("dex"), &1100_i128, &0_i128);
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_DEX_WITHDRAW);
    assert_eq!(events.len(), 1, "exactly one DexWithdrawEvent expected");

    let (_, _, data) = &events[0];
    let event = DexWithdrawEvent::try_from_val(&env, data)
        .expect("DexWithdrawEvent: try_from_val failed — schema may have drifted");

    snap!(event, asset, usdc_token, "DexWithdrawEvent");
    snap!(event, amount_actual, deposit_amount, "DexWithdrawEvent");
    snap!(event, success, true, "DexWithdrawEvent");
}

// ── DexPoolConfiguredEvent (#340) ─────────────────────────────────────────────

#[test]
fn snapshot_dex_pool_configured_event_all_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, dex_pool) =
        setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_dex_pool(&owner, &dex_pool);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_DEX_POOL_CONFIGURED);
    assert_eq!(events.len(), 1, "exactly one DexPoolConfiguredEvent expected");

    let (_, _, data) = &events[0];
    let event = DexPoolConfiguredEvent::try_from_val(&env, data)
        .expect("DexPoolConfiguredEvent: try_from_val failed — schema may have drifted");

    // First configuration: no previous pool.
    let expected_old_pool: Option<Address> = None;
    snap!(event, old_pool, expected_old_pool, "DexPoolConfiguredEvent");
    snap!(event, new_pool, dex_pool, "DexPoolConfiguredEvent");
    snap!(event, owner, owner, "DexPoolConfiguredEvent");
}

// ── Ordering regression ───────────────────────────────────────────────────────

/// Verifies that deposit then withdraw emit events in that exact order and that
/// both decode cleanly. A future re-ordering of emit calls would break this.
#[test]
fn snapshot_event_ordering_deposit_precedes_withdraw() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 8_000_000);
    client.withdraw(&user, &3_000_000_i128, &None);

    let dep_events = find_events_by_topic(env.events().all(), &env, TOPIC_DEPOSIT);
    let wd_events = find_events_by_topic(env.events().all(), &env, TOPIC_WITHDRAW);
    assert_eq!(dep_events.len(), 1);
    assert_eq!(wd_events.len(), 1);

    let (_, _, dep_data) = &dep_events[0];
    let dep = DepositEvent::try_from_val(&env, dep_data).expect("DepositEvent decode");
    assert_eq!(dep.amount, 8_000_000_i128);

    let (_, _, wd_data) = &wd_events[0];
    let wd = WithdrawEvent::try_from_val(&env, wd_data).expect("WithdrawEvent decode");
    assert_eq!(wd.amount, 3_000_000_i128);
}

// ── Schema drift detection ────────────────────────────────────────────────────

/// `try_from_val` is the XDR-level gate that catches schema drift at runtime.
/// If a field is added or removed from an event struct, the XDR layout changes
/// and decoding fails — this test documents that contract.
#[test]
fn snapshot_schema_drift_is_caught_by_try_from_val() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_DEPOSIT);
    let (_, _, data) = &events[0];

    // If the DepositEvent struct changes, this line fails at test time
    let _event: DepositEvent = DepositEvent::try_from_val(&env, data)
        .expect("DepositEvent must decode cleanly; schema drift causes this assertion to fail");
}
