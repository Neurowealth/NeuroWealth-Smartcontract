//! Stress tests for TVL cap enforcement via many serial small deposits (Issue #257).
//!
//! Unlike single-deposit cap tests, these verify that cumulative deposits from
//! many users are correctly blocked once the cap is reached.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

// 1 USDC = 1_000_000 i128 (7 decimal places on Stellar)
const USDC: i128 = 1_000_000;

// ============================================================================
// Test 1: 10 deposits of 100 USDC with cap=1000 USDC
// ============================================================================

/// After exactly 10 deposits of 100 USDC (TVL == cap), the 11th must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #41)")]
fn test_serial_deposits_reach_exact_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let tvl_cap = 1_000 * USDC; // 1000 USDC
    client.set_tvl_cap(&tvl_cap);

    // 10 deposits of 100 USDC each => TVL reaches exactly cap
    let deposit_amount = 100 * USDC;
    for _ in 0..10 {
        let user = Address::generate(&env);
        mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    }

    assert_eq!(
        client.get_total_assets(),
        tvl_cap,
        "TVL should equal the cap after 10 deposits"
    );

    // 11th deposit must be rejected
    let late_user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &late_user, deposit_amount);
}

// ============================================================================
// Test 2: 999 deposits of 1 USDC, 1000th succeeds, 1001st fails
// ============================================================================

/// Cap=10 USDC. After 9 deposits of 1 USDC the 10th must succeed (fills cap),
/// and the 11th must be rejected with #41.
#[test]
#[should_panic(expected = "Error(Contract, #41)")]
fn test_serial_deposits_cap_minus_one() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let tvl_cap = 10 * USDC;
    client.set_tvl_cap(&tvl_cap);

    // 9 deposits of 1 USDC
    for _ in 0..9 {
        let user = Address::generate(&env);
        mint_and_deposit(&env, &client, &usdc_token, &user, USDC);
    }

    assert_eq!(client.get_total_assets(), 9 * USDC);

    // 10th deposit of 1 USDC must succeed (TVL == cap exactly)
    let user_final = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user_final, USDC);
    assert_eq!(client.get_total_assets(), tvl_cap);

    // 11th deposit must be rejected
    let user_over = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user_over, USDC);
}

// ============================================================================
// Test 3: Explicit cap-1 / cap / cap+1 boundary check
// ============================================================================

/// Cap=100 USDC. Deposit 99 USDC (cap - 1 USDC), then exactly 1 USDC (fills cap),
/// then 1 USDC more — must be rejected with #41.
///
/// Note: the minimum deposit is 1 USDC, so 1 USDC is the smallest overage that
/// triggers the TVL cap check rather than the min-deposit guard.
#[test]
#[should_panic(expected = "Error(Contract, #41)")]
fn test_serial_deposits_cap_boundary_off_by_one() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let tvl_cap = 100 * USDC; // 100 USDC = 100_000_000 i128
    client.set_tvl_cap(&tvl_cap);

    // Deposit 99 USDC (cap - 1 USDC) — must succeed
    let user1 = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user1, 99 * USDC);
    assert_eq!(client.get_total_assets(), 99 * USDC);

    // Deposit exactly 1 USDC (fills cap to exactly 100 USDC) — must succeed
    let user2 = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user2, USDC);
    assert_eq!(
        client.get_total_assets(),
        tvl_cap,
        "TVL should equal cap exactly"
    );

    // Any further deposit (minimum is 1 USDC) must be rejected with #41
    let user3 = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user3, USDC);
}

// ============================================================================
// Test 4: Serial deposits do not overflow and produce correct TVL
// ============================================================================

/// 20 deposits of 5 USDC each with cap=101 USDC.
/// All succeed. Final TVL == 100 USDC. No arithmetic overflow.
#[test]
fn test_serial_deposits_no_overflow() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let tvl_cap = 101 * USDC;
    client.set_tvl_cap(&tvl_cap);

    let deposit_each = 5 * USDC;
    let count = 20_i128;

    for _ in 0..count {
        let user = Address::generate(&env);
        mint_and_deposit(&env, &client, &usdc_token, &user, deposit_each);
    }

    let final_tvl = client.get_total_assets();
    assert_eq!(
        final_tvl,
        count * deposit_each,
        "Final TVL must be exactly 100 USDC"
    );
    assert!(final_tvl < tvl_cap, "Final TVL must remain below the cap");
}

// ============================================================================
// Test 5: Deterministic — same sequence in two envs produces identical state
// ============================================================================

/// Running the same deposit sequence twice in independent environments must
/// produce identical TVL and total share counts.
#[test]
fn test_serial_deposits_deterministic() {
    const DEPOSIT_COUNT: usize = 10;
    const DEPOSIT_EACH: i128 = 5 * USDC; // 5 USDC each

    let run = || {
        let env = Env::default();
        env.mock_all_auths();

        let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);

        // No cap — let all deposits through
        for _ in 0..DEPOSIT_COUNT {
            // Use a deterministic seed-style address generation
            let user = Address::generate(&env);
            mint_and_deposit(&env, &client, &usdc_token, &user, DEPOSIT_EACH);
        }

        (client.get_total_assets(), client.get_total_shares())
    };

    let (tvl_a, shares_a) = run();
    let (tvl_b, shares_b) = run();

    assert_eq!(tvl_a, tvl_b, "TVL must be identical across runs");
    assert_eq!(
        shares_a, shares_b,
        "Total shares must be identical across runs"
    );

    // Sanity: TVL must equal total deposited
    assert_eq!(
        tvl_a,
        (DEPOSIT_COUNT as i128) * DEPOSIT_EACH,
        "TVL must equal sum of all deposits"
    );
}

// ============================================================================
// Issue #539: Lowering TVL cap below current TotalAssets must not block withdrawals
// ============================================================================

/// Lower cap below TotalAssets, withdraw full balance, then verify the cap
/// still gates new deposits.
#[test]
#[should_panic(expected = "Error(Contract, #41)")]
fn test_tvl_cap_lowered_below_assets_does_not_block_withdrawals() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let deposit_amount = 8_000 * USDC;
    let lowered_cap = 5_000 * USDC;

    // Step 1: Set cap to 10_000 USDC
    client.set_tvl_cap(&(10_000 * USDC));

    // Step 2: User deposits 8_000 USDC
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    assert_eq!(client.get_total_assets(), deposit_amount);

    // Step 3: Owner lowers cap to 5_000 USDC (below current TotalAssets)
    client.set_tvl_cap(&lowered_cap);

    // Step 4: User withdraws full 8_000 USDC — MUST succeed (no cap check)
    client.withdraw(&user, &deposit_amount, &None);
    assert_eq!(client.get_total_assets(), 0);

    // Step 5: New deposit exceeding the lowered cap must be rejected
    let new_user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &new_user, 6_000 * USDC);
}
