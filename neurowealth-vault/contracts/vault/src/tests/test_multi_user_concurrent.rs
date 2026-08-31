//! Multi-user concurrent deposit/withdraw invariant test (#413).
//!
//! Simulates multiple users depositing and withdrawing in interleaved order,
//! verifying the vault solvency invariant after every operation:
//!
//!   idle_balance + deployed_assets == total_assets
//!   total_shares > 0  =>  total_assets > 0
//!
//! Also verifies that every user's share-derived balance is non-negative and
//! that the sum of individual balances stays consistent with total_assets.

extern crate std;

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};
use std::vec::Vec;

const MIN_DEPOSIT: i128 = 1_000_000;
const MAX_DEPOSIT: i128 = 10_000_000_000;
const NUM_USERS: usize = 5;
const STEPS: usize = 100;

fn assert_vault_invariants(client: &NeuroWealthVaultClient, users: &[Address]) {
    let total_assets = client.get_total_assets();
    let total_shares = client.get_total_shares();
    let idle = client.get_idle_balance();
    let deployed = client.get_deployed_assets();

    // Core invariant: idle + deployed == total_assets
    assert_eq!(
        idle + deployed,
        total_assets,
        "solvency invariant violated: idle({}) + deployed({}) != total_assets({})",
        idle,
        deployed,
        total_assets
    );

    // If there are shares, there must be assets
    assert!(
        total_shares <= 0 || total_assets > 0,
        "share invariant violated: total_shares({}) > 0 but total_assets({}) <= 0",
        total_shares,
        total_assets
    );

    // Each user's balance must be non-negative
    let mut sum_balances: i128 = 0;
    for user in users {
        let bal = client.get_balance(user);
        assert!(bal >= 0, "user balance cannot be negative: got {}", bal);
        sum_balances = sum_balances.checked_add(bal).expect("balance sum overflow");
    }

    // The total of all user get_balance() should be <= total_assets.
    // It can be slightly less due to rounding (floor conversion).
    assert!(
        sum_balances <= total_assets,
        "balance invariant violated: sum_user_balances({}) > total_assets({})",
        sum_balances,
        total_assets
    );
}

/// Simpler pseudo-random number generator (PCG minimal).
fn pcg(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    *state
}

#[test]
fn test_multi_user_concurrent_deposit_withdraw() {
    let env = Env::default();
    env.mock_all_auths();
    // This is a 100-step invariant simulation, not a resource benchmark.
    // Rate-limit bucket bookkeeping intentionally adds storage work per call.
    env.budget().reset_unlimited();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Create multiple users, each funded with 50_000 XLM worth of tokens
    let mut users: Vec<Address> = Vec::with_capacity(NUM_USERS);
    for _ in 0..NUM_USERS {
        let user = Address::generate(&env);
        token.mint(&user, &50_000_000_000);
        assert_eq!(token.balance(&user), 50_000_000_000);
        users.push(user);
    }

    let mut rng: u64 = 0xDEAD_BEEF_CAFE_u64;

    for step in 0..STEPS {
        let user_idx = (pcg(&mut rng) as usize) % NUM_USERS;
        let user = &users[user_idx];
        let op = pcg(&mut rng) % 4; // 0=deposit, 1=withdraw, 2=deposit, 3=withdraw

        // Determine amount from PRNG
        let amount_base = (pcg(&mut rng) as i128 % 100) + 1; // 1..=100
        let amount = MIN_DEPOSIT * amount_base;

        match op {
            0 | 2 => {
                // Deposit
                let token_balance = token.balance(user);
                if amount > MAX_DEPOSIT || token_balance < amount {
                    continue;
                }
                client.deposit(user, &amount);
            }
            1 | 3 => {
                // Withdraw
                let balance = client.get_balance(user);
                if balance < MIN_DEPOSIT {
                    continue;
                }
                let withdraw_amount = amount.min(balance / 2).max(MIN_DEPOSIT);
                client.withdraw(user, &withdraw_amount);
            }
            _ => unreachable!(),
        }

        // Verify invariants after every operation
        assert_vault_invariants(&client, &users);

        // Verify user-level consistency: shares and balance
        let shares = client.get_shares(user);
        let balance = client.get_balance(user);
        assert!(
            shares >= 0,
            "negative shares for user {} at step {}",
            user_idx,
            step
        );
        assert!(
            balance >= 0,
            "negative balance for user {} at step {}",
            user_idx,
            step
        );
        // If user has shares, they should have a balance (or be zero after withdraw-all)
        // If user has no shares, balance must be 0
        if shares == 0 {
            assert_eq!(
                balance, 0,
                "user {} has zero shares but non-zero balance {} at step {}",
                user_idx, balance, step
            );
        }
    }

    // Final invariant check
    assert_vault_invariants(&client, &users);

    // Attempt to withdraw everything from all users
    for user in &users {
        let balance = client.get_balance(user);
        if balance > 0 {
            client.withdraw(user, &balance);
        }
        assert_eq!(client.get_balance(user), 0);
        assert_eq!(client.get_shares(user), 0);
    }

    // After full withdrawal, idle should be 0, deployed may still have assets
    // Total assets should be >= 0
    assert!(client.get_total_assets() >= 0);
    // If all shares are zero, the vault may still have residual assets from deployed
    // positions — that's expected.
}

