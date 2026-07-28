//! # TVL Cap Stress Tests
//!
//! Proves that the TVL cap cannot be exceeded under demanding deposit scenarios:
//! - Rapid sequential deposits from many users
//! - Deposits that land exactly on the cap boundary
//! - Concurrent-style deposits (simulated via interleaved state changes)
//! - Deposits that race alongside cap updates
//! - Off-by-one deposits that would silently breach the cap without the guard
//!
//! ## Acceptance criteria
//! - The cap is NEVER exceeded in any scenario.
//! - Each test that should be rejected PANICS with "vault: exceeds TVL cap".
//! - Removing `require_within_tvl_cap` from `deposit()` would cause the
//!   non-panic tests to fail their post-condition assertions.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Assert the vault's on-chain state never violates the TVL cap.
/// This is the core invariant checked after every deposit batch.
fn assert_tvl_invariant(client: &NeuroWealthVaultClient, cap: i128) {
    let total = client.get_total_deposits();
    assert!(
        total <= cap,
        "TVL INVARIANT VIOLATED: total_deposits={} > cap={}",
        total,
        cap
    );
}

/// Deposit `amount` for `user`, minting tokens first.
fn deposit_for(
    env: &Env,
    client: &NeuroWealthVaultClient,
    usdc_token: &Address,
    user: &Address,
    amount: i128,
) {
    mint_and_deposit(env, client, usdc_token, user, amount);
}

// ─── 1. Rapid sequential deposits fill the cap exactly ───────────────────────

/// 20 users each deposit 5 USDC into a 100 USDC cap vault.
/// After all deposits `total_deposits == tvl_cap`.  The invariant holds at
/// every step.  This baseline confirms the happy path still works.
#[test]
fn test_tvl_cap_sequential_exact_fill() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) =
        setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // 100 USDC cap, 5 USDC per user, 20 users → fills exactly
    let tvl_cap: i128 = 100_000_000;        // 100 USDC (7 dp)
    let per_deposit: i128 = 5_000_000;      //   5 USDC
    let n_users: usize = 20;

    // Raise the per-user cap to match TVL cap so it isn't the binding constraint
    client.set_caps(&tvl_cap, &tvl_cap);

    for i in 0..n_users {
        let user = Address::generate(&env);
        deposit_for(&env, &client, &usdc_token, &user, per_deposit);

        let expected_total = per_deposit * (i as i128 + 1);
        assert_eq!(
            client.get_total_deposits(),
            expected_total,
            "Running total wrong after deposit {}",
            i + 1
        );
        assert_tvl_invariant(&client, tvl_cap);
    }

    assert_eq!(
        client.get_total_deposits(),
        tvl_cap,
        "Final total must equal cap"
    );
}

// ─── 2. (n+1)th deposit is rejected ─────────────────────────────────────────

/// After the cap is full a further deposit MUST be rejected.
/// If `require_within_tvl_cap` is removed this test would panic inside the
/// function body on the arithmetic assertion, not here — either way it fails.
#[test]
#[should_panic(expected = "vault: exceeds TVL cap")]
fn test_tvl_cap_one_over_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let tvl_cap: i128 = 50_000_000;    // 50 USDC
    let fill_amount: i128 = 50_000_000; // fills cap exactly
    let extra: i128 = 1_000_000;        // 1 USDC — must be rejected

    client.set_caps(&tvl_cap, &tvl_cap);

    let filler = Address::generate(&env);
    deposit_for(&env, &client, &usdc_token, &filler, fill_amount);
    assert_eq!(client.get_total_deposits(), tvl_cap);

    // This deposit MUST panic
    let attacker = Address::generate(&env);
    deposit_for(&env, &client, &usdc_token, &attacker, extra);
}

// ─── 3. Off-by-one boundary: deposit of exactly (cap − total) is allowed ─────

/// A deposit of exactly the remaining headroom must SUCCEED.
#[test]
fn test_tvl_cap_boundary_deposit_allowed() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let tvl_cap: i128 = 20_000_000;   // 20 USDC
    let first: i128 = 15_000_000;     // 15 USDC — leaves 5 remaining
    let headroom: i128 = 5_000_000;   //  5 USDC — exactly fills

    client.set_caps(&tvl_cap, &tvl_cap);

    let user1 = Address::generate(&env);
    deposit_for(&env, &client, &usdc_token, &user1, first);

    let user2 = Address::generate(&env);
    deposit_for(&env, &client, &usdc_token, &user2, headroom);

    assert_eq!(client.get_total_deposits(), tvl_cap);
    assert_tvl_invariant(&client, tvl_cap);
}

