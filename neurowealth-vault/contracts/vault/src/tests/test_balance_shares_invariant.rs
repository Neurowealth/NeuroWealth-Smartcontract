//! Accounting Invariant:
//!
//! User balance accounting and share accounting must remain
//! consistent throughout the vault lifecycle.
//!
//! Expected:
//!
//! get_balance(user)
//!     ≈
//! shares_to_assets(get_shares(user))
//!
//! Any divergence outside accepted rounding tolerance
//! indicates accounting corruption.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

const ROUNDING_TOLERANCE: i128 = 1;

fn abs_diff(a: i128, b: i128) -> i128 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

fn within_tolerance(a: i128, b: i128) -> bool {
    abs_diff(a, b) <= ROUNDING_TOLERANCE
}

fn assert_balance_share_consistency(client: &NeuroWealthVaultClient, user: &Address) {
    let balance = client.get_balance(user);
    let shares = client.get_shares(user);
    let converted = client.convert_to_assets(&shares);

    assert!(
        within_tolerance(balance, converted),
        "Balance/share invariant violated for {:?}: balance={}, share-derived balance={}",
        user,
        balance,
        converted
    );
}

fn assert_global_vault_invariants(client: &NeuroWealthVaultClient, users: &Vec<Address>) {
    let total_assets = client.get_total_assets();
    let total_shares = client.get_total_shares();

    let mut sum_balances = 0;
    let mut sum_shares = 0;

    for user in users.iter() {
        let balance = client.get_balance(&user);
        let shares = client.get_shares(&user);

        assert!(balance >= 0, "Negative balance");
        assert!(shares >= 0, "Negative shares");

        sum_balances += balance;
        sum_shares += shares;

        assert_balance_share_consistency(client, &user);
    }

    assert!(total_assets >= 0, "Negative total assets");
    assert!(total_shares >= 0, "Negative total shares");

    // Asset Conservation
    assert!(
        total_assets >= sum_balances,
        "Total Vault Assets ({}) < Sum(User Balances) ({})",
        total_assets,
        sum_balances
    );

    // Share Conservation
    assert_eq!(
        total_shares, sum_shares,
        "Total Vault Shares ({}) != Sum(User Shares) ({})",
        total_shares, sum_shares
    );

    // Exchange Rate Validity
    if total_assets > 0 && total_shares > 0 {
        assert!(total_assets > 0 && total_shares > 0, "Invalid share price");
    }
}

#[test]
fn test_invariant_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000);

    let mut users = Vec::new(&env);
    users.push_back(user.clone());

    assert_global_vault_invariants(&client, &users);
}

#[test]
fn test_invariant_yield_accrual() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);

    let deposit_amount = 10_000_000;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let shares_before = client.get_shares(&user);

    // Simulate yield
    let yield_amount = 5_000_000;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit_amount + yield_amount), &false, &0);

    let mut users = Vec::new(&env);
    users.push_back(user.clone());

    assert_global_vault_invariants(&client, &users);

    let shares_after = client.get_shares(&user);
    assert_eq!(
        shares_before, shares_after,
        "Share count should be unchanged by yield"
    );
}

#[test]
fn test_invariant_partial_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);

    let deposit_amount = 10_000_000;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Simulate yield
    let yield_amount = 5_000_000;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit_amount + yield_amount), &false, &0);

    client.withdraw(&user, &4_000_000);

    let mut users = Vec::new(&env);
    users.push_back(user.clone());

    assert_global_vault_invariants(&client, &users);
}

#[test]
fn test_invariant_full_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);

    let deposit_amount = 10_000_000;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Simulate yield
    let yield_amount = 5_000_000;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit_amount + yield_amount), &false, &0);

    client.withdraw_all(&user);

    let remaining_balance = client.get_balance(&user);
    let remaining_shares = client.get_shares(&user);

    assert_eq!(remaining_balance, 0);
    assert_eq!(remaining_shares, 0);
    assert_eq!(client.convert_to_assets(&0), 0);

    let mut users = Vec::new(&env);
    users.push_back(user.clone());

    assert_global_vault_invariants(&client, &users);
}

#[test]
fn test_invariant_multi_user() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user_a, 10_000_000);

    // Simulate yield
    token_client.mint(&contract_id, &2_000_000);
    client.update_total_assets(&agent, &12_000_000, &false, &0);

    mint_and_deposit(&env, &client, &usdc_token, &user_b, 15_000_000);

    // Simulate yield again
    token_client.mint(&contract_id, &5_000_000);
    client.update_total_assets(&agent, &32_000_000, &false, &0);

    let mut users = Vec::new(&env);
    users.push_back(user_a.clone());
    users.push_back(user_b.clone());

    assert_global_vault_invariants(&client, &users);
}

#[test]
fn test_invariant_blend_integration() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);
    let user = Address::generate(&env);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 10_000_000;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.rebalance(&soroban_sdk::Symbol::new(&env, "blend"), &850_i128, &0_i128);

    // Simulate Blend yield (mint to blend pool)
    token_client.mint(&blend_pool, &1_000_000);

    // Refresh vault state
    let vault_balance = token_client.balance(&contract_id);
    let blend_balance = blend_client.balance(&usdc_token, &contract_id);
    let new_total = vault_balance + blend_balance;

    client.update_total_assets(&agent, &new_total, &false, &0);

    let mut users = Vec::new(&env);
    users.push_back(user.clone());

    assert_global_vault_invariants(&client, &users);

    // Validate: Vault Assets = On-chain Assets + Blend Position Value
    assert_eq!(client.get_total_assets(), new_total);
}

