//! Transfer-delta sanity tests (Issue #603).
//!
//! Vault accounting assumes the amount **received** by a token transfer
//! equals the amount **requested** — true for USDC's Stellar asset contract
//! today, but silently violated if the asset ever became fee-on-transfer or
//! a deflationary wrapper token were configured by mistake. These tests pin
//! that assumption on every fund-moving path: they measure the actual token
//! balance deltas around `deposit`, `withdraw`, and `rebalance`, and fail
//! the moment any transfer delivers less than the requested amount.
//!
//! See SECURITY.md ("Fee-on-transfer / deflationary asset assumption") for
//! the documented rejection stance.

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

#[test]
fn test_deposit_transfer_delta_matches_requested_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128; // 5 USDC
    token_client.mint(&user, &amount);

    let vault_before = token_client.balance(&contract_id);
    let user_before = token_client.balance(&user);

    client.deposit(&user, &amount);

    // The vault must have received EXACTLY the requested amount, and the
    // user must have paid exactly it: any fee-on-transfer behaviour makes
    // one of these deltas diverge and this test fail.
    assert_eq!(
        token_client.balance(&contract_id) - vault_before,
        amount,
        "vault received a different amount than requested (fee-on-transfer?)"
    );
    assert_eq!(
        user_before - token_client.balance(&user),
        amount,
        "user paid a different amount than requested"
    );

    // Accounting matches the physical balance: shares/assets recorded from
    // the requested amount are fully backed by tokens actually held.
    assert_eq!(client.get_total_assets(), amount);
    assert_eq!(
        client.get_total_assets(),
        token_client.balance(&contract_id)
    );
}

#[test]
fn test_withdraw_transfer_delta_matches_requested_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit_amount = 8_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let withdraw_amount = 3_000_000_i128;
    let vault_before = token_client.balance(&contract_id);
    let user_before = token_client.balance(&user);

    client.withdraw(&user, &withdraw_amount);

    assert_eq!(
        token_client.balance(&user) - user_before,
        withdraw_amount,
        "user received a different amount than requested (fee-on-transfer?)"
    );
    assert_eq!(
        vault_before - token_client.balance(&contract_id),
        withdraw_amount,
        "vault paid out a different amount than requested"
    );

    // Remaining recorded assets are still fully backed by physical balance.
    assert_eq!(
        client.get_total_assets(),
        token_client.balance(&contract_id)
    );
}

#[test]
fn test_rebalance_transfer_delta_matches_deployed_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let vault_before = token_client.balance(&contract_id);
    let pool_before = token_client.balance(&blend_pool);
    assert_eq!(vault_before, deposit_amount);

    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    // Every token that left the vault arrived at the pool: the deltas match
    // exactly and the pool's own accounting agrees with the physical
    // balance it received.
    let vault_delta = vault_before - token_client.balance(&contract_id);
    let pool_delta = token_client.balance(&blend_pool) - pool_before;
    assert_eq!(vault_delta, deposit_amount);
    assert_eq!(
        pool_delta, vault_delta,
        "pool received a different amount than the vault sent (fee-on-transfer?)"
    );
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);

    // Recorded total assets remain fully backed across idle + deployed.
    assert_eq!(client.get_total_assets(), deposit_amount);
}

#[test]
fn test_deposit_withdraw_round_trip_conserves_tokens() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 7_000_000_i128;
    token_client.mint(&user, &amount);
    let user_start = token_client.balance(&user);

    client.deposit(&user, &amount);
    client.withdraw_all(&user);

    // With an exact-transfer asset and no fees, a full round trip conserves
    // the user's tokens and leaves the vault empty. A deflationary asset
    // would leak value on each leg and break both assertions.
    assert_eq!(token_client.balance(&user), user_start);
    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(client.get_total_assets(), 0);
}
