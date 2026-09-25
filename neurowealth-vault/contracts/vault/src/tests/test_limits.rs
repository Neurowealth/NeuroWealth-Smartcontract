//! Tests for deposit limits and caps

use super::utils::*;
use crate::{DataKey, DEFAULT_MIN_DEPOSIT, MAX_DEPOSIT_CEILING};
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_owner_can_set_tvl_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let tvl_cap = 100_000_000_000_i128; // 100K USDC
    client.set_tvl_cap(&tvl_cap);

    assert_eq!(client.get_tvl_cap(), tvl_cap);
}

#[test]
fn test_owner_can_set_user_deposit_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user_cap = 50_000_000_000_i128; // 50K USDC
    client.set_user_deposit_cap(&user_cap);

    assert_eq!(client.get_user_deposit_cap(), user_cap);
}

#[test]
fn test_set_caps() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user_cap = 25_000_000_000_i128; // 25K USDC
    let tvl_cap = 150_000_000_000_i128; // 150K USDC

    client.set_caps(&user_cap, &tvl_cap);

    assert_eq!(client.get_user_deposit_cap(), user_cap);
    assert_eq!(client.get_tvl_cap(), tvl_cap);
}

#[test]
#[should_panic(expected = "Error(Contract, #24)")]
fn test_set_caps_negative_user_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_caps(&-1_i128, &100_000_000_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #23)")]
fn test_set_caps_negative_tvl_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_caps(&100_000_000_i128, &-1_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #25)")]
fn test_set_caps_tvl_cap_less_than_user_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // tvl_cap (10) < user_deposit_cap (20)
    client.set_caps(&20_000_000_i128, &10_000_000_i128);
}

#[test]
fn test_set_deposit_limits() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let min = 2_000_000_i128; // 2 USDC
    let max = 20_000_000_000_i128; // 20K USDC

    client.set_deposit_limits(&min, &max);

    assert_eq!(client.get_min_deposit(), min);
    assert_eq!(client.get_max_deposit(), max);
}

#[test]
#[should_panic(expected = "Error(Contract, #26)")]
fn test_set_deposit_limits_min_too_low() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let min = 999_999_i128; // Less than 1 USDC
    let max = 20_000_000_000_i128;

    client.set_deposit_limits(&min, &max);
}

#[test]
#[should_panic(expected = "Error(Contract, #27)")]
fn test_set_deposit_limits_max_less_than_min() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let min = 5_000_000_i128;
    let max = 4_000_000_i128; // Less than min

    client.set_deposit_limits(&min, &max);
}

// ============================================================================
// ISSUE #435 — UPPER BOUND ON set_deposit_limits' max
// ============================================================================

/// `max` set exactly to the ceiling is accepted (inclusive boundary).
#[test]
fn test_set_deposit_limits_max_at_ceiling_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_deposit_limits(&DEFAULT_MIN_DEPOSIT, &MAX_DEPOSIT_CEILING);

    assert_eq!(client.get_max_deposit(), MAX_DEPOSIT_CEILING);
}

/// One unit above the ceiling must be rejected, preventing a misconfigured
/// astronomically-high per-transaction maximum (e.g. an accidental `i128::MAX`).
#[test]
#[should_panic(expected = "Error(Contract, #66)")]
fn test_set_deposit_limits_max_above_ceiling_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_deposit_limits(&DEFAULT_MIN_DEPOSIT, &(MAX_DEPOSIT_CEILING + 1));
}

/// An accidental `i128::MAX` (the historical footgun this issue guards
/// against) must be rejected rather than silently disabling the per-tx cap.
#[test]
#[should_panic(expected = "Error(Contract, #66)")]
fn test_set_deposit_limits_rejects_i128_max() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_deposit_limits(&DEFAULT_MIN_DEPOSIT, &i128::MAX);
}

#[test]
#[should_panic(expected = "Error(Contract, #41)")]
fn test_deposit_enforces_tvl_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Set TVL cap to 10 USDC
    let tvl_cap = 10_000_000_i128;
    client.set_tvl_cap(&tvl_cap);

    let user = Address::generate(&env);
    let amount = 11_000_000_i128; // 11 USDC — exceeds TVL cap

    token_client.mint(&user, &amount);
    client.deposit(&user, &amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #40)")]
fn test_deposit_enforces_user_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Set user deposit cap to 5 USDC
    let user_cap = 5_000_000_i128;
    client.set_user_deposit_cap(&user_cap);

    let user = Address::generate(&env);
    let amount = 6_000_000_i128; // 6 USDC — exceeds user cap

    token_client.mint(&user, &amount);
    client.deposit(&user, &amount);
}