// ============================================================================
// ADDITIONAL CONCURRENT SCENARIOS (#476)
// ============================================================================

/// Two users deposit sequentially → verify share proportions match contributions.
#[test]
fn test_concurrent_two_users_sequential_share_proportions() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let amount_a = 5_000_000_i128;
    let amount_b = 3_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user_a, amount_a);
    mint_and_deposit(&env, &client, &usdc_token, &user_b, amount_b);

    assert_eq!(client.get_shares(&user_a), amount_a);
    assert_eq!(client.get_shares(&user_b), amount_b);
    assert_eq!(client.get_total_shares(), amount_a + amount_b);
}

/// Two users deposit same amount → verify equal shares.
#[test]
fn test_concurrent_two_users_same_amount_equal_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let amount = 7_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user_a, amount);
    mint_and_deposit(&env, &client, &usdc_token, &user_b, amount);

    assert_eq!(client.get_shares(&user_a), client.get_shares(&user_b));
}

/// A deposits, B deposits, A withdraws → verify correct remaining amounts.
#[test]
fn test_concurrent_deposit_deposit_withdraw_correct_amounts() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let a_deposit = 10_000_000_i128;
    let b_deposit = 5_000_000_i128;
    let a_withdraw = 4_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user_a, a_deposit);
    mint_and_deposit(&env, &client, &usdc_token, &user_b, b_deposit);

    client.withdraw(&user_a, &a_withdraw);

    let a_balance = client.get_balance(&user_a);
    let b_balance = client.get_balance(&user_b);
    assert_eq!(a_balance, a_deposit - a_withdraw);
    assert_eq!(b_balance, b_deposit);

    assert_vault_invariants(&client, &[user_a.clone(), user_b.clone()]);
}

/// Multiple deposits interleaved with rebalances.
#[test]
fn test_concurrent_deposits_interleaved_with_rebalances() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);

    // Seed users
    for u in [&user_a, &user_b] {
        token_client.mint(u, &20_000_000_i128);
    }

    // Interleave deposits and rebalances
    client.deposit(&user_a, &10_000_000_i128);
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);

    client.deposit(&user_b, &8_000_000_i128);
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    client.deposit(&user_a, &5_000_000_i128);

    assert_vault_invariants(&client, &[user_a, user_b]);
}

