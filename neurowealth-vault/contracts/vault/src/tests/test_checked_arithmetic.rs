//! Boundary tests for checked arithmetic in the critical accounting paths.
//!
//! Scope (issue #127): deposit, withdraw, share conversion, and cap checks must
//! use checked ops that fail with explicit messages rather than silently
//! wrapping or panicking opaquely. These tests push state to values near
//! `i128::MAX` to actually exercise those guards.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// A single very large deposit (well within `i128`) succeeds and books shares
/// exactly 1:1 on bootstrap — confirming the checked math handles large operands
/// without spurious failure.
#[test]
fn test_large_deposit_within_range_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Disable per-user / TVL caps and raise the per-deposit max to allow a huge deposit.
    client.set_limits(&0, &0);
    client.set_deposit_limits(&1_000_000_i128, &i128::MAX);

    let user = Address::generate(&env);
    let amount = 1_000_000_000_000_000_000_i128; // 1e18, far below i128::MAX (~1.7e38)
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    assert_eq!(client.get_shares(&user), amount, "bootstrap mints 1:1");
    assert_eq!(client.get_total_assets(), amount);
    assert_eq!(client.get_total_deposits(), amount);
}

/// Share conversion multiplies `assets * total_shares`. With a large existing
/// share supply, converting a near-max asset amount overflows that product; the
/// checked_mul must surface the explicit message rather than a silent wrap.
///
/// (The deposit/balance accumulator guards are belt-and-suspenders: because the
/// vault holds tokens 1:1 with booked principal, the token's own balance math
/// reaches `i128::MAX` at the same point, so those guards can't be isolated
/// through the public deposit path. The conversion guard below is the reachable
/// boundary on the share-conversion path.)
#[test]
#[should_panic(expected = "vault: conversion mul overflow")]
fn test_convert_to_shares_mul_overflow_is_checked() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    client.set_limits(&0, &0);
    client.set_deposit_limits(&1_000_000_i128, &i128::MAX);

    // Bootstrap a large share supply: total_shares == total_assets == 1e18.
    let user = Address::generate(&env);
    let amount = 1_000_000_000_000_000_000_i128; // 1e18
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    // convert_to_shares(i128::MAX) computes i128::MAX * total_shares → overflow.
    client.convert_to_shares(&i128::MAX);
}

/// The per-user deposit cap is an exact boundary: depositing right up to the cap
/// succeeds; one unit over is rejected with the explicit cap message.
#[test]
fn test_user_deposit_cap_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let cap = 100_000_000_i128;
    client.set_user_deposit_cap(&cap);
    client.set_deposit_limits(&1_000_000_i128, &i128::MAX);

    let user = Address::generate(&env);
    token.mint(&user, &(cap + 1_000_000));

    // Exactly at the cap: allowed.
    client.deposit(&user, &cap);
    assert_eq!(client.get_shares(&user), cap);

    // One more (above the cap) must be rejected.
    let over = client.try_deposit(&user, &1_000_000_i128);
    assert!(over.is_err(), "deposit above user cap must be rejected");
}

/// Withdrawing more than the user holds must fail via the checked share path
/// rather than underflowing the share accounting.
#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_withdraw_share_underflow_is_checked() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Request more than deposited; share conversion exceeds the user's holdings.
    client.withdraw(&user, &11_000_000_i128, &None);
}

/// A large balance can be fully withdrawn, confirming the withdraw-side checked
/// subtractions (shares, totals, principal) handle near-max operands correctly.
#[test]
fn test_large_balance_full_withdraw() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    client.set_limits(&0, &0);
    client.set_deposit_limits(&1_000_000_i128, &i128::MAX);

    let user = Address::generate(&env);
    let amount = 5_000_000_000_000_000_000_i128; // 5e18
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    let returned = client.withdraw_all(&user, &None);
    assert_eq!(returned, amount, "full large balance withdrawn");
    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_total_shares(), 0);
    assert_eq!(token.balance(&user), amount);
}

/// `update_total_assets` computes `old_total * max_decrease_bps` when bounding a
/// decrease. With `old_total == i128::MAX` this would overflow; the checked_mul
/// must surface the explicit message instead of an opaque panic.
#[test]
#[should_panic(expected = "vault: max decrease mul overflow")]
fn test_update_total_assets_decrease_bound_overflow_is_checked() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    client.set_limits(&0, &0);
    client.set_deposit_limits(&1_000_000_i128, &i128::MAX);

    // Drive stored TotalAssets to i128::MAX via a max-sized deposit.
    let user = Address::generate(&env);
    token.mint(&user, &i128::MAX);
    client.deposit(&user, &i128::MAX);

    // Request a decrease: max_decrease = old_total(MAX) * bps(>=100) overflows.
    client.update_total_assets(&agent, &(i128::MAX - 1), &true, &100);
}

/// `get_exchange_rate` computes `total_assets * SCALAR / total_shares`.
/// The division is now checked; this test verifies that a normal non-zero
/// total_shares produces the correct rate without panicking (#318).
#[test]
fn test_exchange_rate_checked_div_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128; // 10 USDC
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    // Bootstrap: 1:1, so rate = 10_000_000 (scalar = 10_000_000, rate = 1.0)
    let rate = client.get_exchange_rate();
    assert_eq!(rate, 10_000_000_i128, "bootstrap exchange rate should be 1:1 scaled");
}

/// A small but valid decrease within bps bounds uses `.checked_div(10_000)`
/// and must produce the correct max-decrease ceiling without panicking (#318).
#[test]
fn test_update_total_assets_decrease_div_within_bounds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128; // 10 USDC
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    // max_decrease_bps = 1000 (10 %), old_total = 10M
    // max_decrease = 10M * 1000 / 10_000 = 1M
    // new_total = 9M (decrease of 1M = exactly at the 10 % cap) — must succeed
    let new_total = 9_000_000_i128;
    client.update_total_assets(&agent, &new_total, &true, &1000);
    assert_eq!(client.get_total_assets(), new_total);
}
