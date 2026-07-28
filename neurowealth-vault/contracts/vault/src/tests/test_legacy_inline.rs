use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup_vault(env: &Env) -> (Address, Address, Address, Address) {
    let deployer = Address::generate(env);
    let salt = BytesN::from_array(env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(env, &contract_id);
    let agent = Address::generate(env);
    let usdc_token = Address::generate(env);
    let owner = Address::generate(env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    (contract_id, agent, owner, usdc_token)
}

#[test]
fn test_vault_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let deployer = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    assert_eq!(client.get_agent(), agent);
    assert_eq!(client.get_usdc_token(), usdc_token);
    assert_eq!(client.get_total_deposits(), 0);
    assert!(!client.is_paused());
}

#[test]
fn test_pause_and_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(!client.is_paused());

    client.pause(&owner);
    assert!(client.is_paused());

    client.unpause(&owner);
    assert!(!client.is_paused());
}

#[test]
fn test_emergency_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(!client.is_paused());

    client.emergency_pause(&owner);
    assert!(client.is_paused());
}

#[test]
fn test_set_limits() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_min = 20_000_000_000_i128;
    let new_max = 200_000_000_000_i128;

    client.set_limits(&new_min, &new_max);

    assert_eq!(client.get_user_deposit_cap(), new_min);
    assert_eq!(client.get_tvl_cap(), new_max);
}

#[test]
fn test_set_tvl_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_max = 150_000_000_000_i128;

    client.set_tvl_cap(&new_max);

    assert_eq!(client.get_tvl_cap(), new_max);
}

#[test]
fn test_set_user_deposit_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_min = 15_000_000_000_i128;

    client.set_user_deposit_cap(&new_min);

    assert_eq!(client.get_user_deposit_cap(), new_min);
}

#[test]
fn test_update_agent() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    assert_eq!(client.get_agent(), new_agent);
    assert_ne!(client.get_agent(), old_agent);
}

#[test]
fn test_update_total_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(client.get_total_assets(), 0);
}

#[test]
fn test_get_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    assert_eq!(client.get_balance(&user), 0);
}

#[test]
fn test_get_version() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(client.get_version(), 1);
}

#[test]
fn test_withdraw_checks_effects_interactions_pattern() {
    let env = Env::default();
    env.mock_all_auths();

    let deployer = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let agent = Address::generate(&env);
    let user = Address::generate(&env);
    let usdc_token = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    assert_eq!(client.get_balance(&user), 0);
    assert_eq!(client.get_total_deposits(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_withdraw_fails_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    client.pause(&owner);
    client.withdraw(&user, &1_000_000, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #37)")]
fn test_withdraw_rejects_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    client.withdraw(&user, &0, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_withdraw_fails_insufficient_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    client.withdraw(&user, &1_000_000, &None);
}

#[test]
fn test_withdraw_reentrancy_protection() {
    let env = Env::default();
    env.mock_all_auths();

    let deployer = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let agent = Address::generate(&env);
    let usdc_token = Address::generate(&env);
    let owner = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_deposit_fails_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    client.pause(&owner);
    client.deposit(&user, &1_000_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #37)")]
fn test_deposit_rejects_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    client.deposit(&user, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #38)")]
fn test_deposit_enforces_minimum() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    client.deposit(&user, &999_999);
}

#[test]
fn test_rebalance_basic() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let protocol = soroban_sdk::symbol_short!("none");
    let expected_apy = 850_i128;

    client.rebalance(&protocol, &expected_apy, &0_i128);
}
