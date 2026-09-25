//! Index pruning load test (Issue #440).
//!
//! Verifies that the prune-on-full-withdrawal mechanism removes users from the
//! `UserSharesIndex` when their shares reach zero, preventing append-only index
//! growth and the associated performance degradation.
//!
//! Tests demonstrate:
//! 1. Index size stays bounded when users deposit and then fully withdraw
//! 2. Performance of deposit() calls remains stable even after many withdrawal cycles
//! 3. Large-scale (> 500 users) scenarios to quantify improvement over the pre-pruning baseline

extern crate std;

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Resets the env budget to zero, runs `f`, and returns (cpu, mem).
fn measure<F: FnOnce()>(env: &Env, f: F) -> (u64, u64) {
    let mut budget = env.budget();
    budget.reset_unlimited();
    f();
    (
        env.budget().cpu_instruction_cost(),
        env.budget().memory_bytes_cost(),
    )
}

/// Deposits from a fresh user and then withdraws all, ensuring the index is pruned.
fn deposit_and_withdraw_all(
    env: &Env,
    client: &NeuroWealthVaultClient,
    usdc_token: &Address,
    amount: i128,
) -> Address {
    let token_client = TestTokenClient::new(env, usdc_token);
    let user = Address::generate(env);
    token_client.mint(&user, &amount);

    client.deposit(&user, &amount);
    client.withdraw_all(&user, &None);

    user
}

/// Deposits from a fresh user and then partially withdraws.
fn deposit_and_partial_withdraw(
    env: &Env,
    client: &NeuroWealthVaultClient,
    usdc_token: &Address,
    amount: i128,
) -> Address {
    let token_client = TestTokenClient::new(env, usdc_token);
    let user = Address::generate(env);
    token_client.mint(&user, &amount);

    client.deposit(&user, &amount);
    // Withdraw half (rounded down), leaving the user in the index
    let half = amount / 2;
    client.withdraw(&user, &half, &None);

    user
}

/// Verify that the index size doesn't grow when users withdraw to zero.
/// If pruning is working, the index should stay at or below the number of
/// users with non-zero shares.
#[test]
fn test_index_pruning_keeps_size_bounded() {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Raise TVL cap for large-scale test
    client.set_tvl_cap(&10_000_000_000_000_i128);

    let deposit_amount = 10_000_000_i128;
    let cycle_count = 100;

    // Cycle 1: Deposit and withdraw, building the index up without pruning would
    // accumulate dead entries. With pruning, the index should stay small.
    for _ in 0..cycle_count {
        deposit_and_withdraw_all(&env, &client, &usdc_token, deposit_amount);
    }

    // Get the index size via get_users_with_shares
    // We expect to see 0 users with shares since everyone withdrew.
    let (users, _) = client.get_users_with_shares(&0, &1000);
    let index_size = users.len() as u32;

    std::println!("[pruning] After {cycle_count} deposit+withdraw_all cycles: index size = {index_size}");

    // With pruning working, the index should have 0 entries (all users withdrawn).
    // Without pruning, it would have 100 entries of dead users.
    assert_eq!(
        index_size, 0,
        "Index not pruned correctly: expected 0 users after full withdrawals, got {index_size}"
    );
}

/// Verify that index size grows only with active (non-zero-share) users.
#[test]
fn test_index_tracks_active_users_only() {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_tvl_cap(&10_000_000_000_000_i128);

    let deposit_amount = 10_000_000_i128;

    // Phase 1: Create 50 users with active shares
    let mut active_users = Vec::new();
    for _ in 0..50 {
        let token_client = TestTokenClient::new(&env, &usdc_token);
        let user = Address::generate(&env);
        token_client.mint(&user, &deposit_amount);
        client.deposit(&user, &deposit_amount);
        active_users.push(user);
    }

    let (users_phase1, _) = client.get_users_with_shares(&0, &1000);
    std::println!("[pruning] After 50 deposits: index size = {}", users_phase1.len());
    assert_eq!(users_phase1.len(), 50, "Index should have 50 active users");

    // Phase 2: Have 25 of them fully withdraw (prune should remove them)
    for i in 0..25 {
        client.withdraw_all(&active_users[i], &None);
    }

    let (users_phase2, _) = client.get_users_with_shares(&0, &1000);
    std::println!("[pruning] After 25 withdraw_all: index size = {}", users_phase2.len());
    assert_eq!(
        users_phase2.len(),
        25,
        "Index should have 25 active users after pruning"
    );

    // Verify the remaining users are the correct ones
    for remaining_user in users_phase2.iter() {
        let remaining_addr = remaining_user.0.clone();
        // Check that this user is in active_users[25..50]
        assert!(
            active_users[25..].contains(&remaining_addr),
            "Remaining user should be from the non-withdrawn group"
        );
    }
}

