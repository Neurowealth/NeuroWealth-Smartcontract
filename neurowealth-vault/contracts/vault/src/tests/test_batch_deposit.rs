#![cfg(test)]

use super::utils::{setup_vault_with_token, NeuroWealthVaultClient, TestTokenClient};
use soroban_sdk::{testutils::Address as _, vec, Address, Env};

#[test]
fn test_batch_deposit_single_user_multiple_entries() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let first_amount = 100_0000000_i128;
    let second_amount = 200_0000000_i128;
    token_client.mint(&user, &(first_amount + second_amount));

    let entries = vec![
        &env,
        (usdc_token.clone(), first_amount),
        (usdc_token.clone(), second_amount),
    ];
    client.batch_deposit(&user, &entries);

    assert_eq!(client.get_shares(&user), first_amount + second_amount);
    assert_eq!(
        token_client.balance(&contract_id),
        first_amount + second_amount
    );
    assert_eq!(
        client.get_total_assets(),
        first_amount + second_amount,
        "batch_deposit must record the aggregate amount in TotalAssets, \
         not just TotalDeposits/TotalShares"
    );
    assert_eq!(client.get_total_deposits(), first_amount + second_amount);
}

/// Regression test (#987): `batch_deposit` used to mint shares and bump
/// `TotalDeposits`/`TotalShares` without ever updating `TotalAssets`,
/// silently understating the share price for every holder (new shares were
/// issued "for free" relative to the assets actually backing them). A
/// fuzz-harness invariant check (sum(shares) vs total_assets) caught this
/// after a single `batch_deposit` call with no other activity.
#[test]
fn test_batch_deposit_keeps_total_assets_in_sync_with_shares() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    token_client.mint(&user, &amount);

    let entries = vec![&env, (usdc_token.clone(), amount)];
    client.batch_deposit(&user, &entries);

    let total_shares = client.get_total_shares();
    let total_assets = client.get_total_assets();
    assert!(
        total_assets >= total_shares.min(amount),
        "total_assets ({total_assets}) must not be zero while shares/deposits \
         are non-zero after a batch_deposit of {amount}"
    );
    assert_eq!(total_assets, amount);

    let expected_balance =
        client.get_shares(&user).checked_mul(total_assets).unwrap() / total_shares;
    assert_eq!(client.get_balance(&user), expected_balance);
}

/// Regression test (#987): a user whose *first* deposit went through
/// `batch_deposit` never got added to the active-share user index (unlike
/// `deposit`, which always calls `add_to_user_index` on a user's first
/// deposit), so `get_users_with_shares` silently omitted them despite their
/// holding real, non-zero shares.
#[test]
fn test_batch_deposit_registers_first_time_depositor_in_user_index() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    token_client.mint(&user, &amount);

    let entries = vec![&env, (usdc_token.clone(), amount)];
    client.batch_deposit(&user, &entries);

    let (indexed, _next) = client.get_users_with_shares(&0, &u32::MAX);
    let in_index = indexed.iter().any(|(addr, _)| addr == user);
    assert!(
        in_index,
        "a user's first batch_deposit must register them in the user-shares index"
    );
    let indexed_shares = indexed
        .iter()
        .find(|(addr, _)| *addr == user)
        .map(|(_, shares)| shares)
        .unwrap();
    assert_eq!(indexed_shares, client.get_shares(&user));
}
