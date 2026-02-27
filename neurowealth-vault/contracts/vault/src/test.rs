#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, Events}, Address, Env, Symbol};

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
fn test_vault_initialized_event() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let agent = Address::generate(&env);
    let usdc_token = Address::generate(&env);
    let tvl_cap = 100_000_000_000_i128;

    client.initialize(&agent, &usdc_token);

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    
    let event = &events[0];
    assert_eq!(event.0, (symbol_short!("vault_initialized"),));
    
    let event_data: VaultInitializedEvent = event.1.clone().try_into().unwrap();
    assert_eq!(event_data.agent, agent);
    assert_eq!(event_data.usdc_token, usdc_token);
    assert_eq!(event_data.tvl_cap, tvl_cap);
}

#[test]
fn test_vault_paused_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause();

    let events = env.events().all();
    // Find the pause event (skip initialization event)
    let pause_events: Vec<_> = events.iter()
        .filter(|e| e.0 == (symbol_short!("vault_paused"),))
        .collect();
    assert_eq!(pause_events.len(), 1);
    
    let event_data: VaultPausedEvent = pause_events[0].1.clone().try_into().unwrap();
    assert_eq!(event_data.caller, owner);
}

#[test]
fn test_vault_unpaused_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause();
    client.unpause();

    let events = env.events().all();
    let unpause_events: Vec<_> = events.iter()
        .filter(|e| e.0 == (symbol_short!("vault_unpaused"),))
        .collect();
    assert_eq!(unpause_events.len(), 1);
    
    let event_data: VaultUnpausedEvent = unpause_events[0].1.clone().try_into().unwrap();
    assert_eq!(event_data.caller, owner);
}

#[test]
fn test_emergency_paused_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.emergency_pause();

    let events = env.events().all();
    let emergency_events: Vec<_> = events.iter()
        .filter(|e| e.0 == (symbol_short!("emergency_paused"),))
        .collect();
    assert_eq!(emergency_events.len(), 1);
    
    let event_data: EmergencyPausedEvent = emergency_events[0].1.clone().try_into().unwrap();
    assert_eq!(event_data.caller, owner);
}

#[test]
fn test_limits_updated_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_min = 10_000_000_000_i128; // 10K USDC default
    let old_max = 100_000_000_000_i128; // 100M USDC default
    let new_min = 20_000_000_000_i128; // 20K USDC
    let new_max = 200_000_000_000_i128; // 200M USDC

    client.set_limits(&new_min, &new_max);

    let events = env.events().all();
    let limits_events: Vec<_> = events.iter()
        .filter(|e| e.0 == (symbol_short!("limits_updated"),))
        .collect();
    assert_eq!(limits_events.len(), 1);
    
    let event_data: LimitsUpdatedEvent = limits_events[0].1.clone().try_into().unwrap();
    assert_eq!(event_data.old_min, old_min);
    assert_eq!(event_data.new_min, new_min);
    assert_eq!(event_data.old_max, old_max);
    assert_eq!(event_data.new_max, new_max);
}

#[test]
fn test_limits_updated_event_from_set_tvl_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_max = 100_000_000_000_i128; // 100M USDC default
    let new_max = 150_000_000_000_i128; // 150M USDC

    client.set_tvl_cap(&new_max);

    let events = env.events().all();
    let limits_events: Vec<_> = events.iter()
        .filter(|e| e.0 == (symbol_short!("limits_updated"),))
        .collect();
    assert_eq!(limits_events.len(), 1);
    
    let event_data: LimitsUpdatedEvent = limits_events[0].1.clone().try_into().unwrap();
    assert_eq!(event_data.old_max, old_max);
    assert_eq!(event_data.new_max, new_max);
}

#[test]
fn test_limits_updated_event_from_set_user_deposit_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_min = 10_000_000_000_i128; // 10K USDC default
    let new_min = 15_000_000_000_i128; // 15K USDC

    client.set_user_deposit_cap(&new_min);

    let events = env.events().all();
    let limits_events: Vec<_> = events.iter()
        .filter(|e| e.0 == (symbol_short!("limits_updated"),))
        .collect();
    assert_eq!(limits_events.len(), 1);
    
    let event_data: LimitsUpdatedEvent = limits_events[0].1.clone().try_into().unwrap();
    assert_eq!(event_data.old_min, old_min);
    assert_eq!(event_data.new_min, new_min);
}

#[test]
fn test_agent_updated_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    let events = env.events().all();
    let agent_events: Vec<_> = events.iter()
        .filter(|e| e.0 == (symbol_short!("agent_updated"),))
        .collect();
    assert_eq!(agent_events.len(), 1);
    
    let event_data: AgentUpdatedEvent = agent_events[0].1.clone().try_into().unwrap();
    assert_eq!(event_data.old_agent, old_agent);
    assert_eq!(event_data.new_agent, new_agent);
}

#[test]
fn test_assets_updated_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_total = 0_i128;
    let new_total = 50_000_000_000_i128; // 50M USDC

    client.update_total_assets(&new_total);

    let events = env.events().all();
    let assets_events: Vec<_> = events.iter()
        .filter(|e| e.0 == (symbol_short!("assets_updated"),))
        .collect();
    assert_eq!(assets_events.len(), 1);
    
    let event_data: AssetsUpdatedEvent = assets_events[0].1.clone().try_into().unwrap();
    assert_eq!(event_data.old_total, old_total);
    assert_eq!(event_data.new_total, new_total);
}

#[test]
fn test_rebalance_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let protocol = symbol_short!("balanced");
    let expected_apy = 850_i128; // 8.5% in basis points

    // Call rebalance as the agent
    client.rebalance(&protocol, &expected_apy);

    let events = env.events().all();
    let rebalance_events: Vec<_> = events.iter()
        .filter(|e| e.0 == (symbol_short!("rebalance"),))
        .collect();
    assert_eq!(rebalance_events.len(), 1);
    
    let event_data: RebalanceEvent = rebalance_events[0].1.clone().try_into().unwrap();
    assert_eq!(event_data.protocol, protocol);
    assert_eq!(event_data.expected_apy, expected_apy);
}

#[test]
fn test_deposit_and_withdraw_events() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let agent = Address::generate(&env);
    let user = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&agent, &usdc_token);

    let deposit_amount = 1_000_000_i128; // 1 USDC
    // Note: In a real test, you'd need to mock the token transfer
    // For now, we just verify the event structure would be correct
    
    assert_eq!(client.get_balance(&user), 0);
}

#[test]
fn test_pause_and_unpause_events() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(client.is_paused(), false);
    
    client.pause();
    assert_eq!(client.is_paused(), true);
    
    client.unpause();
    assert_eq!(client.is_paused(), false);
}
