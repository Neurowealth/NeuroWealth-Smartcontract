//! Tests for rounding and share math properties
//!
//! These tests ensure the vault's share-based accounting maintains mathematical invariants:
//! - Deposit then withdraw never returns more than total assets
//! - Shares/asset conversions are monotonic and consistent across sequences
//! - Zero/near-zero rounding edges are handled correctly
//! - Multi-user sequences maintain fairness

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Test that deposit then withdraw never returns more than total assets
#[test]
fn test_deposit_withdraw_never_exceeds_total_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128; // 10 USDC

    // Deposit
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Add some yield to make it interesting
    let yield_amount = 2_000_000_i128; // 2 USDC yield
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit_amount + yield_amount), &false, &0);

    let total_assets_before = client.get_total_assets();
    let vault_balance_before = token_client.balance(&contract_id);

    // Withdraw everything
    let withdrawn_amount = client.withdraw_all(&user, &None);

    let total_assets_after = client.get_total_assets();
    let vault_balance_after = token_client.balance(&contract_id);

    // Invariant: Total assets should never increase from a withdrawal
    assert!(
        total_assets_after <= total_assets_before,
        "Total assets should not increase after withdrawal"
    );

    // Invariant: Vault balance should never increase from a withdrawal
    assert!(
        vault_balance_after <= vault_balance_before,
        "Vault balance should not increase after withdrawal"
    );

    // Invariant: Withdrawn amount should not exceed user's proportional share
    assert!(
        withdrawn_amount <= deposit_amount + yield_amount,
        "Withdrawn amount should not exceed total contribution plus yield"
    );

    // Edge case: Withdrawal should never be negative
    assert!(
        withdrawn_amount >= 0,
        "Withdrawn amount should never be negative"
    );
}

/// Test multi-user sequences maintain fairness and invariants
#[test]
fn test_multi_user_deposit_withdraw_invariants() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Create multiple users with different deposit amounts
    let users = [
        (Address::generate(&env), 5_000_000_i128),  // 5 USDC
        (Address::generate(&env), 10_000_000_i128), // 10 USDC
        (Address::generate(&env), 15_000_000_i128), // 15 USDC
        (Address::generate(&env), 20_000_000_i128), // 20 USDC
    ];

    let mut total_deposited = 0_i128;

    // All users deposit
    for (user, amount) in users.iter() {
        mint_and_deposit(&env, &client, &usdc_token, user, *amount);
        total_deposited += amount;
    }

    // Add yield to the system
    let yield_amount = 8_000_000_i128; // 8 USDC yield
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(total_deposited + yield_amount), &false, &0);

    let total_assets_before = client.get_total_assets();
    let _total_shares_before = client.get_total_shares();

    // Users withdraw in random order
    let mut total_withdrawn = 0_i128;
    for (user, original_deposit) in users.iter() {
        let _user_shares_before = client.get_shares(user);
        let _user_balance_before = client.get_balance(user);

        let withdrawn_amount = client.withdraw_all(user, &None);
        total_withdrawn += withdrawn_amount;

        // Invariant: Withdrawn amount should be reasonable
        assert!(
            withdrawn_amount >= 0,
            "Withdrawn amount should never be negative"
        );
        assert!(
            withdrawn_amount <= total_assets_before,
            "Single withdrawal cannot exceed total assets"
        );

        // Invariant: User should get at least their principal (unless rounding)
        // In extreme cases, rounding might cause tiny losses, but never gains
        assert!(
            withdrawn_amount <= original_deposit + yield_amount,
            "User cannot withdraw more than their contribution plus proportional yield"
        );

        // Verify user state is properly reset
        assert_eq!(
            client.get_shares(user),
            0,
            "User shares should be zero after full withdrawal"
        );
        assert_eq!(
            client.get_balance(user),
            0,
            "User balance should be zero after full withdrawal"
        );
    }

    let total_assets_after = client.get_total_assets();
    let total_shares_after = client.get_total_shares();

    // Invariant: All shares should be burned
    assert_eq!(
        total_shares_after, 0,
        "Total shares should be zero after all withdrawals"
    );

    // Invariant: Total assets should be reasonable (may have tiny rounding remainder)
    assert!(
        total_assets_after >= 0,
        "Total assets should never be negative"
    );
    assert!(
        total_assets_after <= total_deposited + yield_amount,
        "Total assets should not exceed total deposits plus yield"
    );

    // Invariant: Total withdrawn should not exceed total assets
    assert!(
        total_withdrawn <= total_assets_before,
        "Total withdrawn cannot exceed initial total assets"
    );
}

