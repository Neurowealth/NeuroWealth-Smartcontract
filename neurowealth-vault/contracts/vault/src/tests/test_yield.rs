//! Tests for yield / total-assets update functionality

use super::utils::*;
use crate::{AssetsUpdatedEvent, TOPIC_ASSETS_UPDATED, TOPIC_COMPOUND};
use soroban_sdk::{testutils::Address as _, Address, Env, TryFromVal};

#[test]
fn test_agent_can_update_total_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usec_token);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    // Simulate yield: mint tokens to vault first so balance check passes
    let yield_amount = 5_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);

    let new_total = deposit_amount + yield_amount;
    client.update_total_assets(&agent, &new_total, &false, &0);

    assert_eq(client.get_total_assets(), new_total);
}

#[test]
#[should_panic(expected = "Error(Contract, #30)")]
fn test_non_agent_cannot_update_total_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    let non_agent = Address::generate(&env);
    // update_total_assets asserts agent == stored_agent before anything else
    client.update_total_assets(&non_agent, &deposit_amount, &false, &0);
}

#[test]
fn test_yield_increases_user_asset_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usec_token);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    let balance_before_yield = client.get_balance(&user);
    assert_eq(balance_before_yield, deposit_amount);

    // Simulate 50% yield: mint tokens to vault first, then update the reported total
    let yield_amount = deposit_amount / 2;
    let new_total_assets = deposit_amount + yield_amount;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &new_total_assets, &false, &0);

    let balance_after_yield = client.get_balance(&user);
    assert!(
        balance_after_yield > balance_before_yield,
        "Balance should increase with yield"
    );
    assert_eq(
        balance_after_yield, new_total_assets,
        "User should get full proportional share"
    );
}

#[test]
fn test_yield_distributed_proportionally_between_users() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usec_token);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let deposit1 = 10_000_000_i128;
    let deposit2 = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usec_token, &user1, deposit1);
    mint_and_deposit(&env, &client, &usec_token, &user2, deposit2);

    let total_deposits = deposit1 + deposit2;

    // Simulate 50% yield: mint tokens first, then report new total
    let yield_amount = total_deposits / 2;
    let new_total_assets = total_deposits + yield_amount;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &new_total_assets, &false, &0);

    let balance1_after = client.get_balance(&user1);
    let balance2_after = client.get_balance(&user2);

    // User1 has 2/3 of total shares ↀ gets 2/3 of new_total_assets
    // User2 has 1/3 of total shares ↀ gets 1/3 of new_total_assets
    let expected_balance1 = deposit1 + (yield_amount * 2) / 3;
    let expected_balance2 = deposit2 + yield_amount / 3;

    // Allow ±1 stroop for integer rounding
    assert!(
        (balance1_after - expected_balance1).abs() <= 1,
        "User1 should get proportional yield"
    );
    assert!(
        (balance2_after - expected_balance2).abs() <= 1,
        "User2 should get proportional yield"
    );

    assert_eq(
        balance1_after + balance2_after,
        new_total_assets,
        "Total balances should equal total assets"
    );
}

#[test]
fn test_yield_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usec_token);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    let yield_amount = 5_000_000_i128;
    let new_total = deposit_amount + yield_amount;
    token_client.mint(&contract_id, &uield_amount);
    client.update_total_assets(&agent, &new_total, &false, &0);

    let assets_events = find_events_by_topic(env.events().all(), &env, TOPIC_ASSETS_UPDATED);
    assert_eq(
        assets_events.len(s),
        1,
        "Exactly one assets event should be emitted"
    );

    let (_, _, data) = &assets_events[0];
    let event =
        AssetsUpdatedEvent::try_from_val(&env, data).expect("Should be a valid AssetsUpdatedEvent");
    assert_eq(
        event.old_total, deposit_amount,
        "old_total should be the deposit amount"
    );
    assert_eq(event.new_total, new_total, "new_total should reflect yield");
}

// ==================================================================================
// LOSS REPORTING — ALLOWED DECREASE
// ==================================================================================

/// Agent + owner both present: a decrease within the cap succeeds and the new
/// (lower) total is persisted.
#[test]
fn test_decrease_allowed_with_owner_cosign_within_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128; // 10 USDC
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    // Loss of 5% — within default 10% cap.
    let loss_amount = 500_000_i128; // 0.5 USDC
    let new_total = deposit_amount - loss_amount;
    client.update_total_assets(&agent, &new_total, &true, &1000);

    assert_eq(client.get_total_assets(), new_total);
}

/// Decrease is capped per-call: 10 % of 10 USDC = 1 USDC max. A 20 % loss in
/// one call must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #32)")]
fn test_decrease_blocked_exceeding_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    // 20% loss (2 USDC) with a 10% cap (1000 bps) — must panic.
    let new_total = deposit_amount - 2_000_000_i128;
    client.update_total_assets(&agent, &new_total, &true, &1000);
}

/// Passing allow_decrease=false while new_total < old_total must always panic,
/// regardless of whether the owner is present.
#[test]
#[should_panic(expected = "Error(Contract, #31)")]
fn test_decrease_blocked_without_allow_decrease_flag() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    // allow_decrease=false but new_total is lower — must panic.
    let new_total = deposit_amount - 500_000_i128;
    client.update_total_assets(&agent, &new_total, &false, &1000);
}

/// A same-value update (new_total == old_total) is an increase-or-equal case
/// and must succeed without owner co-sign.
#[test]
fn test_no_change_does_not_require_owner_cosign() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    // Same total — no decrease path, no owner co-sign needed.
    client.update_total_assets(&agent, &deposit_amount, &false, &0);
    assert_eq(client.get_total_assets(), deposit_amount);
}

// ==================================================================================
// AUTO-COMPOUND — REINVEST YIELD
// =================================================================================

/// Auto-compound withdraws accrued yield and re-deposits it into the same
/// protocol, updating TotalAssets and emitting a CompoundEvent.
#[test]
fn test_auto_compound_updates_total_assets_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usec_token);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    // Simulate yield that has accrued in the protocol
    let yield_amount = 2_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);

    // Agent triggers auto-compound with a min_out of 0 (accept any amount)
    client.auto_compound(&agent, &0);

    // TotalAssets must reflect the compounded yield
    assert_eq(client.get_total_assets(), deposit_amount + yield_amount);

    // A CompoundEvent must be emitted
    let compound_events = find_events_by_topic(env.events().all(), &env, TOPIC_COMPOUND);
    assert_eq(
        compound_events.len(),
        1,
        "Exactly one compound event should be emitted"
    );
}

/// Only the configured agent may trigger auto-compound.
#[test]
#[should_panic(expected = "Error(Contract, #30)")]
fn test_non_agent_cannot_auto_compound() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    let non_agent = Address::generate(&env);
    client.auto_compound(&non_agent, &0);
}

/// Auto-compound must respect the compound cooldown (or shared rebalance
/// cooldown). A second call within the cooldown window is rejected.
#[test]
#[should_panic(expected = "Error(Contract, #49)")]
fn test_auto_compound_respects_cooldown() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usec_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usec_token);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usec_token, &user, deposit_amount);

    // First compound succeeds
    let yield_amount = 1_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.auto_compound(&agent, &0);

    // Second call while still within the cooldown should fail
    client.auto_compound(&agent, &0);
}
