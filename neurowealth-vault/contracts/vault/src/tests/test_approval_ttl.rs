//! Tests for configurable Blend token approval TTL.

use super::utils::*;
use crate::{ApprovalTtlUpdatedEvent, DEFAULT_APPROVAL_TTL, TOPIC_APPROVAL_TTL_UPDATED};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, TryFromVal,
};

fn setup_blend_position(
    env: &Env,
    ttl: Option<u32>,
) -> (
    Address,
    Address,
    Address,
    NeuroWealthVaultClient<'_>,
    TestTokenClient<'_>,
) {
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(env);
    let client = NeuroWealthVaultClient::new(env, &contract_id);
    let token_client = TestTokenClient::new(env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);
    if let Some(ttl) = ttl {
        client.set_approval_ttl(&ttl);
    }

    let user = Address::generate(env);
    mint_and_deposit(env, &client, &usdc_token, &user, 10_000_000_i128);

    (contract_id, usdc_token, blend_pool, client, token_client)
}

/// Sets up a vault with a configured DEX pool, a funded user deposit, and an
/// optional approval TTL — the DEX analogue of [`setup_blend_position`].
fn setup_dex_position(
    env: &Env,
    ttl: Option<u32>,
) -> (
    Address,
    Address,
    Address,
    NeuroWealthVaultClient<'_>,
    TestTokenClient<'_>,
) {
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(env);
    let client = NeuroWealthVaultClient::new(env, &contract_id);
    let token_client = TestTokenClient::new(env, &usdc_token);

    client.set_dex_pool(&owner, &dex_pool);
    if let Some(ttl) = ttl {
        client.set_approval_ttl(&ttl);
    }

    let user = Address::generate(env);
    mint_and_deposit(env, &client, &usdc_token, &user, 10_000_000_i128);

    (contract_id, usdc_token, dex_pool, client, token_client)
}

#[test]
fn test_approval_expiry_uses_configured_ttl() {
    let env = Env::default();
    let configured_ttl = 2_500_u32;
    let (contract_id, _usdc_token, blend_pool, client, token_client) =
        setup_blend_position(&env, Some(configured_ttl));

    let sequence = env.ledger().sequence();
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);

    let expiration = token_client.allowance_expiration(&contract_id, &blend_pool);
    assert_eq!(expiration, sequence + configured_ttl);
}

#[test]
fn test_approval_expiry_minimum_ttl_is_valid() {
    let env = Env::default();
    let minimum_ttl = 1_000_u32;
    let (contract_id, _usdc_token, blend_pool, client, token_client) =
        setup_blend_position(&env, Some(minimum_ttl));

    let sequence = env.ledger().sequence();
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);

    let expiration = token_client.allowance_expiration(&contract_id, &blend_pool);
    assert_eq!(expiration, sequence + minimum_ttl);
    assert!(expiration > sequence);
}

#[test]
fn test_set_approval_ttl_requires_owner_auth() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let attacker = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "set_approval_ttl",
            args: (2_000_u32,).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_set_approval_ttl(&2_000_u32);
    assert!(
        result.is_err(),
        "set_approval_ttl must fail without the owner's authorization"
    );
}

#[test]
fn test_default_approval_ttl_used_when_unconfigured() {
    let env = Env::default();
    let (contract_id, _usdc_token, blend_pool, client, token_client) =
        setup_blend_position(&env, None);

    assert_eq!(client.get_approval_ttl(), DEFAULT_APPROVAL_TTL);

    let sequence = env.ledger().sequence();
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);

    let expiration = token_client.allowance_expiration(&contract_id, &blend_pool);
    assert_eq!(expiration, sequence + DEFAULT_APPROVAL_TTL);
}

#[test]
fn test_set_approval_ttl_rejects_below_minimum() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let result = client.try_set_approval_ttl(&999_u32);
    assert!(
        result.is_err(),
        "set_approval_ttl should reject TTL below minimum"
    );
}

#[test]
fn test_set_approval_ttl_rejects_above_maximum() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let result = client.try_set_approval_ttl(&500_001_u32);
    assert!(
        result.is_err(),
        "set_approval_ttl should reject TTL above maximum"
    );
}