/// Deposit during active Blend deployment → verify idle vs deployed split.
#[test]
fn test_concurrent_deposit_during_blend_deployment() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 20_000_000_i128;
    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    blend_client.set_max_supply_limit(&15_000_000_i128);
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);

    // Now deposit additional funds while Blend holds 15 USDC.
    let second_deposit = 10_000_000_i128;
    token_client.mint(&user, &second_deposit);
    client.deposit(&user, &second_deposit);

    // Vault should have some idle + deployed, total = 30
    assert_eq!(client.get_total_assets(), deposit + second_deposit);
    let idle = token_client.balance(&contract_id);
    let deployed = client.get_deployed_assets();
    assert_eq!(idle + deployed, deposit + second_deposit);
    assert_vault_invariants(&client, &[user]);
}

/// Harvest interleaved with withdrawals → vault solvency is maintained throughout.
///
/// Scenario (#522):
/// 1. Two users deposit into a Blend-backed vault.
/// 2. Agent rebalances into Blend and then calls harvest() twice, interleaved
///    with user withdrawals.
/// 3. After each operation the core invariant is verified:
///       idle_balance + deployed_assets == total_assets
/// 4. Each user can still withdraw their full remaining balance at the end.
#[test]
fn test_concurrent_harvest_interleaved_with_withdrawals() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let deposit_a = 20_000_000_i128;
    let deposit_b = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user_a, deposit_a);
    mint_and_deposit(&env, &client, &usdc_token, &user_b, deposit_b);

    assert_vault_invariants(&client, &[user_a.clone(), user_b.clone()]);

    // Deploy most of the vault into Blend (cap at 25 USDC so some idle remains).
    blend_client.set_max_supply_limit(&25_000_000_i128);
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);
    assert_vault_invariants(&client, &[user_a.clone(), user_b.clone()]);

    // User A makes a partial withdrawal while Blend holds assets.
    let withdraw_a1 = 4_000_000_i128;
    client.withdraw(&user_a, &withdraw_a1);
    assert_vault_invariants(&client, &[user_a.clone(), user_b.clone()]);

    // Agent harvests (compounds yield back into Blend).
    // Disable cooldown so harvest can fire right after rebalance.
    client.set_rebalance_cooldown(&0_u32);
    client.harvest(&0_i128);
    assert_vault_invariants(&client, &[user_a.clone(), user_b.clone()]);

    // User B makes a partial withdrawal.
    let withdraw_b1 = 3_000_000_i128;
    client.withdraw(&user_b, &withdraw_b1);
    assert_vault_invariants(&client, &[user_a.clone(), user_b.clone()]);

    // Second harvest after the second withdrawal.
    client.harvest(&0_i128);
    assert_vault_invariants(&client, &[user_a.clone(), user_b.clone()]);

    // Both users withdraw their remaining balances; vault should be fully drained.
    let balance_a = client.get_balance(&user_a);
    if balance_a > 0 {
        client.withdraw(&user_a, &balance_a);
    }
    let balance_b = client.get_balance(&user_b);
    if balance_b > 0 {
        client.withdraw(&user_b, &balance_b);
    }

    assert_eq!(client.get_shares(&user_a), 0);
    assert_eq!(client.get_shares(&user_b), 0);
    assert!(client.get_total_assets() >= 0);
}

/// Withdrawal that triggers partial Blend exit → verify remaining shares.
#[test]
fn test_concurrent_withdrawal_triggers_partial_blend_exit() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit = 20_000_000_i128;
    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Deploy 15 to Blend, leave 5 idle by capping pool at 15.
    blend_client.set_max_supply_limit(&15_000_000_i128);
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);

    let idle_before = token_client.balance(&contract_id);
    let deployed_before = token_client.balance(&blend_pool);
    assert_eq!(idle_before, 5_000_000_i128);
    assert_eq!(deployed_before, 15_000_000_i128);

    // Withdraw 7 USDC: vault pulls 5 idle and 2 from Blend (partial exit).
    let withdraw_amount = 7_000_000_i128;
    client.withdraw(&user, &withdraw_amount);

    let balance_after = client.get_balance(&user);
    assert_eq!(balance_after, deposit - withdraw_amount);
    assert_vault_invariants(&client, &[user]);
}
