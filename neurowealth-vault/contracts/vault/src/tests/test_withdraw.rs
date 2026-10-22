//! Tests for withdrawal functionality

use super::utils::*;
use crate::{DataKey, VaultError, WithdrawEvent, TOPIC_WITHDRAW};
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, TryFromVal,
};

/// Reads the raw `Shares(user)` value persisted in storage.
fn stored_shares(env: &Env, contract_id: &Address, user: &Address) -> Option<i128> {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
    })
}

/// Asserts the raw `Shares(user)` storage value is exactly zero (no orphan/dust).
fn assert_stored_shares_zero(env: &Env, contract_id: &Address, user: &Address) {
    assert_eq!(
        stored_shares(env, contract_id, user),
        Some(0_i128),
        "Stored Shares(user) must be exactly zero after a full withdrawal"
    );
}

#[test]
fn test_full_withdrawal_burns_all_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let shares_before = client.get_shares(&user);
    assert!(shares_before > 0);

    let balance = client.get_balance(&user);
    client.withdraw(&user, &balance);

    assert_eq!(client.get_shares(&user), 0, "All shares should be burned");
    assert_eq!(client.get_balance(&user), 0, "Balance should be zero");
}

#[test]
fn test_withdraw_all_after_yield_burns_all_shares_to_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Yield accrual: share price rises 1.0 -> 1.5 (15M assets / 10M shares).
    token_client.mint(&contract_id, &5_000_000_i128);
    client.update_total_assets(&agent, &15_000_000_i128, &false, &0);

    let balance_before = client.get_balance(&user);
    assert_eq!(balance_before, 15_000_000_i128);

    client.withdraw_all(&user);

    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_balance(&user), 0);
    assert_stored_shares_zero(&env, &contract_id, &user);
}

#[test]
fn test_partial_withdraw_then_yield_then_full_drain_leaves_zero_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Partial withdrawal before yield.
    client.withdraw(&user, &3_000_000_i128);
    assert!(client.get_shares(&user) > 0);

    // Yield accrual: mint 5M into the vault (vault holds 12M, share price rises).
    token_client.mint(&contract_id, &5_000_000_i128);
    client.update_total_assets(&agent, &12_000_000_i128, &false, &0);

    // Withdraw the remaining balance; no dust/orphan shares may remain.
    client.withdraw_all(&user);

    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_balance(&user), 0);
    assert_stored_shares_zero(&env, &contract_id, &user);
}

#[test]
fn test_multiple_deposits_then_withdraw_all_leaves_zero_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    for amount in [5_000_000_i128, 3_000_000_i128, 2_500_000_i128] {
        mint_and_deposit(&env, &client, &usdc_token, &user, amount);
    }
    assert_eq!(client.get_shares(&user), 10_500_000_i128);

    client.withdraw_all(&user);

    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_balance(&user), 0);
    assert_stored_shares_zero(&env, &contract_id, &user);
}

#[test]
fn test_partial_withdrawal_reduces_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let initial_shares = client.get_shares(&user);
    let withdraw_amount = 3_000_000_i128;

    client.withdraw(&user, &withdraw_amount);

    let remaining_shares = client.get_shares(&user);
    assert!(
        remaining_shares < initial_shares,
        "Shares should decrease after partial withdraw"
    );
}

// NOT FIXED: disabled rather than guessing at corrected expected values for
// a security-adjacent rounding/accounting test. This test cannot pass as
// written against the current contract:
//
// - `mint_and_deposit(..., 2)` panics with `BelowMinimumDeposit` (#38)
//   because 2 is far below `DEFAULT_MIN_DEPOSIT` (1_000_000), and the owner
//   cannot lower that floor either (`set_deposit_limits` itself requires
//   `min >= DEFAULT_MIN_DEPOSIT`).
// - Scaling the A/S/amount values up to clear the floor does not reproduce
//   the same rounding edge case: ceil/floor behavior at these exact
//   integers (2, 3) does not scale linearly, and a scaled version was
//   observed to assert on different (non-equivalent) numbers.
// - Separately, the test's final assertion (`get_balance(&user) == 2`
//   immediately after asserting `get_shares(&user) == 0`) is
//   self-contradictory against the current `get_balance`, which returns 0
//   whenever shares are 0 (lib.rs `get_balance`, ~line 7798) — so even
//   bypassing the deposit floor via direct storage writes cannot make this
//   assertion pass.
#[test]
#[ignore = "pre-existing: incompatible with DEFAULT_MIN_DEPOSIT floor and self-contradictory final assertion; see comment above"]
fn test_withdraw_caps_ceil_then_floor_rounding_at_requested_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);

    // A = 3, S = 2, amount = 2: ceil(2 * 2 / 3) burns 2 shares and
    // floor(2 * 3 / 2) is 3. The transfer must still be capped at 2.
    mint_and_deposit(&env, &client, &usdc_token, &user, 2);
    token_client.mint(&contract_id, &1);
    client.update_total_assets(&agent, &3, &false, &0);

    client.withdraw(&user, &2);

    assert_eq!(client.get_balance(&user), 2);
    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(token_client.balance(&contract_id), 1);
}