/// `set_approval_ttl` emits `ApprovalTtlUpdatedEvent` with the old and new
/// TTL so indexers can track TTL changes without polling storage (Issue #437).
#[test]
fn test_set_approval_ttl_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_approval_ttl(&17_280_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_APPROVAL_TTL_UPDATED);
    assert_eq!(
        events.len(),
        1,
        "exactly one approval-ttl-updated event expected"
    );

    let (_, _, data) = &events[0];
    let event = ApprovalTtlUpdatedEvent::try_from_val(&env, data)
        .expect("should be a valid ApprovalTtlUpdatedEvent");
    assert_eq!(
        event.old_ttl, DEFAULT_APPROVAL_TTL,
        "old_ttl before any explicit configuration is the default TTL"
    );
    assert_eq!(event.new_ttl, 17_280);
}

/// A second call to `set_approval_ttl` reports the previously configured TTL
/// as `old_ttl`, not the all-time default.
#[test]
fn test_set_approval_ttl_event_reflects_previous_value() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_approval_ttl(&50_000_u32);
    client.set_approval_ttl(&200_000_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_APPROVAL_TTL_UPDATED);
    assert_eq!(events.len(), 2, "two approval-ttl-updated events expected");

    let (_, _, data) = &events[1];
    let event = ApprovalTtlUpdatedEvent::try_from_val(&env, data)
        .expect("should be a valid ApprovalTtlUpdatedEvent");
    assert_eq!(event.old_ttl, 50_000);
    assert_eq!(event.new_ttl, 200_000);
}

/// A rejected `set_approval_ttl` call (out of bounds) must not emit an event.
#[test]
fn test_set_approval_ttl_rejected_call_emits_no_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let _ = client.try_set_approval_ttl(&999_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_APPROVAL_TTL_UPDATED);
    assert!(
        events.is_empty(),
        "a rejected set_approval_ttl call must not emit an event"
    );
}

/// Test that setting Blend approval TTL to 0 while funds are deployed does not
/// strand user assets. This verifies that emergency withdrawal is still possible
/// even when the approval TTL is set to zero (Issue #572).
#[test]
fn test_set_blend_approval_ttl_zero_with_deployed_funds_allows_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    // Set initial approval TTL to a valid value
    client.set_blend_approval_ttl(&owner, &10_000_u32);

    // Deposit funds and deploy them to Blend via rebalance
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);
    client.rebalance(&symbol_short!("blend"), &5_000_000_i128, &0_i128);

    // Verify funds are deployed (vault has less idle USDC)
    let vault_idle_balance = token_client.balance(&contract_id);
    assert!(
        vault_idle_balance < 10_000_000_i128,
        "funds should be deployed to Blend"
    );

    // Set Blend approval TTL to 0 while funds are deployed
    client.set_blend_approval_ttl(&owner, &0_u32);
    assert_eq!(client.get_blend_approval_ttl(), 0, "TTL should be set to 0");

    // Attempt to withdraw - this should succeed to prevent asset stranding
    // The vault should handle the expired approval gracefully by refreshing approvals
    let withdraw_amount = 1_000_000_i128;
    let user_balance_before = token_client.balance(&user);

    client.withdraw(&user, &withdraw_amount);

    let user_balance_after = token_client.balance(&user);
    assert_eq!(
        user_balance_after - user_balance_before,
        withdraw_amount,
        "user should receive the withdrawn amount even with TTL=0"
    );
}

// ─── DEX supply path (#341) ─────────────────────────────────────────────────
//
// The DEX supply path (`rebalance("dex", ..)` → `add_liquidity`) approves the
// pool to spend USDC using the same configurable approval TTL as Blend. These
// tests mirror the Blend coverage above for the DEX flow.

#[test]
fn test_dex_approval_expiry_uses_configured_ttl() {
    let env = Env::default();
    let configured_ttl = 2_500_u32;
    let (contract_id, _usdc_token, dex_pool, client, token_client) =
        setup_dex_position(&env, Some(configured_ttl));

    let sequence = env.ledger().sequence();
    client.rebalance(&symbol_short!("dex"), &700_i128, &0_i128);

    let expiration = token_client.allowance_expiration(&contract_id, &dex_pool);
    assert_eq!(expiration, sequence + configured_ttl);
}

