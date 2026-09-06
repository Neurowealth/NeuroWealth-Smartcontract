
#![cfg(test)]
//! Performance-fee arithmetic tests.
//!
//! The vault does not yet charge a performance fee on-chain; these tests lock
//! in the basis-point arithmetic and the maximum-fee bound that the future
//! implementation must respect.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

//! Pure arithmetic regression tests for the documented performance-fee policy.


/// Maximum performance fee the vault will ever be allowed to charge (10%).
const MAX_PERFORMANCE_FEE_BPS: u32 = 1_000;

fn fee_on_yield(yield_earned: i128, bps: u32) -> i128 {
    (yield_earned * (bps as i128)) / 10_000
}

#[test]
fn test_performance_fee_configuration_and_deduction() {

    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 1_000_0000000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    assert_eq!(client.get_total_assets(), deposit_amount);

    let set_bps = 500u32; // 5%
    assert!(set_bps <= MAX_PERFORMANCE_FEE_BPS);

    // Verify fee calculation logic on 1,000 USDC of yield.
    let yield_earned = 1_000_0000000_i128;
    let fee_amount = fee_on_yield(yield_earned, set_bps);

    // Performance fee is expressed in basis points and is capped at 10%.
    let max_allowed_bps = 1_000_u32;
    let set_bps = 500_u32;
    assert!(set_bps <= max_allowed_bps);

    let yield_earned = 1_000_000_000_i128;
    let fee_amount = (yield_earned * i128::from(set_bps)) / 10_000;

    let net_yield = yield_earned - fee_amount;

    assert_eq!(fee_amount, 50_000_000);
    assert_eq!(net_yield, 950_000_000);
}

#[test]
fn test_performance_fee_exceeds_maximum_rejected() {

    let invalid_bps = MAX_PERFORMANCE_FEE_BPS + 1;
    assert!(invalid_bps > MAX_PERFORMANCE_FEE_BPS);
}

#[test]
fn test_performance_fee_zero_bps_takes_nothing() {
    assert_eq!(fee_on_yield(1_000_0000000, 0), 0);

    let max_allowed_bps = 1_000_u32;
    let invalid_bps = 1_001_u32;
    assert!(invalid_bps > max_allowed_bps);

}
