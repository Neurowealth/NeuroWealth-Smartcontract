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

extern crate std;

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
// Cap-lowering-after-yield then raising scenario (issue #299 regression)
// ============================================================================

/// After yield accrual pushes `TotalAssets` past the TVL cap, a new deposit
/// is rejected with error #41.  Raising the cap above the current
/// `TotalAssets` must re-open deposits.  All three phases are verified in a
/// single continuous environment so that vault state carries across steps.
///
/// This is the canonical regression for the cap-lowering-after-yield scenario
/// called out in issue #299.  It proves three things:
///   (a) deposit is blocked with #41 once `TotalAssets > cap` after yield
///   (b) lowering the cap further keeps deposits blocked
///   (c) raising the cap above `TotalAssets` unblocks deposits again
///
/// The final assertion (`TotalDeposits != TotalAssets`) also acts as a
/// lint that the guard is reading `TotalAssets` and not the cheaper
/// `TotalDeposits` counter — if the implementation regressed to
/// `TotalDeposits` the step-5 deposit would be over-allowed and the
/// post-condition arithmetic would diverge.
#[test]
fn test_tvl_cap_lowered_after_yield_then_raised_allows_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // ── Step 1: initial cap = 12 USDC; user1 deposits 10 USDC ────────────
    // After deposit:  TotalAssets = 10, TotalDeposits = 10
    let initial_cap = 12_000_000_i128; // 12 USDC (7 dp)
    client.set_tvl_cap(&initial_cap);

    let user1 = Address::generate(&env);
    let first_deposit = 10_000_000_i128; // 10 USDC
    mint_and_deposit(&env, &client, &usdc_token, &user1, first_deposit);

    assert_eq!(client.get_total_assets(), first_deposit);
    assert_eq!(client.get_total_deposits(), first_deposit);

    // ── Step 2: accrue 3 USDC yield → TotalAssets = 13 > cap (12) ────────
    // TotalDeposits stays at 10 (principal-only counter, issue #299).
    let yield_amount = 3_000_000_i128; // 3 USDC
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(first_deposit + yield_amount), &false, &0);

    assert_eq!(client.get_total_assets(), 13_000_000_i128);
    assert_eq!(
        client.get_total_deposits(),
        first_deposit,
        "TotalDeposits must not change on yield accrual"
    );

    // ── Step 3: deposit while TotalAssets > cap must be rejected ──────────
    // Mint tokens for user2 outside the vault; the deposit itself must panic.
    let user2 = Address::generate(&env);
    token_client.mint(&user2, &1_000_000_i128);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.deposit(&user2, &1_000_000_i128);
    }));
    assert!(
        result.is_err(),
        "deposit must be rejected (#41) when TotalAssets already exceeds the cap"
    );
    // Vault must be unchanged after the rejected call.
    assert_eq!(
        client.get_total_assets(),
        13_000_000_i128,
        "TotalAssets must not change after a rejected deposit"
    );

    // ── Step 4: owner lowers cap to 10 USDC — still below TotalAssets ─────
    // Deposits must remain blocked because TotalAssets (13) > new cap (10).
    let lower_cap = 10_000_000_i128; // 10 USDC
    client.set_tvl_cap(&lower_cap);

    let user3 = Address::generate(&env);
    token_client.mint(&user3, &1_000_000_i128);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.deposit(&user3, &1_000_000_i128);
    }));
    assert!(
        result.is_err(),
        "deposit must remain blocked after cap is lowered below TotalAssets"
    );

    // ── Step 5: owner raises cap to 20 USDC > TotalAssets (13) ───────────
    // A 2 USDC deposit must now succeed:
    //   new TotalAssets = 13 + 2 = 15  ≤  20 (cap)
    let raised_cap = 20_000_000_i128; // 20 USDC
    client.set_tvl_cap(&raised_cap);

    let user4 = Address::generate(&env);
    let new_deposit = 2_000_000_i128; // 2 USDC
    mint_and_deposit(&env, &client, &usdc_token, &user4, new_deposit);

    assert_eq!(
        client.get_total_assets(),
        15_000_000_i128,
        "TotalAssets must include the new deposit"
    );
    assert!(
        client.get_total_assets() <= raised_cap,
        "TotalAssets must not exceed the raised cap"
    );

    // TotalDeposits = 10 (original) + 2 (step 5) = 12.
    // TotalAssets = 15 (13 yield-inflated + 2 new deposit).
    // The gap (15 vs 12) proves the guard evaluated TotalAssets, not TotalDeposits:
    // if the guard had used TotalDeposits (12) the step-3/4 deposits would not
    // have been blocked, and TotalDeposits would be higher here.
    assert_eq!(
        client.get_total_deposits(),
        first_deposit + new_deposit, // 12 USDC principal
        "TotalDeposits must only reflect principal"
    );
    assert!(
        client.get_total_assets() > client.get_total_deposits(),
        "TotalAssets must exceed TotalDeposits due to unrealised yield"
    );
}

/// Minimal `#[should_panic]` companion: a single deposit panics with #41
/// when `TotalAssets` (not `TotalDeposits`) is already above the cap after
/// yield accrual.
///
/// If `require_within_tvl_cap` regressed to comparing against `TotalDeposits`
/// this test would NOT panic — the deposit would silently succeed and the
/// `#[should_panic]` attribute would cause the test to fail.
#[test]
#[should_panic(expected = "Error(Contract, #41)")]
fn test_tvl_cap_blocks_deposit_when_yield_already_exceeds_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // cap = 10 USDC; deposit 8 USDC  →  TotalDeposits = 8  (under cap)
    let cap = 10_000_000_i128;
    client.set_tvl_cap(&cap);

    let user1 = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user1, 8_000_000_i128);

    // Accrue 3 USDC yield → TotalAssets = 11  (above cap)
    // TotalDeposits is still 8 — below the cap.
    // The guard MUST use TotalAssets and reject the next deposit.
    token_client.mint(&contract_id, &3_000_000_i128);
    client.update_total_assets(&agent, &11_000_000_i128, &false, &0);

    // This deposit must panic with #41 because TotalAssets (11) > cap (10).
    // It would NOT panic if the guard mistakenly checked TotalDeposits (8).
    let user2 = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user2, 1_000_000_i128);
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
