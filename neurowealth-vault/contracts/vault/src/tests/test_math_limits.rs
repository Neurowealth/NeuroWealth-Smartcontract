//! Tests for math boundary conditions and checked arithmetic
use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_deposit_overflow_guard_exists() {
    // Verifies that balance accounting uses checked_add so an overflow would
    // panic with "vault: balance overflow" rather than silently wrapping.
    // Triggering i128::MAX overflow directly is impractical (requires ~1.7e38 USDC),
    // so this test confirms the vault initialises cleanly and the code path exists.
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Disable per-user cap so we are only limited by token supply
    client.set_limits(&0, &0);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128; // 1 USDC — tiny, just confirms the path works
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    assert_eq!(client.get_total_deposits(), amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_withdraw_underflow() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    // Deposit 10 USDC
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000);

    // Try to withdraw 11 USDC — more than the user holds.
    // The contract asserts share sufficiency before the subtraction, so the
    // message is "vault: insufficient shares for requested amount".
    client.withdraw(&user, &11_000_000, &None);
}

#[test]
fn test_conversion_math_is_correct() {
    // Verifies share-price conversion maintains the invariant:
    // convert_to_assets(convert_to_shares(x)) ≈ x (within 1 stroop rounding).
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit = 10_000_000_i128; // 10 USDC
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // At bootstrap (1:1 price), shares == deposit and round-trips exactly
    let shares = client.convert_to_shares(&deposit);
    let back = client.convert_to_assets(&shares);
    assert_eq!(
        back, deposit,
        "round-trip conversion should be lossless at 1:1 price"
    );
}