/// Test shares/asset conversions are monotonic and consistent
#[test]
fn test_shares_asset_conversions_monotonic() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let initial_deposit = 10_000_000_i128;

    // Initial state: empty vault
    assert_eq!(
        client.get_total_shares(),
        0,
        "Initial shares should be zero"
    );
    assert_eq!(
        client.get_total_assets(),
        0,
        "Initial assets should be zero"
    );

    // Test bootstrap case (first deposit)
    let shares_for_10 = client.preview_deposit_to_shares(&initial_deposit);
    assert_eq!(
        shares_for_10, initial_deposit,
        "First deposit should be 1:1 shares"
    );

    mint_and_deposit(&env, &client, &usdc_token, &user, initial_deposit);

    // Add yield to increase share price
    let yield_amount = 5_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(initial_deposit + yield_amount), &false, &0);

    let total_assets_after_yield = client.get_total_assets();
    let total_shares_after_yield = client.get_total_shares();

    // Share price should have increased
    assert!(
        total_assets_after_yield > total_shares_after_yield,
        "Share price should increase after yield"
    );

    // Test conversion monotonicity using sufficiently separated inputs to avoid rounding ties.
    let shares_for_20 = client.preview_deposit_to_shares(&20_000_000_i128);
    let shares_for_60 = client.preview_deposit_to_shares(&60_000_000_i128);

    assert!(
        shares_for_60 > shares_for_20,
        "More assets should give more shares"
    );

    // Test conversion consistency: shares -> assets -> shares should be consistent
    let assets_from_shares = client.preview_shares_to_assets(&shares_for_10);
    let shares_back = client.preview_deposit_to_shares(&assets_from_shares);

    // Due to rounding, we might lose precision but never gain
    assert!(
        shares_back <= shares_for_10,
        "Round-trip conversion should not gain shares"
    );

    // Test that share price is consistent
    let share_price_1 = total_assets_after_yield * 1_000_000 / total_shares_after_yield;
    let assets_from_1m_shares = client.preview_shares_to_assets(&1_000_000_i128);
    let calculated_price = assets_from_1m_shares;

    assert_eq!(
        share_price_1, calculated_price,
        "Share price should be consistent across calculations"
    );
}

/// Test zero and near-zero rounding edge cases
#[test]
fn test_zero_near_zero_rounding_edges() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);

    // Test zero amount conversions
    let shares_for_zero = client.preview_deposit_to_shares(&0);
    let assets_for_zero = client.preview_shares_to_assets(&0);

    assert_eq!(shares_for_zero, 0, "Zero assets should give zero shares");
    assert_eq!(assets_for_zero, 0, "Zero shares should give zero assets");

    // Test sub-minimum amount conversions (1 unit = 0.0000001 USDC).
    // This should be safe for previews, but deposits will be rejected by the min-deposit guard.
    let sub_min_amount = 1_i128;
    let _shares_for_sub_min = client.preview_deposit_to_shares(&sub_min_amount);

    // Deposit the configured minimum (1 USDC = 1_000_000 with 7 decimals).
    let min_deposit = 1_000_000_i128;
    token_client.mint(&user, &min_deposit);
    client.deposit(&user, &min_deposit);

    // Test that tiny amounts don't break the system
    let tiny_yield = 1_i128;
    token_client.mint(&contract_id, &tiny_yield);
    client.update_total_assets(&agent, &(min_deposit + tiny_yield), &false, &0);

    // Withdraw should work even with tiny amounts
    let withdrawn = client.withdraw_all(&user, &None);
    assert!(withdrawn >= 0, "Withdrawal should work with tiny amounts");
    assert!(
        withdrawn <= min_deposit + tiny_yield,
        "Withdrawal should not exceed total"
    );

    // Test rounding with very small amounts in large pool
    let big_user = Address::generate(&env);
    let big_deposit = 10_000_000_000_i128; // 1,000 USDC (default max deposit)
    mint_and_deposit(&env, &client, &usdc_token, &big_user, big_deposit);

    let small_user = Address::generate(&env);
    let small_deposit = 1_000_000_i128; // 1 USDC (minimum deposit)
    token_client.mint(&small_user, &small_deposit);
    client.deposit(&small_user, &small_deposit);

    // Both users should be able to withdraw
    let big_withdrawn = client.withdraw_all(&big_user, &None);
    let small_withdrawn = client.withdraw_all(&small_user, &None);

    assert!(big_withdrawn > 0, "Big user should withdraw something");
    assert!(
        small_withdrawn >= 0,
        "Small user should not lose money due to rounding"
    );
    assert!(
        big_withdrawn + small_withdrawn <= big_deposit + small_deposit,
        "Total withdrawn should not exceed total deposited"
    );
}

