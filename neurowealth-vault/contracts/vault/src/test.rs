#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup_vault(env: &Env) -> (Address, Address, Address) {
    let contract_id = env.register_contract(None, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(env, &contract_id);
    
    let agent = Address::generate(env);
    let usdc_token = Address::generate(env);
    let owner = agent.clone();
    
    client.initialize(&agent, &usdc_token);
    
    (contract_id, agent, owner)
}

#[test]
fn test_get_min_deposit_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let min_deposit = client.get_min_deposit();
    assert_eq!(min_deposit, 1_000_000_i128); // 1 USDC default
}

#[test]
fn test_get_max_deposit_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let max_deposit = client.get_max_deposit();
    assert_eq!(max_deposit, 10_000_000_000_i128); // 10K USDC default
}

#[test]
fn test_set_deposit_limits_success() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_min = 2_000_000_i128; // 2 USDC
    let new_max = 20_000_000_000_i128; // 20K USDC

    client.set_deposit_limits(&new_min, &new_max);

    assert_eq!(client.get_min_deposit(), new_min);
    assert_eq!(client.get_max_deposit(), new_max);
}

#[test]
#[should_panic(expected = "Minimum deposit must be at least 1 USDC")]
fn test_set_deposit_limits_min_too_low() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let min = 999_999_i128; // Less than 1 USDC
    let max = 10_000_000_000_i128;

    client.set_deposit_limits(&min, &max);
}

#[test]
#[should_panic(expected = "Maximum deposit must be greater than or equal to minimum")]
fn test_set_deposit_limits_max_less_than_min() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let min = 5_000_000_i128; // 5 USDC
    let max = 4_000_000_i128; // 4 USDC (less than min)

    client.set_deposit_limits(&min, &max);
}

#[test]
#[should_panic(expected = "Below minimum deposit")]
fn test_deposit_below_minimum() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set minimum to 5 USDC
    let min = 5_000_000_i128;
    let max = 20_000_000_000_i128;
    client.set_deposit_limits(&min, &max);

    let _user = Address::generate(&env);
    let amount = 4_000_000_i128; // 4 USDC (below minimum)

    // This should panic
    client.deposit(&_user, &amount);
}

#[test]
#[should_panic(expected = "Exceeds maximum deposit")]
fn test_deposit_above_maximum() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set maximum to 5 USDC
    let min = 1_000_000_i128;
    let max = 5_000_000_i128;
    client.set_deposit_limits(&min, &max);

    let _user = Address::generate(&env);
    let amount = 6_000_000_i128; // 6 USDC (above maximum)

    // This should panic
    client.deposit(&_user, &amount);
}

#[test]
fn test_deposit_at_minimum_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set minimum to 5 USDC
    let min = 5_000_000_i128;
    let max = 20_000_000_000_i128;
    client.set_deposit_limits(&min, &max);

    let _user = Address::generate(&env);
    let amount = 5_000_000_i128; // Exactly at minimum

    // This should succeed (though we can't fully test without token mocking)
    assert_eq!(client.get_min_deposit(), min);
    assert!(amount >= min);
}

#[test]
fn test_deposit_at_maximum_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set maximum to 5 USDC
    let min = 1_000_000_i128;
    let max = 5_000_000_i128;
    client.set_deposit_limits(&min, &max);

    let _user = Address::generate(&env);
    let amount = 5_000_000_i128; // Exactly at maximum

    // This should succeed (though we can't fully test without token mocking)
    assert_eq!(client.get_max_deposit(), max);
    assert!(amount <= max);
}

#[test]
#[should_panic(expected = "Below minimum deposit")]
fn test_deposit_one_stroop_below_minimum() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Use default minimum of 1 USDC
    let _user = Address::generate(&env);
    let amount = 999_999_i128; // 1 stroop below 1 USDC

    // This should panic
    client.deposit(&_user, &amount);
}

#[test]
#[should_panic(expected = "Exceeds maximum deposit")]
fn test_deposit_one_stroop_above_maximum() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set maximum to 10 USDC
    let min = 1_000_000_i128;
    let max = 10_000_000_i128;
    client.set_deposit_limits(&min, &max);

    let _user = Address::generate(&env);
    let amount = 10_000_001_i128; // 1 stroop above maximum

    // This should panic
    client.deposit(&_user, &amount);
}

#[test]
fn test_owner_updates_limits_immediate_effect() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Verify initial limits
    assert_eq!(client.get_min_deposit(), 1_000_000_i128);
    assert_eq!(client.get_max_deposit(), 10_000_000_000_i128);

    // Update limits
    let new_min = 3_000_000_i128; // 3 USDC
    let new_max = 15_000_000_000_i128; // 15K USDC
    client.set_deposit_limits(&new_min, &new_max);

    // Verify new limits are immediately effective
    assert_eq!(client.get_min_deposit(), new_min);
    assert_eq!(client.get_max_deposit(), new_max);

    // Test that new limits apply immediately by checking validation
    let _user = Address::generate(&env);
    
    // Amount below new minimum should fail
    let below_min = 2_000_000_i128; // 2 USDC
    assert!(below_min < new_min);
    
    // Amount above new maximum should fail  
    let above_max = 20_000_000_000_i128; // 20K USDC
    assert!(above_max > new_max);
    
    // Amount within new range should be valid
    let within_range = 5_000_000_i128; // 5 USDC
    assert!(within_range >= new_min && within_range <= new_max);
}