/// Deposit performance should remain stable when pruning is active,
/// even after many cycles of deposit/withdrawal.
#[test]
fn test_deposit_performance_stable_with_pruning() {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_tvl_cap(&10_000_000_000_000_i128);

    let deposit_amount = 10_000_000_i128;

    // Warm up: create and destroy 100 users to build a pruned index
    for _ in 0..100 {
        deposit_and_withdraw_all(&env, &client, &usdc_token, deposit_amount);
    }

    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Measure the cost of depositing into a fresh user after 100 withdraw cycles
    let user_after_cycles = Address::generate(&env);
    token_client.mint(&user_after_cycles, &deposit_amount);

    let (cpu_after_cycles, mem_after_cycles) = measure(&env, || {
        client.deposit(&user_after_cycles, &deposit_amount);
    });

    std::println!(
        "[pruning] Deposit cost after 100 deposit+withdraw cycles: cpu={cpu_after_cycles} mem={mem_after_cycles}"
    );

    // The cost should be comparable to a fresh deposit on an empty vault.
    // Without pruning, this would show significant regression (10-50x).
    // With pruning, it should be roughly O(1) with respect to historical cycle count.
    assert!(
        cpu_after_cycles < 5_000_000,
        "Deposit CPU cost regressed after cycles: {cpu_after_cycles}"
    );
    assert!(
        mem_after_cycles < 300_000,
        "Deposit memory cost regressed after cycles: {mem_after_cycles}"
    );
}

/// Large-scale (500+) performance test: measure index size and deposit cost.
/// With pruning, both should remain bounded. Without pruning, both would
/// scale with the total number of distinct addresses ever deposited.
#[test]
fn test_large_scale_index_pruning_500_plus_users() {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_tvl_cap(&100_000_000_000_000_i128);

    let deposit_amount = 1_000_000_i128; // Small amounts to stay within budget
    const CYCLE_COUNT: u32 = 600;

    // Execute 600 full deposit+withdraw cycles
    for cycle in 0..CYCLE_COUNT {
        if cycle % 100 == 0 {
            std::println!("[pruning] Completed {cycle}/{CYCLE_COUNT} cycles");
        }
        deposit_and_withdraw_all(&env, &client, &usdc_token, deposit_amount);
    }

    // Verify index is empty (all users withdrawn)
    let (users_final, _) = client.get_users_with_shares(&0, &1000);
    std::println!(
        "[pruning] After {CYCLE_COUNT} cycles: index size = {}",
        users_final.len()
    );

    assert_eq!(
        users_final.len(),
        0,
        "Index should be empty after all full withdrawals; got {} users",
        users_final.len()
    );

    // Now measure deposit cost with a pruned, essentially-empty index
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let final_user = Address::generate(&env);
    token_client.mint(&final_user, &deposit_amount);

    let (cpu_final, mem_final) = measure(&env, || {
        client.deposit(&final_user, &deposit_amount);
    });

    std::println!(
        "[pruning] Final deposit cost (after {CYCLE_COUNT} cycles): cpu={cpu_final} mem={mem_final}"
    );

    // Cost should remain reasonable despite the historical cycle count
    assert!(
        cpu_final < 5_000_000,
        "Deposit CPU cost unexpectedly high: {cpu_final}"
    );
    assert!(
        mem_final < 300_000,
        "Deposit memory cost unexpectedly high: {mem_final}"
    );
}

/// Verify that partial withdrawals do NOT prune (user remains in index).
#[test]
fn test_partial_withdraw_does_not_prune() {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_tvl_cap(&10_000_000_000_000_i128);

    let deposit_amount = 100_000_000_i128;

    // Deposit
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    token_client.mint(&user, &deposit_amount);
    client.deposit(&user, &deposit_amount);

    // Partial withdraw (50%)
    let half = deposit_amount / 2;
    client.withdraw(&user, &half, &None);

    // User should still be in the index
    let (users, _) = client.get_users_with_shares(&0, &1000);
    let found = users.iter().any(|(addr, _)| addr == &user);
    assert!(found, "User should remain in index after partial withdrawal");

    // Full withdraw
    client.withdraw_all(&user, &None);

    // User should now be pruned
    let (users_after, _) = client.get_users_with_shares(&0, &1000);
    let found_after = users_after.iter().any(|(addr, _)| addr == &user);
    assert!(
        !found_after,
        "User should be pruned from index after full withdrawal"
    );
}