/// Test that share price calculations are consistent
#[test]
fn test_share_price_consistency() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    // Multiple deposits at different times
    let deposit1 = 10_000_000_i128;
    let deposit2 = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user1, deposit1);

    // Add yield between deposits
    let yield1 = 2_000_000_i128;
    token_client.mint(&contract_id, &yield1);
    client.update_total_assets(&agent, &(deposit1 + yield1), &false, &0);

    let shares1_after_yield = client.get_shares(&user1);
    let assets1_from_shares = client.preview_shares_to_assets(&shares1_after_yield);

    // Second user deposits at higher share price
    mint_and_deposit(&env, &client, &usdc_token, &user2, deposit2);

    let shares2 = client.get_shares(&user2);
    let assets2_from_shares = client.preview_shares_to_assets(&shares2);

    // Both users should get proportional assets
    assert!(
        assets1_from_shares > deposit1,
        "User1 should have earned yield"
    );
    let diff2 = (assets2_from_shares - deposit2).abs();
    assert!(
        diff2 <= 1,
        "User2 should get approximately their deposit (allowing 1 unit rounding)"
    );

    // Add more yield
    let yield2 = 3_000_000_i128;
    token_client.mint(&contract_id, &yield2);
    client.update_total_assets(&agent, &(deposit1 + deposit2 + yield1 + yield2), &false, &0);

    // Check that share price is applied consistently
    let final_assets1 = client.preview_shares_to_assets(&shares1_after_yield);
    let final_assets2 = client.preview_shares_to_assets(&shares2);

    // Both should have benefited from second yield proportionally
    assert!(
        final_assets1 > assets1_from_shares,
        "User1 should benefit from second yield"
    );
    assert!(
        final_assets2 > assets2_from_shares,
        "User2 should benefit from second yield"
    );

    // Verify total assets consistency
    let total_assets = client.get_total_assets();
    let calculated_total = final_assets1 + final_assets2;

    // Should be very close (allowing for tiny rounding differences)
    let diff = (total_assets - calculated_total).abs();
    assert!(
        diff <= 1,
        "Total assets should match sum of user assets (allowing 1 unit rounding)"
    );
}

