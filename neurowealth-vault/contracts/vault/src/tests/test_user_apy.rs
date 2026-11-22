#![cfg(test)]

use super::utils::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

#[test]
fn test_user_realized_apy_zero_before_and_immediately_after_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // No deposit snapshot yet: APY is 0.
    assert_eq!(client.get_user_apy(&user), 0);

    let start_time = 1_000_000u64;
    env.ledger().with_mut(|l| l.timestamp = start_time);

    mint_and_deposit(&env, &client, &usdc_token, &user, 1000_0000000);

    // Immediately after deposit, held_seconds < 1 day, so APY is still 0.
    assert_eq!(client.get_user_apy(&user), 0);
}

#[test]
fn test_user_realized_apy_after_holding_period_with_no_yield() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    let start_time = 1_000_000u64;
    env.ledger().with_mut(|l| l.timestamp = start_time);
    mint_and_deposit(&env, &client, &usdc_token, &user, 1000_0000000);

    // Advance past the 1-day noise floor with no yield accrued: growth is 0,
    // so the realized APY should be 0.
    env.ledger()
        .with_mut(|l| l.timestamp = start_time + 2 * 86_400);
    assert_eq!(client.get_user_apy(&user), 0);
}
