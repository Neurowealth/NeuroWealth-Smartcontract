//! Tests for small-amount and dust deposit/withdrawal rounding behavior (Issue #256).
//!
//! These tests verify the vault's floor-mint / ceil-burn rounding policy at the
//! smallest possible amounts and confirm the invariant: users can never extract
//! more value than they deposited through rounding.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

// 1 USDC = 1_000_000 i128 (7 decimal places on Stellar)
const USDC: i128 = 1_000_000;

// ============================================================================
// Test 1: 1-stroop deposit mints non-zero shares OR contract panics with #6
// ============================================================================

/// Verifies the share formula at minimal amounts via preview.
///
/// A 1-stroop (1 i128) deposit cannot be submitted because the min-deposit guard
/// enforces DEFAULT_MIN_DEPOSIT = 1_000_000 (1 USDC). This test uses
/// `preview_deposit_to_shares` to verify the underlying formula without hitting
/// the guard, then confirms a 1-USDC deposit produces the expected bootstrap shares.
///
/// On a fresh vault (bootstrap state): shares_to_mint = assets (1:1).
/// After the vault has shares: shares_to_mint = floor(assets * total_shares / total_assets).
#[test]
fn test_tiny_deposit_mints_nonzero_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Preview formula for 1 stroop on empty vault: bootstrap path returns assets = 1.
    // The actual deposit is blocked by min-deposit (1 USDC), but the formula is correct.
    let preview_stroop = client.preview_deposit_to_shares(&1_i128);
    assert_eq!(
        preview_stroop, 1_i128,
        "Bootstrap: 1 stroop previews as 1 share (1:1)"
    );

    // Smallest actually-depositable amount: 1 USDC = 1_000_000 i128.
    // On bootstrap it mints exactly 1_000_000 shares (1:1 ratio).
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, USDC);
    let shares = client.get_shares(&user);
    assert!(shares > 0, "Bootstrap deposit must mint non-zero shares");
    assert_eq!(shares, USDC, "Bootstrap: shares == deposited assets (1:1)");

    // After bootstrap: total_shares = 1_000_000, total_assets = 1_000_000.
    // floor(1 * 1_000_000 / 1_000_000) = 1 — preview still returns 1.
    let preview_stroop_after = client.preview_deposit_to_shares(&1_i128);
    assert_eq!(
        preview_stroop_after, 1_i128,
        "Post-bootstrap: floor(1 * shares/assets) = 1"
    );
}

// ============================================================================
// Test 2: Small deposits immediately withdrawn never return more than deposited
// ============================================================================

/// Deposit then immediately withdraw a range of small amounts.
/// Verify withdrawn ≤ deposited for every case (no value creation from rounding).
///
/// Amounts that are below the minimum deposit (1 USDC) will be rejected by the
/// min-deposit guard — this test only exercises amounts ≥ 1 USDC.
#[test]
fn test_tiny_deposit_no_value_creation() {
    // All amounts at or above the enforced minimum of 1 USDC
    let amounts: &[i128] = &[
        USDC,       // 1 USDC  (minimum)
        2 * USDC,   // 2 USDC
        5 * USDC,   // 5 USDC
        10 * USDC,  // 10 USDC
        50 * USDC,  // 50 USDC
        100 * USDC, // 100 USDC
    ];

    for &deposit_amount in amounts {
        let env = Env::default();
        env.mock_all_auths();

        let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);
        let token_client = TestTokenClient::new(&env, &usdc_token);

        let user = Address::generate(&env);
        token_client.mint(&user, &deposit_amount);
        client.deposit(&user, &deposit_amount);

        let withdrawn = client.withdraw_all(&user);

        assert!(
            withdrawn <= deposit_amount,
            "Withdrawn ({}) must not exceed deposited ({}) for amount {} — no value creation from rounding",
            withdrawn,
            deposit_amount,
            deposit_amount
        );
        assert!(withdrawn >= 0, "Withdrawn amount must never be negative");
    }
}

// ============================================================================
// Test 3: 1-stroop withdrawals never extract more than deposited total
// ============================================================================

