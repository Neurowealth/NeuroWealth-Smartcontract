//! Tests for zero-assets-with-shares protection (#973).
//!
//! When total_shares > 0 but total_assets == 0, new deposits are rejected
//! to prevent dilution attacks where existing worthless shares would capture
//! a pro-rata slice of new deposits.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Deposits are rejected when total_shares > 0 but total_assets == 0.
#[test]
#[should_panic(expected = "Error(Contract, #93)")]
fn test_deposit_rejected_when_shares_exist_but_assets_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // First user deposits
    let user1 = Address::generate(&env);
    let deposit1 = 10_000_000_i128;
    token.mint(&user1, &deposit1);
    client.deposit(&user1, &deposit1);

    // Owner reports total loss. Write TotalAssets directly rather than going
    // through update_total_assets's owner-cosigned, percentage-capped decrease
    // path (same approach as test_withdraw.rs's dust-holder scenario).
    env.as_contract(&contract_id, || {
        env.storage().instance().set(&DataKey::TotalAssets, &0_i128);
    });

    // Second user attempts to deposit - should be rejected
    let user2 = Address::generate(&env);
    let deposit2 = 5_000_000_i128;
    token.mint(&user2, &deposit2);
    client.deposit(&user2, &deposit2);
}

/// Deposits succeed when both total_shares and total_assets are zero (bootstrap).
#[test]
fn test_deposit_succeeds_when_both_shares_and_assets_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // First deposit should succeed (bootstrap state)
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    assert_eq!(client.get_total_shares(), deposit);
    assert_eq!(client.get_total_assets(), deposit);
}

/// Deposits succeed when total_shares > 0 and total_assets > 0 (normal operation).
#[test]
fn test_deposit_succeeds_when_both_shares_and_assets_positive() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // First user deposits
    let user1 = Address::generate(&env);
    let deposit1 = 10_000_000_i128;
    token.mint(&user1, &deposit1);
    client.deposit(&user1, &deposit1);

    // Yield accrues
    let yield_amount = 2_000_000_i128;
    token.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit1 + yield_amount), &false, &0);

    // Second user deposits - should succeed
    let user2 = Address::generate(&env);
    let deposit2 = 5_000_000_i128;
    token.mint(&user2, &deposit2);
    client.deposit(&user2, &deposit2);

    assert!(client.get_total_shares() > deposit1);
    assert!(client.get_total_assets() > deposit1 + yield_amount);
}

/// batch_deposit is also rejected when total_shares > 0 but total_assets == 0.
#[test]
#[should_panic(expected = "Error(Contract, #93)")]
fn test_batch_deposit_rejected_when_shares_exist_but_assets_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // First user deposits
    let user1 = Address::generate(&env);
    let deposit1 = 10_000_000_i128;
    token.mint(&user1, &deposit1);
    client.deposit(&user1, &deposit1);

    // Owner reports total loss (direct storage write; see above).
    env.as_contract(&contract_id, || {
        env.storage().instance().set(&DataKey::TotalAssets, &0_i128);
    });

    // Second user attempts batch_deposit - should be rejected
    let user2 = Address::generate(&env);
    let deposit2 = 5_000_000_i128;
    token.mint(&user2, &deposit2);

    let mut entries = soroban_sdk::Vec::new(&env);
    entries.push_back((usdc_token, deposit2));
    client.batch_deposit(&user2, &entries);
}

/// #1090: queue_withdrawal returns a defined error, not a raw divide-by-zero
/// panic, when shares exist but total_assets is zero.
#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_queue_withdrawal_rejected_when_shares_exist_but_assets_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Write the vault assets down to zero while the user still holds shares.
    env.as_contract(&contract_id, || {
        env.storage().instance().set(&DataKey::TotalAssets, &0_i128);
    });

    client.queue_withdrawal(&user, &1_000_000_i128);
}
