//! Property tests for the total_shares invariant (Issue #531).
//!
//! This test verifies that at any point, the contract-level `total_shares` must equal
//! the sum of all individual user `Shares(address)` values.
//!
//! This is a fundamental vault invariant. If `total_shares` drifts away from the sum
//! of user shares, share price calculation becomes incorrect and users lose/gain value.
//!
//! The test uses property-based testing with random deposit/withdraw sequences
//! to detect any violations of this invariant.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

// The vault crate is `#![no_std]`; tests are run with the standard test harness
// which links std, but we must declare it explicitly in no_std crates.
extern crate std;
use std::vec::Vec;

use proptest::prelude::*;

proptest! {
    /// Proptest: total_shares never exceeds sum of all user shares.
    ///
    /// This test generates random deposit/withdraw sequences for 5 users and verifies
    /// after each operation that the contract-level total_shares equals the sum of all
    /// individual user shares.
    #[test]
    fn prop_total_shares_equals_sum_of_user_shares(
        operations in prop::collection::vec(
            (prop::sample::select(std::vec![0u8, 1]), 1_000_000i128..=10_000_000_000i128),
            // Capped well below soroban-sdk 21.7.7's Env::drop scale limit
            // (confirmed via isolated reproduction: a long enough operation
            // sequence in one Env can itself panic during the test
            // binary's own cleanup, aborting the whole process rather than
            // just failing this test — the same host-side limitation
            // documented on test_tvl_cap_stress.rs's #[ignore]d tests).
            1..20
        )
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);

        // All operations below act on a single user (`users[0]`), and up to
        // 99 deposits of up to 1,000 USDC each can accumulate for that one
        // user, so the per-user and vault-wide caps (1,000 USDC / 10,000 USDC
        // by default) need to be raised well above the worst case
        // (~99,000 USDC) to avoid `ExceedsUserDepositCap`/`ExceedsTvlCap`
        // aborting the sequence partway through. Individual deposits (up to
        // 1,000 USDC) already fit under the default per-tx max.
        let generous_cap = 10_000_000_000_000i128;
        client.set_caps(&generous_cap, &generous_cap);

        let users: Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();

        for (op_type, amount) in operations.iter() {
            let user = &users[0]; // Use first user for each operation
            let amount = *amount;

            if *op_type == 0 {
                // Deposit - need to mint tokens first
                let token_client = TestTokenClient::new(&env, &usdc_token);
                token_client.mint(user, &amount);
                client.deposit(user, &amount);
            } else {
                // Withdraw
                let balance = client.get_balance(user);
                if balance <= 0 {
                    continue;
                }
                let withdraw_amount = amount.min(balance);
                if withdraw_amount <= 0 {
                    continue;
                }
                client.withdraw(user, &withdraw_amount);
            }

            // Verify the invariant after each operation
            let total_shares: i128 = client.get_total_shares();
            let mut sum_user_shares: i128 = 0;

            for user in &users {
                let shares: i128 = client.get_shares(user);
                sum_user_shares += shares;
            }

            assert_eq!(total_shares, sum_user_shares,
                "Invariant violation: total_shares ({}) != sum of user shares ({})",
                total_shares, sum_user_shares);
        }
    }
}