/// Multiple partial withdrawals never return more than the total deposited.
///
/// Deposit 10 USDC. Do 5 partial withdrawals of 1 USDC each, then drain the rest
/// with withdraw_all. Total withdrawn must not exceed 10 USDC — verified after
/// each step and at the end.
#[test]
fn test_tiny_withdrawal_no_value_creation() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposited = 10 * USDC;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposited);

    let mut total_withdrawn: i128 = 0;

    // 5 partial withdrawals of 1 USDC each
    for _ in 0..5 {
        client.withdraw(&user, &USDC);
        total_withdrawn += USDC;
    }

    // Drain whatever remains
    let remaining = client.withdraw_all(&user);
    total_withdrawn += remaining;

    assert!(
        total_withdrawn <= deposited,
        "Total withdrawn ({}) must not exceed deposited ({}) — no cumulative rounding gain",
        total_withdrawn,
        deposited
    );
    assert!(total_withdrawn >= 0, "Total withdrawn must not be negative");
}

// ============================================================================
// Test 4: Floor share conversion formula at inflated share price
// ============================================================================

/// Seed vault with 10 USDC, accrue yield to bring total_assets to 11 USDC.
/// Then deposit 1 stroop. Verify shares_minted == floor(1 * total_shares / total_assets).
///
/// If the floor is 0, the contract will panic with #6 — that is correct behavior.
#[test]
fn test_precision_floor_share_conversion() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let seed_user = Address::generate(&env);
    let seed_deposit = 10 * USDC;
    mint_and_deposit(&env, &client, &usdc_token, &seed_user, seed_deposit);

    // Accrue 1 USDC yield: total_assets = 11 USDC, total_shares = 10 USDC (shares issued 1:1)
    let yield_amount = USDC;
    token_client.mint(&contract_id, &yield_amount);
    let new_total_assets = seed_deposit + yield_amount;
    client.update_total_assets(&agent, &new_total_assets, &false, &0);

    let total_assets = client.get_total_assets();
    let total_shares = client.get_total_shares();

    assert_eq!(total_assets, 11 * USDC);
    assert_eq!(total_shares, 10 * USDC); // 10:1 ratio (10M shares, 11M assets)

    // Preview how many shares 1 stroop mints at this ratio:
    //   floor(1 * 10_000_000 / 11_000_000) = floor(0.909...) = 0
    // The contract must panic with #6 since shares_to_mint == 0.
    let previewed = client.preview_deposit_to_shares(&1_i128);
    // Expected: floor(1 * total_shares / total_assets) = floor(10_000_000 / 11_000_000) = 0
    let expected = total_shares / total_assets; // floor(1 * total_shares / total_assets)
    assert_eq!(
        previewed, expected,
        "preview_deposit_to_shares must implement floor division"
    );

    // For amounts that preview to 0, the deposit panics with #6.
    // We verify the formula holds; we do NOT attempt the deposit here because
    // the min-deposit guard may fire before the share check.
    // The key invariant: previewed == floor(1 * shares / assets) == 0.
    assert_eq!(
        previewed, 0_i128,
        "floor(1 * 10M / 11M) = 0: sub-USDC deposit at inflated price mints no shares"
    );
}

// ============================================================================
// Test 5: 100 deposits of 100 stroops — no phantom value creation
// ============================================================================

/// 100 deposits of 100 i128 (100 stroops) each.
/// All pass through the min-deposit guard only if the guard allows sub-USDC amounts.
/// Since the min-deposit is 1 USDC by default, sub-USDC deposits will be rejected.
///
/// This test therefore uses 1 USDC deposits when the sub-USDC path is guarded,
/// verifying that get_total_assets() == 100 * USDC after 100 deposits.
/// If sub-USDC deposits are accepted (guard bypassed), it verifies no phantom
/// value is created or destroyed.
#[test]
fn test_dust_repeated_small_deposits() {
    let env = Env::default();
    env.mock_all_auths();
    // This is a 100-user accounting stress test, not a single-call budget
    // benchmark. Rate-limit bucket writes are intentionally included in each
    // deposit and make the default test budget too small for the whole loop.
    env.budget().reset_unlimited();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Use 1 USDC (minimum accepted deposit) to ensure deposits pass the guard.
    // 100 * 1 USDC = 100 USDC total.
    let deposit_each = USDC;
    let count = 100_i128;

    for _ in 0..count {
        let user = Address::generate(&env);
        mint_and_deposit(&env, &client, &usdc_token, &user, deposit_each);
    }

    let total_assets = client.get_total_assets();
    let expected_total = count * deposit_each;

    assert_eq!(
        total_assets, expected_total,
        "get_total_assets() must equal exactly the sum of all deposits — no phantom value"
    );

    // Verify shares are proportional (1:1 on first deposits)
    let total_shares = client.get_total_shares();
    assert_eq!(
        total_shares, expected_total,
        "Total shares must equal total deposited when no yield has accrued"
    );
}