#[test]
fn test_tvl_cap_zero_means_unlimited() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // cap = 0 disables enforcement
    client.set_tvl_cap(&0_i128);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    assert_eq!(client.get_total_deposits(), amount);
}

#[test]
fn test_user_deposit_cap_zero_means_unlimited() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // cap = 0 disables enforcement
    client.set_user_deposit_cap(&0_i128);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    assert_eq!(client.get_shares(&user), amount);
}

#[test]
fn test_get_min_deposit_uses_default_when_key_missing() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.as_contract(&contract_id, || {
        env.storage().instance().remove(&DataKey::MinDeposit);
    });

    assert_eq!(client.get_min_deposit(), DEFAULT_MIN_DEPOSIT);
}

#[test]
#[should_panic(expected = "Error(Contract, #38)")]
fn test_deposit_uses_default_minimum_when_key_missing() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    env.as_contract(&contract_id, || {
        env.storage().instance().remove(&DataKey::MinDeposit);
    });

    let user = Address::generate(&env);
    let below_default_min = DEFAULT_MIN_DEPOSIT - 1;
    token_client.mint(&user, &below_default_min);

    client.deposit(&user, &below_default_min);
}

// ============================================================================
// ISSUE #119 — REJECT NEGATIVE VALUES IN TVL AND PER-USER CAP SETTERS
// ============================================================================

// ---- set_tvl_cap ------------------------------------------------------------

#[test]
#[should_panic(expected = "Error(Contract, #23)")]
fn test_set_tvl_cap_rejects_negative() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_tvl_cap(&-1_i128);
}

/// Zero is a valid TVL cap meaning "no cap enforced".
#[test]
fn test_set_tvl_cap_accepts_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_tvl_cap(&0_i128);
    assert_eq!(client.get_tvl_cap(), 0);
}

// ---- set_user_deposit_cap ---------------------------------------------------

#[test]
#[should_panic(expected = "Error(Contract, #24)")]
fn test_set_user_deposit_cap_rejects_negative() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_user_deposit_cap(&-1_i128);
}

/// Zero is a valid per-user cap meaning "no cap enforced".
#[test]
fn test_set_user_deposit_cap_accepts_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_user_deposit_cap(&0_i128);
    assert_eq!(client.get_user_deposit_cap(), 0);
}

// ============================================================================
// DEPOSIT CAP — ASSETS-BASED SEMANTICS (includes accrued yield)
// ============================================================================

/// After yield pushes the user's asset value to the cap, any further deposit
/// must be rejected even though the user's principal is still below the cap.
#[test]
#[should_panic(expected = "Error(Contract, #40)")]
fn test_deposit_cap_blocks_deposit_when_yield_fills_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Cap = 10 USDC; user deposits 8 USDC (2 USDC headroom by principal).
    let cap = 10_000_000_i128;
    let deposit = 8_000_000_i128;
    client.set_user_deposit_cap(&cap);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Accrue 2 USDC yield — user's asset value is now exactly 10 USDC (= cap).
    let yield_amount = 2_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit + yield_amount), &false, &0);

    assert_eq!(client.get_balance(&user), cap);

    // Any further deposit must be rejected (assets + amount > cap).
    let extra = 1_000_000_i128;
    token_client.mint(&user, &extra);
    client.deposit(&user, &extra);
}

/// Yield accrual alone (without a deposit) does not block a user whose asset
/// value still has room under the cap.
#[test]
fn test_deposit_cap_allows_deposit_when_assets_still_under_cap_after_yield() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Cap = 12 USDC; user deposits 8 USDC.
    let cap = 12_000_000_i128;
    let deposit = 8_000_000_i128;
    client.set_user_deposit_cap(&cap);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Accrue 2 USDC yield — user's asset value is now 10 USDC (2 USDC headroom).
    let yield_amount = 2_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit + yield_amount), &false, &0);

    assert_eq!(client.get_balance(&user), deposit + yield_amount);

    // A 2 USDC deposit fits exactly: assets(10) + amount(2) = cap(12).
    let top_up = 2_000_000_i128;
    token_client.mint(&user, &top_up);
    client.deposit(&user, &top_up);

    assert!(client.get_balance(&user) >= cap - 1_000_i128);
}