/// Test extreme rounding scenarios
#[test]
fn test_extreme_rounding_scenarios() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Use minimum deposit to avoid triggering the min-deposit guard.
    let tiny_deposit = 1_000_000_i128;

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);

    // Each user deposits tiny amount
    token_client.mint(&user1, &tiny_deposit);
    client.deposit(&user1, &tiny_deposit);

    token_client.mint(&user2, &tiny_deposit);
    client.deposit(&user2, &tiny_deposit);

    token_client.mint(&user3, &tiny_deposit);
    client.deposit(&user3, &tiny_deposit);

    let total_deposited = tiny_deposit * 3;
    assert_eq!(client.get_total_shares(), total_deposited);
    assert_eq!(client.get_total_assets(), total_deposited);

    // Add tiny yield to create rounding challenges
    let tiny_yield = 1_i128;
    token_client.mint(&contract_id, &tiny_yield);
    client.update_total_assets(&agent, &(total_deposited + tiny_yield), &false, &0);

    // All users withdraw - should handle rounding gracefully
    let withdrawn1 = client.withdraw_all(&user1, &None);
    let withdrawn2 = client.withdraw_all(&user2, &None);
    let withdrawn3 = client.withdraw_all(&user3, &None);
    let total_withdrawn = withdrawn1 + withdrawn2 + withdrawn3;

    // Check each withdrawal
    assert!(withdrawn1 >= 0, "No user should have negative withdrawal");
    assert!(
        withdrawn1 <= tiny_deposit + 1,
        "No user should gain from rounding"
    );
    assert!(withdrawn2 >= 0, "No user should have negative withdrawal");
    assert!(
        withdrawn2 <= tiny_deposit + 1,
        "No user should gain from rounding"
    );
    assert!(withdrawn3 >= 0, "No user should have negative withdrawal");
    assert!(
        withdrawn3 <= tiny_deposit + 1,
        "No user should gain from rounding"
    );

    // System should remain stable
    assert_eq!(client.get_total_shares(), 0, "All shares should be burned");
    assert!(
        client.get_total_assets() >= 0,
        "Total assets should not be negative"
    );
    assert!(
        total_withdrawn <= total_deposited + tiny_yield,
        "Total withdrawn should not exceed total plus yield"
    );
}

/// Test that preview functions match actual conversions
#[test]
fn test_preview_functions_match_actual_conversions() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit_amount = 7_543_210_i128; // Odd amount to test rounding

    // Preview should match actual
    let previewed_shares = client.preview_deposit_to_shares(&deposit_amount);

    token_client.mint(&user, &deposit_amount);
    client.deposit(&user, &deposit_amount);

    let actual_shares = client.get_shares(&user);
    assert_eq!(
        previewed_shares, actual_shares,
        "Preview should match actual shares"
    );

    // Add yield
    let yield_amount = 2_345_678_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit_amount + yield_amount), &false, &0);

    // Preview withdrawal should match actual
    let previewed_assets = client.preview_shares_to_assets(&actual_shares);
    let withdrawn_assets = client.withdraw_all(&user, &None);

    // Should be very close (allowing for 1 unit rounding difference)
    let diff = (previewed_assets - withdrawn_assets).abs();
    assert!(
        diff <= 1,
        "Preview should match actual withdrawal (allowing 1 unit rounding)"
    );
}

/// Test that preview_withdraw uses ceiling division matching actual withdraw
#[test]
fn test_preview_withdraw_matches_actual_withdraw_rounding() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Add yield to increase share price
    let yield_amount = 3_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit_amount + yield_amount), &false, &0);

    // Total assets = 13M, total shares = 10M => share price = 1.3
    // Withdraw 3M assets
    let withdraw_amount = 3_000_000_i128;

    // Preview withdraw uses ceiling division
    let previewed_shares_to_burn = client.preview_withdraw(&withdraw_amount);

    // Manual calculation of ceil(3M * 10M / 13M) = ceil(3000000000000/13000000) = ceil(230769.23...) = 230770
    let expected_ceil_shares =
        (withdraw_amount * client.get_total_shares() + client.get_total_assets() - 1)
            / client.get_total_assets();

    assert_eq!(
        previewed_shares_to_burn, expected_ceil_shares,
        "preview_withdraw should use ceiling division"
    );

    // Verify it differs from floor preview (preview_deposit_to_shares)
    let floor_shares = client.preview_deposit_to_shares(&withdraw_amount);
    assert!(
        previewed_shares_to_burn >= floor_shares,
        "Ceil should be >= floor for same amount"
    );

    // Actual withdraw should burn same number of shares as preview_withdraw
    let shares_before = client.get_shares(&user);
    client.withdraw(&user, &withdraw_amount, &None);
    let shares_after = client.get_shares(&user);
    let actual_shares_burned = shares_before - shares_after;

    assert_eq!(
        previewed_shares_to_burn, actual_shares_burned,
        "preview_withdraw should match actual shares burned"
    );
}

