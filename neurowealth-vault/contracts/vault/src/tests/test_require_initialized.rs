//! Tests that public functions panic with `NotInitialized` before init.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Symbol};

fn setup_uninitialized(env: &Env) -> (Address, NeuroWealthVaultClient<'_>) {
    let deployer = Address::generate(env);
    let salt = BytesN::from_array(env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(env, &contract_id);
    (contract_id, client)
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_deposit_before_init_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client) = setup_uninitialized(&env);
    let user = Address::generate(&env);
    client.deposit(&user, &1_000_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_withdraw_before_init_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client) = setup_uninitialized(&env);
    let user = Address::generate(&env);
    client.withdraw(&user, &1_000_000, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_withdraw_all_before_init_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client) = setup_uninitialized(&env);
    let user = Address::generate(&env);
    client.withdraw_all(&user, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_rebalance_before_init_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client) = setup_uninitialized(&env);
    client.rebalance(&Symbol::new(&env, "none"), &0, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_get_balance_before_init_panics() {
    let env = Env::default();
    let (_contract_id, client) = setup_uninitialized(&env);
    let user = Address::generate(&env);
    client.get_balance(&user);
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_get_total_deposits_before_init_panics() {
    let env = Env::default();
    let (_contract_id, client) = setup_uninitialized(&env);
    client.get_total_deposits();
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_get_total_assets_before_init_panics() {
    let env = Env::default();
    let (_contract_id, client) = setup_uninitialized(&env);
    client.get_total_assets();
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_get_shares_before_init_panics() {
    let env = Env::default();
    let (_contract_id, client) = setup_uninitialized(&env);
    let user = Address::generate(&env);
    client.get_shares(&user);
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_get_exchange_rate_before_init_panics() {
    let env = Env::default();
    let (_contract_id, client) = setup_uninitialized(&env);
    client.get_exchange_rate();
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_pause_before_init_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client) = setup_uninitialized(&env);
    let owner = Address::generate(&env);
    client.pause(&owner);
}