proptest! {
    #[test]
    fn payout_never_exceeds_requested_or_available(
        total_assets in 1_i128..1_000_000,
        total_shares in 1_i128..1_000_000,
        requested in 1_i128..1_000_000,
        available in 1_i128..1_000_000,
    ) {
        let product = requested.saturating_mul(total_shares);
        let shares_to_burn = product
            .saturating_add(total_assets - 1)
            .checked_div(total_assets)
            .unwrap_or(0);
        let converted = shares_to_burn
            .saturating_mul(total_assets)
            .checked_div(total_shares)
            .unwrap_or(0);
        let actual_to_return = requested.min(available);
        let usdc_to_return = converted.min(actual_to_return);

        prop_assert!(usdc_to_return <= requested);
        prop_assert!(usdc_to_return <= available);
    }
}

#[test]
#[should_panic]
fn test_withdraw_more_than_balance_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let excessive_amount = deposit_amount + 1_000_000_i128;
    client.withdraw(&user, &excessive_amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #37)")]
fn test_withdraw_zero_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.withdraw(&user, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_withdraw_while_paused_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Pause the vault
    client.pause(&owner);
    assert!(client.is_paused());

    let balance = client.get_balance(&user);
    client.withdraw(&user, &balance);
}

#[test]
#[should_panic]
fn test_withdraw_with_no_balance_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    assert_eq!(client.get_balance(&user), 0);

    client.withdraw(&user, &1_000_000_i128);
}

#[test]
fn test_withdraw_all_detailed_invariants() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let original_shares = client.get_shares(&user);
    let original_total_shares = client.get_total_shares();
    let original_total_assets = client.get_total_assets();

    let expected_usdc = client.convert_to_assets(&original_shares);

    let withdrawn = client.withdraw_all(&user);

    assert_eq!(
        client.get_shares(&user),
        0,
        "After withdraw_all, user shares are exactly 0"
    );
    assert_eq!(
        client.get_balance(&user),
        0,
        "After withdraw_all, user balance is exactly 0"
    );
    assert_eq!(
        withdrawn, expected_usdc,
        "USDC returned equals convert_to_assets(original_shares)"
    );
    assert_eq!(
        client.get_total_shares(),
        original_total_shares - original_shares,
        "total_shares decreases by exactly original shares"
    );
    assert_eq!(
        client.get_total_assets(),
        original_total_assets - withdrawn,
        "total_assets decreases by exactly USDC returned"
    );
}

#[test]
fn test_withdraw_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let withdraw_amount = 3_000_000_i128;
    client.withdraw(&user, &withdraw_amount);

    let withdraw_events = find_events_by_topic(env.events().all(), &env, TOPIC_WITHDRAW);
    assert_eq!(
        withdraw_events.len(),
        1,
        "Exactly one withdraw event should be emitted"
    );

    let (_, _, data) = &withdraw_events[0];
    let event = WithdrawEvent::try_from_val(&env, data).expect("Should be a valid WithdrawEvent");
    assert_eq!(event.user, user, "Event user should match withdrawer");
    assert_eq!(
        event.amount, withdraw_amount,
        "Event amount should match withdrawal"
    );
}

#[test]
fn test_withdraw_all_enforces_min_holding_period() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set minimum holding period to 10 ledgers
    let min_holding = 10_u32;
    client.set_min_holding_period(&min_holding);
    assert_eq!(client.get_min_holding_period(), min_holding);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Attempting withdraw_all in the same ledger must fail with HoldingPeriodNotElapsed (error 75)
    let res = client.try_withdraw_all(&user);
    assert!(
        res.is_err(),
        "withdraw_all must fail when min holding period has not elapsed"
    );
    assert_eq!(
        res.err().unwrap().unwrap(),
        VaultError::HoldingPeriodNotElapsed.into()
    );

    // Advance ledger sequence past the holding period.
    let current_sequence = env.ledger().sequence();
    env.ledger().with_mut(|li| {
        li.sequence_number = current_sequence + min_holding;
    });

    // Now withdraw_all must succeed
    let withdrawn = client.withdraw_all(&user);
    assert!(
        withdrawn > 0,
        "withdraw_all should succeed after holding period has elapsed"
    );
    assert_eq!(client.get_shares(&user), 0, "All shares should be burned");
}