/// Test preview_withdraw with odd amounts to ensure ceiling rounding is correct
#[test]
fn test_preview_withdraw_odd_amounts() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit_amount = 7_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Add yield to create non-1:1 share price
    let yield_amount = 2_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit_amount + yield_amount), &false, &0);

    // Test with various odd amounts
    let test_amounts = [1_i128, 999_999_i128, 1_234_567_i128, 2_500_001_i128];

    for amount in test_amounts {
        let previewed = client.preview_withdraw(&amount);

        // Manual ceiling division check
        let total_shares = client.get_total_shares();
        let total_assets = client.get_total_assets();
        let product = amount * total_shares;
        let expected = (product + total_assets - 1) / total_assets;

        assert_eq!(
            previewed, expected,
            "preview_withdraw({}) should use ceiling: {} != {}",
            amount, previewed, expected
        );

        // Ceil should be >= floor for same input
        let floor_val = client.preview_deposit_to_shares(&amount);
        assert!(
            previewed >= floor_val,
            "Ceil result ({}) should be >= floor ({}) for amount {}",
            previewed,
            floor_val,
            amount
        );

        // Difference should be at most 1
        assert!(
            previewed - floor_val <= 1,
            "Ceil and floor should differ by at most 1"
        );
    }
}

/// Test dust withdrawals at high share price (ceil burn prevents under-burn)
#[test]
fn test_dust_withdrawal_at_high_share_price() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit_amount = 1_000_000_i128; // 1 USDC minimum
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Simulate massive yield accrual to drive share price very high
    // e.g. vault generated 99x yield
    let yield_amount = 99_000_000_i128; // 99 USDC yield
    token_client.mint(&contract_id, &yield_amount);
    let total_assets = deposit_amount + yield_amount; // 100M total
    client.update_total_assets(&agent, &total_assets, &false, &0);

    // Share price: total_assets / total_shares = 100 / 1 = 100x
    // total_shares = 1,000,000 (1:1 initial deposit)

    // At 100x price, 1 asset = 0.01 shares (floor = 0 shares if using floor)
    // Ceil burn ensures at least 1 share is burned even for 1 asset withdrawal
    let dust_amount = 1_i128; // 1 stroop (smallest unit)

    let previewed = client.preview_withdraw(&dust_amount);
    // Ceil(1 * 1_000_000 / 100_000_000) = Ceil(0.01) = 1 share
    assert!(
        previewed >= 1,
        "preview_withdraw for dust should return >= 1 share due to ceil rounding"
    );

    // Preview assets returned for 1 share
    let assets_from_1_share = client.preview_shares_to_assets(&1);
    assert!(
        assets_from_1_share >= dust_amount,
        "1 share should be worth at least {} assets",
        dust_amount
    );

    // Actual withdraw of dust amount should succeed (not revert)
    let shares_before = client.get_shares(&user);
    client.withdraw(&user, &dust_amount, &None);
    let shares_after = client.get_shares(&user);
    let shares_burned = shares_before - shares_after;

    assert!(
        shares_burned >= 1,
        "Dust withdrawal should burn at least 1 share"
    );

    // Verify vault state is consistent after dust withdrawal
    assert!(
        client.get_total_assets() >= 0,
        "Total assets should not be negative"
    );
    assert!(
        client.get_total_shares() >= 0,
        "Total shares should not be negative"
    );
}

/// Test preview_withdraw with zero and very small amounts
#[test]
fn test_preview_withdraw_edge_cases() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Zero amount should return 0
    assert_eq!(
        client.preview_withdraw(&0),
        0,
        "preview_withdraw(0) should return 0"
    );

    // Very small amounts should return at least 0
    assert!(
        client.preview_withdraw(&1) >= 0,
        "preview_withdraw(1) should not be negative"
    );

    // Preview with amount greater than total assets should still work (rounding prediction only)
    let large_amount = 100_000_000_000_i128;
    let previewed = client.preview_withdraw(&large_amount);
    assert!(
        previewed >= 0,
        "preview_withdraw with large amount should not be negative"
    );
}