/// Yield that only partially closes the gap still blocks a deposit that would
/// push total asset value over the cap.
#[test]
#[should_panic(expected = "Error(Contract, #40)")]
fn test_deposit_cap_blocks_when_yield_partially_fills_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Cap = 10 USDC; user deposits 8 USDC.
    let cap = 10_000_000_i128;
    let deposit = 8_000_000_i128;
    client.set_user_deposit_cap(&cap);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Accrue 1 USDC yield — user's asset value is now 9 USDC (1 USDC headroom).
    let yield_amount = 1_000_000_i128;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &(deposit + yield_amount), &false, &0);

    // Attempting to deposit 2 USDC would put assets at 11 > cap(10) — must fail.
    let over_limit = 2_000_000_i128;
    token_client.mint(&user, &over_limit);
    client.deposit(&user, &over_limit);
}

// ============================================================================
// ISSUE #547 — LOWERING USER DEPOSIT CAP BLOCKS NEW DEPOSITS
// ============================================================================

/// Verifies that when the owner lowers the per-user deposit cap below a user's
/// current deposit, the existing deposit is not affected, but new deposits from
/// that user are blocked until withdrawals reduce their balance below the cap.
#[test]
fn test_lowering_user_deposit_cap_below_current_deposit_blocks_new_deposits() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // 1. Set user deposit cap to 1000 USDC (1,000,000,000 stroops / units)
    let cap_1000 = 1_000_000_000_i128;
    client.set_user_deposit_cap(&cap_1000);
    assert_eq!(client.get_user_deposit_cap(), cap_1000);

    // 2. User A deposits 800 USDC (succeeds)
    let user_a = Address::generate(&env);
    let deposit_800 = 800_000_000_i128;
    token_client.mint(&user_a, &deposit_800);
    client.deposit(&user_a, &deposit_800);
    assert_eq!(client.get_balance(&user_a), deposit_800);

    // 3. Owner lowers cap to 500 USDC (should succeed)
    let cap_500 = 500_000_000_i128;
    client.set_user_deposit_cap(&cap_500);
    assert_eq!(client.get_user_deposit_cap(), cap_500);

    // 4. User A tries to deposit another 100 USDC (should fail because 800 + 100 > 500)
    let deposit_100 = 100_000_000_i128;
    token_client.mint(&user_a, &deposit_100);
    let result = client.try_deposit(&user_a, &deposit_100);
    assert_eq!(
        result,
        Err(Ok(soroban_sdk::Error::from_contract_error(40))),
        "New deposit exceeding lowered cap must be blocked with VaultError::ExceedsUserDepositCap (#40)"
    );

    // 5. Assert User A's existing 800 USDC remains untouched
    assert_eq!(client.get_balance(&user_a), deposit_800);

    // 6. User A withdraws 400 USDC (succeeds)
    let withdraw_400 = 400_000_000_i128;
    client.withdraw(&user_a, &withdraw_400, &None);
    assert_eq!(client.get_balance(&user_a), deposit_800 - withdraw_400);

    // 7. User A deposits 100 USDC (succeeds since 400 + 100 <= 500)
    client.deposit(&user_a, &deposit_100);
    assert_eq!(
        client.get_balance(&user_a),
        deposit_800 - withdraw_400 + deposit_100
    );

    // 8. Multi-user isolation check: User B deposits 300 USDC (succeeds since 300 <= 500)
    let user_b = Address::generate(&env);
    let deposit_300 = 300_000_000_i128;
    token_client.mint(&user_b, &deposit_300);
    client.deposit(&user_b, &deposit_300);
    assert_eq!(client.get_balance(&user_b), deposit_300);

    // User B tries to deposit another 300 USDC (300 + 300 = 600 > 500 cap) -> Fails with ExceedsUserDepositCap
    token_client.mint(&user_b, &deposit_300);
    let result_b = client.try_deposit(&user_b, &deposit_300);
    assert_eq!(
        result_b,
        Err(Ok(soroban_sdk::Error::from_contract_error(40))),
        "User B deposit exceeding lowered cap must be blocked with VaultError::ExceedsUserDepositCap (#40)"
    );
}