#[test]
fn test_dex_approval_expiry_minimum_ttl_is_valid() {
    let env = Env::default();
    let minimum_ttl = 1_000_u32;
    let (contract_id, _usdc_token, dex_pool, client, token_client) =
        setup_dex_position(&env, Some(minimum_ttl));

    let sequence = env.ledger().sequence();
    client.rebalance(&symbol_short!("dex"), &700_i128, &0_i128);

    let expiration = token_client.allowance_expiration(&contract_id, &dex_pool);
    assert_eq!(expiration, sequence + minimum_ttl);
    assert!(expiration > sequence);
}

#[test]
fn test_default_approval_ttl_used_for_dex_supply() {
    let env = Env::default();
    let (contract_id, _usdc_token, dex_pool, client, token_client) = setup_dex_position(&env, None);

    assert_eq!(client.get_approval_ttl(), DEFAULT_APPROVAL_TTL);

    let sequence = env.ledger().sequence();
    client.rebalance(&symbol_short!("dex"), &700_i128, &0_i128);

    let expiration = token_client.allowance_expiration(&contract_id, &dex_pool);
    assert_eq!(expiration, sequence + DEFAULT_APPROVAL_TTL);
}

#[test]
fn test_dex_position_set_approval_ttl_rejects_below_minimum() {
    let env = Env::default();
    let (_contract_id, _usdc_token, _dex_pool, client, _token_client) =
        setup_dex_position(&env, None);

    let result = client.try_set_approval_ttl(&999_u32);
    assert!(
        result.is_err(),
        "set_approval_ttl should reject TTL below minimum on the DEX path"
    );
}

#[test]
fn test_dex_position_set_approval_ttl_rejects_above_maximum() {
    let env = Env::default();
    let (_contract_id, _usdc_token, _dex_pool, client, _token_client) =
        setup_dex_position(&env, None);

    let result = client.try_set_approval_ttl(&500_001_u32);
    assert!(
        result.is_err(),
        "set_approval_ttl should reject TTL above maximum on the DEX path"
    );
}

// ─── Blend approval TTL regression test (#381) ────────────────────────────────
//
// This test verifies that set_blend_approval_ttl actually affects the approval
// ledger used by supply_to_blend. Without this test, a storage-key mismatch
// (e.g., using ApprovalTtl instead of BlendApprovalTtl) would go undetected.

#[test]
fn test_blend_approval_ttl_affects_supply_to_blend_approval_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    // Set a custom Blend approval TTL distinct from the default
    let custom_ttl = 7_500_u32;
    client.set_blend_approval_ttl(&owner, &custom_ttl);

    // Deposit funds to enable supply_to_blend
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Trigger supply_to_blend via rebalance
    let sequence = env.ledger().sequence();
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);

    // Assert the approval expiration uses the custom TTL, not the default
    let expiration = token_client.allowance_expiration(&contract_id, &blend_pool);
    assert_eq!(
        expiration,
        sequence + custom_ttl,
        "approval expiration should reflect custom BlendApprovalTtl, not default"
    );
}

/// The legacy `set_blend_approval_ttl` mutates the same shared TTL storage as
/// `set_approval_ttl` and must leave the same audit trail (#591).
#[test]
fn test_set_blend_approval_ttl_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_approval_ttl(&owner, &50_000_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_APPROVAL_TTL_UPDATED);
    assert_eq!(
        events.len(),
        1,
        "set_blend_approval_ttl must emit an ApprovalTtlUpdatedEvent"
    );

    let (_, _, data) = &events[0];
    let event = ApprovalTtlUpdatedEvent::try_from_val(&env, data)
        .expect("should be a valid ApprovalTtlUpdatedEvent");
    assert_eq!(
        event.old_ttl, DEFAULT_APPROVAL_TTL,
        "old_ttl before any explicit configuration is the default TTL"
    );
    assert_eq!(event.new_ttl, 50_000);
}

/// A subsequent `set_approval_ttl` reports the value configured through the
/// legacy setter as `old_ttl`, proving the two setters share one audit trail.
#[test]
fn test_blend_and_shared_ttl_setters_share_audit_trail() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_approval_ttl(&owner, &50_000_u32);
    client.set_approval_ttl(&20_000_u32);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_APPROVAL_TTL_UPDATED);
    assert_eq!(events.len(), 2);

    let (_, _, data) = &events[1];
    let event = ApprovalTtlUpdatedEvent::try_from_val(&env, data)
        .expect("should be a valid ApprovalTtlUpdatedEvent");
    assert_eq!(event.old_ttl, 50_000);
    assert_eq!(event.new_ttl, 20_000);
}