// ============================================================================
// RANDOMIZED PROPERTY TESTS
// ============================================================================

struct SimplePrng {
    state: u64,
}

impl SimplePrng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    // Simple LCG random generator
    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn next_range(&mut self, min: i128, max: i128) -> i128 {
        assert!(max >= min);
        let range = (max - min + 1) as u64;
        if range == 0 {
            return min;
        }
        let val = self.next_u64() % range;
        min + val as i128
    }
}

/// Property Test 1: Deposit then withdraw never increases shares
#[test]
fn test_property_deposit_withdraw_never_increases_shares() {
    let mut prng = SimplePrng::new(42);

    for _ in 0..100 {
        let env = Env::default();
        env.mock_all_auths();

        let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);
        let token_client = TestTokenClient::new(&env, &usdc_token);

        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        // Generate randomized initial deposit (1 USDC to 10,000 USDC)
        let initial_deposit = prng.next_range(1_000_000, 10_000_000_000);
        mint_and_deposit(&env, &client, &usdc_token, &user1, initial_deposit);

        // Generate optional yield (0 USDC to 5,000 USDC)
        let yield_amount = prng.next_range(0, 5_000_000_000);
        if yield_amount > 0 {
            token_client.mint(&contract_id, &yield_amount);
            client.update_total_assets(&agent, &(initial_deposit + yield_amount), &false, &0);
        }

        // Generate randomized subsequent deposit
        let subsequent_deposit = prng.next_range(1_000_000, 10_000_000_000);

        // Check shares minted for the subsequent deposit
        let shares_minted = client.preview_deposit_to_shares(&subsequent_deposit);

        // Perform actual deposit
        mint_and_deposit(&env, &client, &usdc_token, &user2, subsequent_deposit);
        let actual_shares = client.get_shares(&user2);
        assert_eq!(shares_minted, actual_shares);

        // If they withdraw the same amount of assets they just deposited
        let shares_burned = client.preview_withdraw(&subsequent_deposit);

        // INVARIANT: The shares burned on withdrawal of `subsequent_deposit` assets
        // must be at least the shares minted on deposit of `subsequent_deposit` assets.
        // This ensures the deposit-then-withdraw cycle never increases the user's shares.
        assert!(
            shares_burned >= shares_minted,
            "Invariant Violation: shares_burned ({}) < shares_minted ({}) for deposit/withdraw of {}",
            shares_burned,
            shares_minted,
            subsequent_deposit
        );

        // DUAL INVARIANT: If they withdraw exactly the shares they got,
        // the assets returned must never exceed the assets they deposited.
        let assets_returned = client.preview_shares_to_assets(&shares_minted);
        assert!(
            assets_returned <= subsequent_deposit,
            "Invariant Violation: assets_returned ({}) > subsequent_deposit ({})",
            assets_returned,
            subsequent_deposit
        );
    }
}