#[test]
fn test_invariant_sequential_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);
    client.set_blend_pool(&owner, &blend_pool);

    let user_a = Address::generate(&env);
    let mut users = Vec::new(&env);
    users.push_back(user_a.clone());

    // 1. Deposit
    mint_and_deposit(&env, &client, &usdc_token, &user_a, 10_000_000);
    assert_global_vault_invariants(&client, &users);

    // 2. Yield
    token_client.mint(&contract_id, &1_000_000);
    client.update_total_assets(&agent, &11_000_000, &false, &0);
    assert_global_vault_invariants(&client, &users);

    // 3. Deposit Again
    mint_and_deposit(&env, &client, &usdc_token, &user_a, 5_000_000);
    assert_global_vault_invariants(&client, &users);

    // 4. Yield
    token_client.mint(&contract_id, &2_000_000);
    client.update_total_assets(&agent, &18_000_000, &false, &0);
    assert_global_vault_invariants(&client, &users);

    // 5. Partial Withdraw
    client.withdraw(&user_a, &6_000_000);
    assert_global_vault_invariants(&client, &users);

    // 6. Blend Update
    client.rebalance(&soroban_sdk::Symbol::new(&env, "blend"), &850_i128, &0_i128);
    token_client.mint(&blend_pool, &2_000_000); // Blend Yield

    let vault_balance = token_client.balance(&contract_id);
    let blend_balance = blend_client.balance(&usdc_token, &contract_id);
    let new_total = vault_balance + blend_balance;
    client.update_total_assets(&agent, &new_total, &false, &0);
    assert_global_vault_invariants(&client, &users);

    // 7. Withdraw Remaining
    client.withdraw_all(&user_a);
    assert_global_vault_invariants(&client, &users);
}

#[test]
fn test_invariant_tiny_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 1_000_000);

    let mut users = Vec::new(&env);
    users.push_back(user.clone());

    assert_global_vault_invariants(&client, &users);
}

#[test]
fn test_invariant_large_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Large deposit near MAX_DEPOSIT limit (10_000_000_000 is the default MAX_DEPOSIT)
    let large_amount = 10_000_000_000;
    mint_and_deposit(&env, &client, &usdc_token, &user, large_amount);

    let mut users = Vec::new(&env);
    users.push_back(user.clone());

    assert_global_vault_invariants(&client, &users);
}

#[test]
fn test_invariant_repeated_yield_updates() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000);

    let mut users = Vec::new(&env);
    users.push_back(user.clone());

    let mut current_assets = 10_000_000;
    for _ in 0..10 {
        let yield_amount = 1_234_567;
        token_client.mint(&contract_id, &yield_amount);
        current_assets += yield_amount;
        client.update_total_assets(&agent, &current_assets, &false, &0);

        assert_global_vault_invariants(&client, &users);
    }
}

#[test]
fn test_invariant_rounding_edge_cases() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let mut users = Vec::new(&env);
    users.push_back(user_a.clone());
    users.push_back(user_b.clone());

    mint_and_deposit(&env, &client, &usdc_token, &user_a, 3_000_000);

    // Yield that creates a fractional exchange rate
    token_client.mint(&contract_id, &1_000_000);
    client.update_total_assets(&agent, &4_000_000, &false, &0);

    // Deposit an odd amount
    mint_and_deposit(&env, &client, &usdc_token, &user_b, 2_000_001);

    assert_global_vault_invariants(&client, &users);

    client.withdraw(&user_b, &1);
    assert_global_vault_invariants(&client, &users);
}

#[test]
fn test_idle_deployed_total_invariant_across_rebalance_cycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);
    let user = Address::generate(&env);

    client.set_blend_pool(&owner, &blend_pool);

    // Helper to assert idle + deployed == total
    let assert_asset_breakdown_invariant = |client: &NeuroWealthVaultClient| {
        let (idle, deployed) = client.get_asset_breakdown();
        let total = client.get_total_assets();
        assert_eq!(
            idle + deployed,
            total,
            "idle_balance + deployed_assets must equal total_assets"
        );
    };

    // 1. Deposit
    let deposit_amount = 10_000_000;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    assert_asset_breakdown_invariant(&client);

    // 2. Rebalance to Blend
    client.rebalance(&soroban_sdk::Symbol::new(&env, "blend"), &850_i128, &0_i128);
    assert_asset_breakdown_invariant(&client);

    // 3. Simulate Blend yield
    token_client.mint(&blend_pool, &1_000_000);
    let vault_balance = token_client.balance(&contract_id);
    let blend_balance = blend_client.balance(&usdc_token, &contract_id);
    let new_total = vault_balance + blend_balance;
    client.update_total_assets(&agent, &new_total, &false, &0);
    assert_asset_breakdown_invariant(&client);

    // 4. Partial withdraw
    client.withdraw(&user, &4_000_000);
    assert_asset_breakdown_invariant(&client);

    // 5. Rebalance to different protocol (none)
    client.rebalance(&soroban_sdk::Symbol::new(&env, "none"), &0_i128, &0_i128);
    assert_asset_breakdown_invariant(&client);

    // 6. Verify final state
    let (idle, deployed) = client.get_asset_breakdown();
    let total = client.get_total_assets();
    assert_eq!(idle + deployed, total);
    assert_eq!(deployed, 0, "Deployed should be 0 after rebalance to none");
}
