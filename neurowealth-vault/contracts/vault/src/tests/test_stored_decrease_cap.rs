//! Tests for owner-configured decrease cap (#974).
//!
//! The decrease cap is now stored in owner-configured storage (DataKey::MaxDecreaseBps)
//! with a default of 1000 bps (10%) and a hard ceiling of 10_000 bps (100%).
//! The caller-supplied argument in update_total_assets is ignored.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// get_max_decrease_bps returns default 1000 bps (10%) when not explicitly set.
#[test]
fn test_get_max_decrease_bps_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit to initialize the vault
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Default should be 1000 bps (10%)
    assert_eq!(client.get_max_decrease_bps(), 1000);
}

/// set_max_decrease_bps allows owner to configure the cap.
#[test]
fn test_set_max_decrease_bps() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit to initialize the vault
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Set to 500 bps (5%)
    client.set_max_decrease_bps(&500);
    assert_eq!(client.get_max_decrease_bps(), 500);

    // Set to 2000 bps (20%)
    client.set_max_decrease_bps(&2000);
    assert_eq!(client.get_max_decrease_bps(), 2000);
}

/// set_max_decrease_bps clamps values above 10_000 to the hard ceiling.
#[test]
fn test_set_max_decrease_bps_hard_ceiling() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit to initialize the vault
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Set to 10_000 bps (100%) - should succeed
    client.set_max_decrease_bps(&10_000);
    assert_eq!(client.get_max_decrease_bps(), 10_000);

    // Set to 20_000 bps - should be clamped to 10_000
    client.set_max_decrease_bps(&20_000);
    assert_eq!(client.get_max_decrease_bps(), 10_000);
}

/// update_total_assets uses stored cap, ignores caller-supplied argument.
#[test]
fn test_update_total_assets_uses_stored_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Set cap to 500 bps (5%)
    client.set_max_decrease_bps(&500);

    // Attempt to report a 10% decrease with caller-supplied max_decrease_bps=10_000
    // Should fail because stored cap is 500 bps
    let new_total = 9_000_000_i128; // 10% decrease
    client.update_total_assets(&agent, &new_total, &true, &10_000);
}

/// update_total_assets rejects decrease exceeding stored cap even with owner co-sign.
#[test]
#[should_panic(expected = "Error(Contract, #32)")]
fn test_update_total_assets_rejects_excess_decrease_with_stored_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Set cap to 500 bps (5%)
    client.set_max_decrease_bps(&500);

    // Attempt to report a 10% decrease - should fail
    let new_total = 9_000_000_i128; // 10% decrease exceeds 5% cap
    client.update_total_assets(&agent, &new_total, &true, &0);
}

/// update_total_assets allows decrease within stored cap.
#[test]
fn test_update_total_assets_allows_decrease_within_stored_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    // Set cap to 500 bps (5%)
    client.set_max_decrease_bps(&500);

    // Report a 5% decrease - should succeed
    let new_total = 9_500_000_i128; // 5% decrease
    client.update_total_assets(&agent, &new_total, &true, &0);
    assert_eq!(client.get_total_assets(), new_total);
}

/// set_max_decrease_bps requires owner authorization.
#[test]
fn test_set_max_decrease_bps_requires_owner() {
    let env = Env::default();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // Make a deposit to initialize the vault
    let user = Address::generate(&env);
    let deposit = 10_000_000_i128;
    token.mint(&user, &deposit);
    client.deposit(&user, &deposit);

    env.mock_auths(&[]);
    let result = client.try_set_max_decrease_bps(&500);
    assert!(
        result.is_err(),
        "set_max_decrease_bps must fail without the owner's authorization"
    );
}