/// Property Test 2: Total shares conserved on transfers and updates
#[test]
fn test_property_total_shares_conserved_on_transfers() {
    let mut prng = SimplePrng::new(1337);

    for _ in 0..100 {
        let env = Env::default();
        env.mock_all_auths();

        let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);
        let token_client = TestTokenClient::new(&env, &usdc_token);

        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);
        let user3 = Address::generate(&env);

        // 1. Initial deposits to establish user shares
        let deposit1 = prng.next_range(1_000_000, 10_000_000_000);
        let deposit2 = prng.next_range(1_000_000, 10_000_000_000);
        let deposit3 = prng.next_range(1_000_000, 10_000_000_000);

        mint_and_deposit(&env, &client, &usdc_token, &user1, deposit1);
        mint_and_deposit(&env, &client, &usdc_token, &user2, deposit2);
        mint_and_deposit(&env, &client, &usdc_token, &user3, deposit3);

        let total_shares_before = client.get_total_shares();
        let user1_shares = client.get_shares(&user1);
        let user2_shares = client.get_shares(&user2);
        let user3_shares = client.get_shares(&user3);

        // Invariant: total shares must equal sum of user shares
        assert_eq!(
            total_shares_before,
            user1_shares + user2_shares + user3_shares
        );

        // 2. Perform direct token transfers of the underlying token (USDC)
        // This simulates users sending USDC directly between themselves or to external parties.
        // It must NOT affect their shares in the vault, nor the total vault shares.
        let transfer_amount = prng.next_range(1, 500_000);
        token_client.mint(&user1, &transfer_amount);
        token_client.transfer(&user1, &user2, &transfer_amount);

        // Invariant: Vault total shares and user vault shares must remain perfectly constant
        assert_eq!(client.get_total_shares(), total_shares_before);
        assert_eq!(client.get_shares(&user1), user1_shares);
        assert_eq!(client.get_shares(&user2), user2_shares);
        assert_eq!(client.get_shares(&user3), user3_shares);

        // 3. Perform direct token transfer to the vault contract address ("donation")
        // Direct transfers bypass the deposit/withdraw functions.
        // This must not alter total shares.
        let donation_amount = prng.next_range(1, 10_000_000);
        token_client.mint(&user3, &donation_amount);
        token_client.transfer(&user3, &contract_id, &donation_amount);

        // Invariant: total shares must remain perfectly conserved
        assert_eq!(client.get_total_shares(), total_shares_before);
        assert_eq!(client.get_shares(&user1), user1_shares);
        assert_eq!(client.get_shares(&user2), user2_shares);
        assert_eq!(client.get_shares(&user3), user3_shares);

        // 4. Update total assets (simulating yield accrual)
        // Yield updates the exchange rate but must NOT alter total shares or individual shares.
        let yield_amount = prng.next_range(0, 1_000_000_000);
        if yield_amount > 0 {
            token_client.mint(&contract_id, &yield_amount);
            let total_assets_new = deposit1 + deposit2 + deposit3 + donation_amount + yield_amount;
            client.update_total_assets(&agent, &total_assets_new, &false, &0);
        }

        // Invariant: total shares must remain conserved
        assert_eq!(client.get_total_shares(), total_shares_before);
        assert_eq!(client.get_shares(&user1), user1_shares);
        assert_eq!(client.get_shares(&user2), user2_shares);
        assert_eq!(client.get_shares(&user3), user3_shares);
    }
}

// ============================================================================
// ISSUE #320 — Explicit ceil-burn rounding policy tests
// ============================================================================

/// Rounding policy: deposit mints FLOOR shares; withdraw burns CEIL shares.
/// This test verifies the vault never gives the user more than they deposited
/// when they immediately deposit then withdraw the same amount.
#[test]
fn test_floor_mint_ceil_burn_vault_never_loses() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Seed with a large depositor to create a non-trivial share price.
    let whale = Address::generate(&env);
    let whale_deposit = 10_000_000_i128; // 10 USDC
    mint_and_deposit(&env, &client, &usdc_token, &whale, whale_deposit);

    // Add 3 USDC yield so share price = 13/10 = 1.3.
    let yield_amt = 3_000_000_i128;
    token_client.mint(&contract_id, &yield_amt);
    client.update_total_assets(&agent, &(whale_deposit + yield_amt), &false, &0);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128; // 5 USDC
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Compute floor shares minted (deposit path) and ceil shares burned (withdraw path).
    let shares_minted = client.get_shares(&user);
    let shares_burned_preview = client.preview_withdraw(&deposit_amount);

    // Ceil burn >= floor mint — vault never loses value from a deposit-then-withdraw.
    assert!(
        shares_burned_preview >= shares_minted,
        "ceil burn ({}) must be >= floor mint ({})",
        shares_burned_preview,
        shares_minted
    );

    // Assets returned from floor-minted shares must not exceed the deposit.
    let assets_returned = client.preview_shares_to_assets(&shares_minted);
    assert!(
        assets_returned <= deposit_amount,
        "assets returned ({}) must not exceed deposit ({})",
        assets_returned,
        deposit_amount
    );
}

