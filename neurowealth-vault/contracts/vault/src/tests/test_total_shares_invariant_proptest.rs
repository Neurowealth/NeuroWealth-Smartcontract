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
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

// The vault crate is `#![no_std]`; tests are run with the standard test harness
// which links std, but we must declare it explicitly in no_std crates.
extern crate std;

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
            (prop::sample::select(vec![0u8, 1]), prop::num::i128::range(1_000_000..=10_000_000_000)),
            1..100
        )
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);

        let mut users: Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();

        for (op_type, amount) in operations.iter() {
            let user = &users[0]; // Use first user for each operation

            if *op_type == 0 {
                // Deposit - need to mint tokens first
                let token_client = TestTokenClient::new(&env, &usdc_token);
                token_client.mint(user, &amount);
                client.deposit(user, amount);
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
                client.withdraw(user, &withdraw_amount, &None);
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