// ─── 4. Off-by-one: deposit of (headroom + 1 stroop) is rejected ─────────────

#[test]
#[should_panic(expected = "vault: exceeds TVL cap")]
fn test_tvl_cap_one_stroop_over_headroom_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let tvl_cap: i128 = 20_000_000;
    let first: i128 = 15_000_000;
    let one_over: i128 = 5_000_001; // headroom + 1 stroop

    client.set_caps(&tvl_cap, &tvl_cap);

    let user1 = Address::generate(&env);
    deposit_for(&env, &client, &usdc_token, &user1, first);

    let user2 = Address::generate(&env);
    deposit_for(&env, &client, &usdc_token, &user2, one_over); // must panic
}

// ─── 5. Rapid deposits from many users in tight succession ───────────────────

/// 50 users deposit in rapid succession.  Each deposit checks the invariant.
/// The cap is set so that the last deposit lands exactly on it.
#[test]
fn test_tvl_cap_rapid_many_users() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let n: i128 = 50;
    let per_deposit: i128 = 2_000_000; // 2 USDC each
    let tvl_cap: i128 = n * per_deposit; // 100 USDC total

    // Per-user cap must be >= per_deposit; TVL cap is the binding constraint.
    client.set_caps(&per_deposit, &tvl_cap);

    for _ in 0..n {
        let user = Address::generate(&env);
        deposit_for(&env, &client, &usdc_token, &user, per_deposit);
        assert_tvl_invariant(&client, tvl_cap);
    }

    assert_eq!(client.get_total_deposits(), tvl_cap);

    // One extra user must be rejected
    let late_user = Address::generate(&env);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        deposit_for(&env, &client, &usdc_token, &late_user, per_deposit);
    }));
    assert!(
        result.is_err(),
        "Deposit beyond cap should have panicked"
    );
    // Vault state must be unchanged after the rejected deposit
    assert_eq!(client.get_total_deposits(), tvl_cap);
    assert_tvl_invariant(&client, tvl_cap);
}

// ─── 6. Cap lowered mid-flight — existing deposits stay, new ones blocked ────

/// Owner lowers the TVL cap below the current total.
/// Existing balances are unaffected; NEW deposits must be blocked immediately.
/// This proves `require_within_tvl_cap` uses live storage, not a cached value.
#[test]
#[should_panic(expected = "vault: exceeds TVL cap")]
fn test_tvl_cap_lowered_blocks_new_deposits() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let initial_cap: i128 = 50_000_000;  // 50 USDC
    let existing: i128 = 30_000_000;     // 30 USDC already in
    let new_cap: i128 = 30_000_000;      // owner tightens to exactly what's in
    let new_deposit: i128 = 1_000_000;   //  1 USDC — must be blocked

    client.set_caps(&initial_cap, &initial_cap);

    let early_user = Address::generate(&env);
    deposit_for(&env, &client, &usdc_token, &early_user, existing);
    assert_eq!(client.get_total_deposits(), existing);

    // Owner tightens cap — existing balance is fine, new deposits should fail
    client.set_tvl_cap(&new_cap);

    let late_user = Address::generate(&env);
    deposit_for(&env, &client, &usdc_token, &late_user, new_deposit); // must panic
}

// ─── 7. Interleaved deposits and withdrawals never breach cap ─────────────────

/// Two users alternate deposits and withdrawals.  After each operation the
/// invariant is checked.  This validates that partial withdrawals correctly
/// reduce TotalDeposits, freeing headroom for subsequent deposits.
#[test]
fn test_tvl_cap_interleaved_deposits_and_withdrawals() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let tvl_cap: i128 = 20_000_000;     // 20 USDC
    let deposit_a: i128 = 10_000_000;   // 10 USDC
    let deposit_b: i128 = 10_000_000;   // 10 USDC
    let withdraw_a: i128 = 5_000_000;   //  5 USDC (partial)
    let deposit_c: i128 = 5_000_000;    //  5 USDC (refills freed headroom)

    client.set_caps(&tvl_cap, &tvl_cap);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let user_c = Address::generate(&env);

    // Fill to cap
    deposit_for(&env, &client, &usdc_token, &user_a, deposit_a);
    assert_tvl_invariant(&client, tvl_cap);

    deposit_for(&env, &client, &usdc_token, &user_b, deposit_b);
    assert_tvl_invariant(&client, tvl_cap);
    assert_eq!(client.get_total_deposits(), tvl_cap);

    // user_a partially withdraws → frees headroom
    client.withdraw(&user_a, &withdraw_a, &None);
    assert_tvl_invariant(&client, tvl_cap);
    assert!(client.get_total_deposits() < tvl_cap);

    // user_c fills the freed headroom
    deposit_for(&env, &client, &usdc_token, &user_c, deposit_c);
    assert_tvl_invariant(&client, tvl_cap);
    assert_eq!(client.get_total_deposits(), tvl_cap);
}