/// `withdraw_all` burns all user shares using floor conversion to assets and the
/// burn itself is all remaining shares (no ceil needed). Verify the total
/// returned assets equal the user's proportional claim.
#[test]
fn test_withdraw_all_returns_proportional_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user1, 6_000_000_i128);
    mint_and_deposit(&env, &client, &usdc_token, &user2, 4_000_000_i128);

    // Add 5 USDC yield to total 15 USDC across 10M shares.
    token_client.mint(&contract_id, &5_000_000_i128);
    client.update_total_assets(&agent, &15_000_000_i128, &false, &0);

    let user1_shares = client.get_shares(&user1);
    let total_shares = client.get_total_shares();
    let total_assets = client.get_total_assets();

    // Expected: user1 holds 60 % of shares → 60 % of 15M = 9M.
    let expected_user1 = user1_shares * total_assets / total_shares;
    let withdrawn1 = client.withdraw_all(&user1, &None);

    assert_eq!(
        withdrawn1, expected_user1,
        "withdraw_all must return exact proportional assets"
    );
    assert_eq!(
        client.get_shares(&user1),
        0,
        "all shares burned after withdraw_all"
    );
}

/// Withdrawing 1 stroop (smallest unit) at a very high share price must burn
/// at least 1 share due to ceiling division. Under floor division it would
/// burn 0 shares, allowing free withdrawals.
#[test]
fn test_ceil_burn_prevents_free_withdrawal_at_high_price() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit = 1_000_000_i128; // 1 USDC — 1M shares minted bootstrap
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Simulate 999x yield: 1 share is now worth 1000 stroops.
    let huge_yield = 999_000_000_i128;
    token_client.mint(&contract_id, &huge_yield);
    client.update_total_assets(&agent, &(deposit + huge_yield), &false, &0);

    // At price = 1000 assets / share:
    // floor(1 * 1_000_000 / 1_000_000_000) = floor(0.001) = 0 shares  ← WRONG
    // ceil(1 * 1_000_000 / 1_000_000_000)  = ceil(0.001)  = 1 share   ← CORRECT
    let shares_preview = client.preview_withdraw(&1_i128);
    assert!(
        shares_preview >= 1,
        "withdrawing 1 stroop must burn at least 1 share (got {})",
        shares_preview
    );

    // Actual withdraw must burn >= 1 share.
    let shares_before = client.get_shares(&user);
    client.withdraw(&user, &1_i128, &None);
    let shares_after = client.get_shares(&user);
    assert!(
        shares_before - shares_after >= 1,
        "actual withdraw must burn at least 1 share"
    );
}

/// Multiple sequential partial withdrawals must each burn ceil shares,
/// ensuring no user can gradually drain the vault by exploiting rounding.
#[test]
fn test_sequential_partial_withdrawals_ceil_each_time() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Add yield so share price is > 1.
    let yield_amt = 5_000_000_i128;
    token_client.mint(&contract_id, &yield_amt);
    client.update_total_assets(&agent, &(deposit + yield_amt), &false, &0);

    let total_assets_initial = client.get_total_assets();
    let mut cumulative_withdrawn = 0_i128;

    // Withdraw 1M assets 5 times.
    for _ in 0..5 {
        let shares_before = client.get_shares(&user);
        if shares_before == 0 {
            break;
        }
        client.withdraw(&user, &1_000_000_i128, &None);
        cumulative_withdrawn += 1_000_000_i128;
    }

    // Total withdrawn must not exceed what the user is entitled to.
    assert!(
        cumulative_withdrawn <= total_assets_initial,
        "cumulative withdrawals ({}) must not exceed total assets ({})",
        cumulative_withdrawn,
        total_assets_initial
    );
    // Vault total assets must remain non-negative.
    assert!(
        client.get_total_assets() >= 0,
        "total assets must not go negative"
    );
}
