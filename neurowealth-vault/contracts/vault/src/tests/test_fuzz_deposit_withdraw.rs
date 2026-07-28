//! Deterministic smoke test for deposit/withdraw sequences (mirrors the libFuzzer harness).
//!
//! The full fuzz target lives in `neurowealth-vault/fuzz/fuzz_targets/deposit_withdraw_sequence.rs`
//! and runs on the weekly CI schedule via `cargo fuzz`.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

const MIN_DEPOSIT: i128 = 1_000_000;
const MAX_DEPOSIT: i128 = 10_000_000_000;

#[test]
fn test_deposit_withdraw_sequence_smoke() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let token = TestTokenClient::new(&env, &usdc_token);
    token.mint(&user, &50_000_000_000);

    let mut state: u64 = 0xDEAD_BEEF_CAFE;

    for _step in 0..24 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let op = (state >> 63) & 1;
        let amount = MIN_DEPOSIT * (i128::from((state >> 32) as u32 % 50) + 1);

        if op == 0 {
            if amount > MAX_DEPOSIT || token.balance(&user) < amount {
                continue;
            }
            client.deposit(&user, &amount);
        } else {
            let balance = client.get_balance(&user);
            if balance <= MIN_DEPOSIT {
                continue;
            }
            let withdraw_amount = amount.min(balance / 2).max(MIN_DEPOSIT);
            client.withdraw(&user, &withdraw_amount, &None);
        }

        let user_shares = client.get_shares(&user);
        assert!(user_shares >= 0);
        assert!(client.get_balance(&user) >= 0);
    }
}
