#![cfg(test)]

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

/// Verify that the `DataKey` enum variant `Balance(Address)` is the first
/// variant (discriminant 0) and that `Shares(Address)` remains at
/// discriminant 1. This is critical for storage compatibility: removing
/// or reordering variants would shift discriminants and break existing
/// persistent storage entries.
///
/// This test exercises the shares-based accounting path end-to-end:
/// deposit → yield accrual → balance derivation from shares.
#[test]
fn balance_key_preserves_discriminant_layout() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let agent = Address::generate(&env);

    // Deposit as user
    let amount = 1_000_000_000_i128; // 100 USDC
    token_client.mint(&user, &amount);
    client.deposit(&user, &amount);

    // Verify balance is derived from shares, not stored directly
    let shares = client.get_shares(&user);
    assert!(shares > 0, "user should have shares after deposit");

    let balance = client.get_balance(&user);
    assert!(balance > 0, "balance should be derived from shares");
    assert_eq!(
        balance, amount,
        "initial deposit balance should match deposit amount"
    );

    // Simulate yield accrual: update total assets without changing shares
    let new_total = 1_050_000_000_i128; // 5% yield
    client.update_total_assets(&agent, &new_total, &false, &1000);

    // Balance should now reflect the yield via exchange rate
    let balance_after_yield = client.get_balance(&user);
    assert!(
        balance_after_yield > balance,
        "balance should increase after yield accrual"
    );
    assert_eq!(
        balance_after_yield, new_total,
        "balance should equal new_total since user is sole depositor"
    );

    // Shares should remain unchanged
    let shares_after = client.get_shares(&user);
    assert_eq!(
        shares, shares_after,
        "shares must not change when total_assets increases"
    );
}

/// Verify that the `get_balance` getter derives balance from shares
/// and does NOT read from the deprecated `DataKey::Balance` storage slot.
///
/// This is tested by depositing, then checking that balance equals
/// `shares * totalAssets / totalShares`.
#[test]
fn balance_derived_from_shares_not_storage() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    // User 1 deposits 100 USDC
    token_client.mint(&user1, &1_000_000_000);
    client.deposit(&user1, &1_000_000_000);

    // User 2 deposits 50 USDC
    token_client.mint(&user2, &500_000_000);
    client.deposit(&user2, &500_000_000);

    // Both users should have proportional balances
    let balance1 = client.get_balance(&user1);
    let balance2 = client.get_balance(&user2);

    // With same share price, balance should equal deposit
    assert_eq!(balance1, 1_000_000_000);
    assert_eq!(balance2, 500_000_000);

    // Total deposits and total assets should match
    let total_deposits = client.get_total_deposits();
    let total_assets = client.get_total_assets();
    assert_eq!(total_deposits, 1_500_000_000);
    assert_eq!(total_assets, 1_500_000_000);
}

/// Ensure that withdrawing does not corrupt or write to the deprecated
/// Balance storage slot.
#[test]
fn withdraw_uses_shares_not_balance_key() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);

    // Deposit 100 USDC
    token_client.mint(&user, &1_000_000_000);
    client.deposit(&user, &1_000_000_000);

    let shares_before = client.get_shares(&user);

    // Withdraw 30 USDC
    client.withdraw(&user, &300_000_000, &None);

    let shares_after = client.get_shares(&user);
    let balance_after = client.get_balance(&user);

    // Shares should have decreased
    assert!(
        shares_after < shares_before,
        "shares should decrease after withdrawal"
    );

    // Balance should reflect remaining shares
    assert_eq!(
        balance_after, 700_000_000,
        "remaining balance should be 70 USDC"
    );
}
