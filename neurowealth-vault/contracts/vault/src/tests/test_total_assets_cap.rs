//! Regression tests for issues #183 and #299 – TVL cap uses TotalAssets (not TotalDeposits).
//!
//! ## TotalDeposits vs TotalAssets (issue #299)
//!
//! `TotalDeposits` tracks principal only; `TotalAssets` includes yield.
//! After `update_total_assets()`, `TotalAssets >= TotalDeposits`.
//!
//! Design decision (issue #299): `TotalDeposits` is intentionally *not* synced on
//! yield updates.  It is a principal-only counter for reporting.  All cap guards
//! and share-pricing use `TotalAssets` so that yield is correctly accounted for.
//!
//! The TVL cap compares against `TotalAssets` to prevent the vault from accepting
//! deposits that would push total managed value (principal + yield) past the cap.
//! Checking `TotalDeposits` instead would allow over-subscription once yield grows
//! the vault past the cap.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

// ============================================================================
// TVL cap uses TotalAssets, not TotalDeposits
// ============================================================================

/// After yield is credited (TotalAssets > TotalDeposits), the TVL cap is
/// evaluated against TotalAssets.  A deposit that would push TotalAssets
/// above the cap is rejected even if TotalDeposits is still below the cap.
#[test]
#[should_panic(expected = "Error(Contract, #41)")]
fn test_tvl_cap_blocks_deposit_after_yield_accrual() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // TVL cap = 15 USDC
    let tvl_cap = 15_000_000_i128;
    client.set_tvl_cap(&tvl_cap);

    // User deposits 10 USDC  →  TotalDeposits = 10, TotalAssets = 10
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Simulate 4 USDC yield  →  TotalAssets = 14, TotalDeposits stays 10
    let yield_amount = 4_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit + yield_amount), &false, &0);

    assert_eq!(client.get_total_assets(), 14_000_000_i128);
    assert_eq!(client.get_total_deposits(), 10_000_000_i128);

    // Attempting to deposit another 2 USDC would push TotalAssets to 16,
    // which exceeds the cap of 15 USDC.  Must be rejected.
    let user2 = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user2, 2_000_000_i128);
}

/// Deposit is accepted when TotalAssets + new_deposit <= TVL cap,
/// even after yield has grown TotalAssets above TotalDeposits.
#[test]
fn test_deposit_accepted_when_total_assets_plus_amount_within_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // TVL cap = 20 USDC
    let tvl_cap = 20_000_000_i128;
    client.set_tvl_cap(&tvl_cap);

    // Deposit 10 USDC
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Accrue 4 USDC yield  →  TotalAssets = 14
    let yield_amount = 4_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &14_000_000_i128, &false, &0);

    // Depositing 5 USDC pushes TotalAssets to 19, still within 20 cap
    let user2 = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user2, 5_000_000_i128);

    assert!(
        client.get_total_assets() <= tvl_cap,
        "TotalAssets should not exceed the TVL cap"
    );
}

// ============================================================================
// Deposit → yield → withdraw → cap check regression (#183)
// ============================================================================

/// Full lifecycle: deposit, accrue yield, withdraw, then verify a subsequent
/// deposit respects the cap based on remaining TotalAssets (not TotalDeposits).
#[test]
fn test_deposit_yield_withdraw_cap_regression() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // TVL cap = 20 USDC
    let tvl_cap = 20_000_000_i128;
    client.set_tvl_cap(&tvl_cap);

    // Step 1: user1 deposits 10 USDC
    let user1 = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user1, 10_000_000_i128);

    // Step 2: 5 USDC yield credited  →  TotalAssets = 15
    token_client.mint(&contract_id, &5_000_000_i128);
    client.update_total_assets(&agent, &15_000_000_i128, &false, &0);

    // Step 3: user1 withdraws 5 USDC  →  TotalAssets shrinks
    client.withdraw(&user1, &5_000_000_i128, &None);
    let assets_after_withdraw = client.get_total_assets();

    // Step 4: user2 deposits up to what remains under the cap
    let headroom = tvl_cap - assets_after_withdraw;
    if headroom >= 1_000_000 {
        let user2 = Address::generate(&env);
        mint_and_deposit(
            &env,
            &client,
            &usdc_token,
            &user2,
            headroom.min(5_000_000_i128),
        );
        assert!(
            client.get_total_assets() <= tvl_cap,
            "TotalAssets must not exceed TVL cap after deposit"
        );
    }
}

// ============================================================================
// TotalDeposits vs TotalAssets relationship documentation test
// ============================================================================

/// Confirms that TotalAssets ≥ TotalDeposits after yield accrual,
/// and that share pricing reflects TotalAssets (not TotalDeposits).
#[test]
fn test_total_assets_reflects_yield_while_total_deposits_stays_as_principal() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let principal = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, principal);

    assert_eq!(client.get_total_deposits(), principal);
    assert_eq!(client.get_total_assets(), principal);

    // Accrue 50 % yield
    let yield_amount = 5_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(principal + yield_amount), &false, &0);

    // TotalDeposits stays unchanged (tracks principal)
    assert_eq!(
        client.get_total_deposits(),
        principal,
        "TotalDeposits must not change on yield"
    );
    // TotalAssets grows (tracks principal + yield)
    assert_eq!(
        client.get_total_assets(),
        principal + yield_amount,
        "TotalAssets must include yield"
    );
    // User's share-based balance reflects the yield
    let user_balance = client.get_balance(&user);
    assert!(
        user_balance > principal,
        "User balance should exceed principal after yield accrual"
    );
    assert_eq!(
        user_balance,
        principal + yield_amount,
        "Sole depositor should receive all yield"
    );
}