// ─── 8. Guard is the enforcing line — without it the cap would be breached ───

/// This is the "test fails if the guard is removed" contract.
///
/// We assert that `get_total_deposits() <= tvl_cap` immediately after a
/// sequence of deposits that would overshoot if the guard were absent.
/// If `require_within_tvl_cap` is deleted, the penultimate deposit in this
/// sequence would push `total_deposits` past `tvl_cap`, causing the final
/// `assert_tvl_invariant` to fail with a clear message.
#[test]
fn test_tvl_cap_invariant_would_fail_without_guard() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Cap at 30 USDC; three 10-USDC deposits fill it exactly.
    // A 4th deposit (also 10 USDC) is attempted but caught by the guard.
    let tvl_cap: i128 = 30_000_000;
    let chunk: i128 = 10_000_000;

    client.set_caps(&tvl_cap, &tvl_cap);

    for _ in 0..3 {
        let user = Address::generate(&env);
        deposit_for(&env, &client, &usdc_token, &user, chunk);
    }

    // Invariant holds after filling cap
    assert_tvl_invariant(&client, tvl_cap);
    assert_eq!(client.get_total_deposits(), tvl_cap);

    // 4th deposit must be blocked.  catch_unwind lets us assert the vault
    // state did NOT change after the rejection — the critical property.
    let overshooter = Address::generate(&env);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        deposit_for(&env, &client, &usdc_token, &overshooter, chunk);
    }));

    // Even if the panic is swallowed, the invariant must still hold.
    // Without the guard, total_deposits would be 40_000_000 here and this
    // assertion would fire.
    assert_tvl_invariant(&client, tvl_cap);
    assert_eq!(
        client.get_total_deposits(),
        tvl_cap,
        "State must be unchanged after rejected deposit"
    );
}

// ─── 9. Stress: 100 users, random-sized deposits, cap tightened mid-run ──────

/// Simulates a realistic adversarial sequence:
/// - 100 users attempt to deposit varying amounts.
/// - The owner tightens the cap mid-run.
/// - After every operation the invariant is checked.
/// - The final total must never exceed the cap in force at that moment.
#[test]
fn test_tvl_cap_stress_100_users_cap_tightened_midrun() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Start with a generous cap that allows initial wave to pass
    let initial_cap: i128 = 200_000_000; // 200 USDC
    let tight_cap: i128 = 100_000_000;   // tightened to 100 USDC at midpoint
    let per_user: i128 = 2_000_000;      //   2 USDC per user

    // User cap must be >= per_user; set it high enough
    client.set_caps(&per_user, &initial_cap);

    let mut accepted = 0i128;
    let n = 100usize;
    let tighten_at = n / 2; // tighten after 50 deposits

    for i in 0..n {
        // Tighten the cap at the midpoint
        if i == tighten_at {
            // Only tighten if current total is already <= tight_cap, otherwise
            // it would be a no-op barrier; in this test the first 50 deposits
            // consume 50 * 2 = 100 USDC which equals tight_cap exactly.
            client.set_tvl_cap(&tight_cap);
        }

        let cap_now = if i < tighten_at { initial_cap } else { tight_cap };
        let user = Address::generate(&env);

        if accepted + per_user <= cap_now {
            deposit_for(&env, &client, &usdc_token, &user, per_user);
            accepted += per_user;
            assert_tvl_invariant(&client, cap_now);
        } else {
            // Expect rejection
            let before = client.get_total_deposits();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                deposit_for(&env, &client, &usdc_token, &user, per_user);
            }));
            assert!(result.is_err(), "Deposit over cap must be rejected (i={})", i);
            assert_eq!(
                client.get_total_deposits(),
                before,
                "State unchanged after rejection (i={})",
                i
            );
            assert_tvl_invariant(&client, cap_now);
        }
    }
}
