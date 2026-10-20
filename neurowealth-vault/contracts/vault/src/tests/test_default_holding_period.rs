//! Tests for default最小 holding period to prevent yield sniping (#975).
//!
//! The vault now initializes with a non-zero default MinHoldingPeriod of 120 ledgers
//! (~10 minutes) to prevent same-block deposit/withdraw cycles that would allow
//! yield sniping around update_total_assets.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// get_min_holding_period returns default 120 ledgers after initialization.
#[test]
fn test_default_min_holding_period() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit to initialize the vault
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Default should be 120 ledgers (~10 minutes)
    assert_eq!(client.get_min_holding_period(), 120);
}

/// Withdrawal is blocked if MinHoldingPeriod has not elapsed after deposit.
#[test]
#[should_panic(expected = "Error(Contract, #75)")]
fn test_withdraw_blocked_before_holding_period_elapses() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Attempt to withdraw immediately - should fail due to holding period
    client.withdraw(&user, &deposit);
}

/// Withdrawal succeeds after MinHoldingPeriod elapses.
#[test]
fn test_withdraw_succeeds_after_holding_period_elapses() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Advance ledger by 120 ledgers (default holding period)
    env.ledger().set_sequence(120);

    // Withdrawal should now succeed
    client.withdraw(&user, &deposit);
    assert_eq!(client.get_shares(&user), 0);
}

/// Owner can adjust MinHoldingPeriod if needed.
#[test]
fn test_owner_can_adjust_min_holding_period() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit to initialize the vault
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Verify default
    assert_eq!(client.get_min_holding_period(), 120);

    // Owner adjusts to 60 ledgers
    client.set_min_holding_period(&owner, &60);
    assert_eq!(client.get_min_holding_period(), 60);

    // Owner can set to 0 to disable
    client.set_min_holding_period(&owner, &0);
    assert_eq!(client.get_min_holding_period(), 0);
}

/// Yield sniping attack is prevented by default holding period.
#[test]
#[should_panic(expected = "Error(Contract, #75)")]
fn test_yield_sniping_prevented_by_holding_period() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // User A deposits
    let user_a = Address::generate(&env);
    let deposit_a = 10_000_000_i128;
    token.mint(&user_a, &deposit_a);
    client.deposit(&user_a, &deposit_a);

    // Yield accrues
    let yield_amount = 2_000_000_i128;
    token.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit_a + yield_amount), &false, &0);

    // User B attempts to snipe: deposit immediately before yield update
    let user_b = Address::generate(&env);
    let deposit_b = 5_000_000_i128;
    token.mint(&user_b, &deposit_b);
    client.deposit(&user_b, &deposit_b);

    // User B tries to withdraw immediately to capture yield
    // Should fail due to holding period
    client.withdraw(&user_b, &deposit_b);
}
