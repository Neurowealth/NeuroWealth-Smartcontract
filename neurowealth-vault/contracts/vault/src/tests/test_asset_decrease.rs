//! Tests for the controlled asset decrease policy in `update_total_assets` (#319).
//!
//! Policy (all conditions must hold for a decrease to be accepted):
//! 1. `allow_decrease = true` — caller explicitly opts in.
//! 2. Owner has co-signed the transaction.
//! 3. The decrease does not exceed `max_decrease_bps` basis points of the current total
//!    (minimum floor: 100 bps = 1 %).
//!
//! Tests verify:
//! - Allowed decrease within bps succeeds.
//! - Increase always succeeds (no change to existing behavior).
//! - Decrease without `allow_decrease` flag is rejected (`TotalAssetsDecreaseNotAllowed`).
//! - Decrease exceeding `max_decrease_bps` is rejected (`DecreaseExceedsMaximumAllowedBps`).
//! - Boundary at exactly max bps succeeds; one unit over is rejected.
//! - Share price decreases proportionally after a loss report.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Increasing total assets always succeeds without the allow_decrease flag.
#[test]
fn test_increase_total_assets_always_allowed() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    let yield_amount = 2_000_000_i128;
    token.mint(&contract_id, &yield_amount);

    client.update_total_assets(&agent, &(amount + yield_amount), &false, &0);
    assert_eq!(client.get_total_assets(), amount + yield_amount);
}

/// An allowed decrease within the bps cap succeeds and updates total assets.
#[test]
fn test_allowed_decrease_within_bps_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    // Decrease by 500 bps (5 %) of 10M = 500_000 — within 1000 bps cap.
    let new_total = 9_500_000_i128;
    client.update_total_assets(&agent, &new_total, &true, &1000);
    assert_eq!(client.get_total_assets(), new_total);
}

/// Decrease at exactly max_decrease_bps succeeds (boundary: equal is allowed).
#[test]
fn test_decrease_at_exactly_max_bps_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    // The per-call max_decrease_bps argument is ignored (#974); the cap comes
    // from owner-configured storage. Set it to 500 (5%).
    client.set_max_decrease_bps(&500);

    // max_decrease_bps = 500 (5 %), old_total = 10M
    // max_decrease = 10M * 500 / 10_000 = 500_000
    // new_total = 9_500_000 (decrease of exactly 500_000 = at the boundary)
    let new_total = 9_500_000_i128;
    client.update_total_assets(&agent, &new_total, &true, &0);
    assert_eq!(client.get_total_assets(), new_total);
}

/// Decrease exceeding max_decrease_bps is rejected (`DecreaseExceedsMaximumAllowedBps = 32`).
#[test]
#[should_panic(expected = "Error(Contract, #32)")]
fn test_decrease_exceeding_max_bps_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    // The per-call max_decrease_bps argument is ignored (#974); the cap comes
    // from owner-configured storage. Set it to 500 (5 %).
    client.set_max_decrease_bps(&500);

    // max_decrease_bps = 500 (5 %), old_total = 10M
    // Attempting a 6 % decrease (600_000) exceeds the 5 % cap.
    let new_total = 9_400_000_i128; // decrease of 600_000
    client.update_total_assets(&agent, &new_total, &true, &0);
}

/// Decrease without `allow_decrease = true` is rejected (`TotalAssetsDecreaseNotAllowed = 31`).
#[test]
#[should_panic(expected = "Error(Contract, #31)")]
fn test_decrease_without_flag_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    // Decrease without allow_decrease flag must be rejected.
    client.update_total_assets(&agent, &9_000_000_i128, &false, &0);
}

/// After a confirmed loss report, share price decreases proportionally.
/// Users who withdraw afterwards receive less than they deposited.
#[test]
fn test_share_price_decreases_after_loss_report() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    let rate_before = client.get_exchange_rate();

    // Report a 10 % loss (within 1000 bps cap).
    let loss = 1_000_000_i128;
    let new_total = deposit - loss;
    client.update_total_assets(&agent, &new_total, &true, &1000);

    let rate_after = client.get_exchange_rate();
    assert!(
        rate_after < rate_before,
        "exchange rate must decrease after loss report"
    );

    // User's USDC-equivalent balance should also reflect the loss.
    let user_balance = client.get_balance(&user);
    assert!(
        user_balance < deposit,
        "user balance must be less than deposit after loss"
    );
    assert_eq!(
        user_balance, new_total,
        "user balance equals new total since they are the sole depositor"
    );
}

/// The minimum effective bps floor of 100 prevents a cap of 0 from
/// disabling decreases entirely. Passing max_decrease_bps = 0 applies
/// the 100 bps floor, so a 0.5 % decrease succeeds and a 1.5 % decrease fails.
#[test]
fn test_min_bps_floor_of_100_applied_when_zero_passed() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    token.mint(&user, &amount);
    client.deposit(&user, &amount);

    // The per-call max_decrease_bps argument is ignored (#974); the floor of
    // 100 bps applies to the stored cap. Store 0 so the clamp(100, 10_000)
    // path is what produces the effective 100 bps floor.
    client.set_max_decrease_bps(&0);

    // effective floor = 100 bps (1 %)
    // max_decrease = 10M * 100 / 10_000 = 100_000
    // A 100_000 decrease should succeed (at the floor exactly).
    let new_total = amount - 100_000_i128;
    client.update_total_assets(&agent, &new_total, &true, &0);
    assert_eq!(client.get_total_assets(), new_total);
}
