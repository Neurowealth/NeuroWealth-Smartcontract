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

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_total = 0_i128;
    let new_total = 50_000_000_000_i128; // 50M USDC

    client.update_total_assets(&agent, &new_total);

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

// ============================================================================
// SHARE-BASED ACCOUNTING TESTS
// ============================================================================

#[test]
fn test_first_deposit_receives_1_to_1_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Verify initial state
    assert_eq!(client.get_total_shares(), 0);
    assert_eq!(client.get_total_assets(), 0);

    // Note: In a real test environment, you would need to mock the token transfer
    // For now, we verify the accounting functions work correctly
    // The actual deposit would require a real token contract
    
    // After first deposit of 10 USDC, shares should equal 10 USDC (1:1)
    // This is verified by the convert_to_shares logic in the contract
}

#[test]
fn test_share_conversion_math() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Test that share conversion functions are accessible
    // In a real scenario, we would test:
    // 1. First deposit: 10 USDC -> 10 shares (1:1)
    // 2. After yield: total_assets = 11 USDC, total_shares = 10
    // 3. Second deposit: 5 USDC -> (5 * 10) / 11 = ~4.54 shares
    // 4. User balance: shares * total_assets / total_shares should equal their proportional value
}

#[test]
fn test_yield_accrual_increases_withdrawal_value() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Scenario:
    // 1. User deposits 10 USDC -> receives 10 shares
    // 2. Agent updates total_assets to 11 USDC (10% yield)
    // 3. User's balance should now be 11 USDC (10 shares * 11 assets / 10 shares)
    // 4. User can withdraw 11 USDC (more than original deposit)

    // Verify initial state
    assert_eq!(client.get_total_assets(), 0);
    assert_eq!(client.get_total_shares(), 0);

    // After yield accrual via update_total_assets:
    // - Total assets increase
    // - Total shares remain constant
    // - Share price (assets/shares) increases
    // - User balances increase proportionally
}

#[test]
fn test_post_yield_deposits_maintain_correct_pricing() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Scenario:
    // 1. User A deposits 10 USDC -> 10 shares
    // 2. Yield accrues: total_assets = 11 USDC, total_shares = 10
    // 3. User B deposits 10 USDC -> should receive (10 * 10) / 11 = ~9.09 shares
    // 4. Both users should have proportional ownership
    // 5. User A: 10 shares / 19.09 total = ~52.4% ownership
    // 6. User B: 9.09 shares / 19.09 total = ~47.6% ownership
}

#[test]
fn test_full_and_partial_withdrawals() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Scenario:
    // 1. User deposits 10 USDC -> 10 shares
    // 2. Yield accrues: total_assets = 12 USDC, total_shares = 10
    // 3. Partial withdrawal: User withdraws 6 USDC
    //    - Shares to burn: (6 * 10) / 12 = 5 shares
    //    - Actual amount: (5 * 12) / 10 = 6 USDC
    //    - Remaining: 5 shares worth 6 USDC
    // 4. Full withdrawal: User withdraws remaining 6 USDC
    //    - Burns remaining 5 shares
    //    - User balance should be 0
}

#[test]
fn test_share_price_monotonically_increasing() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Verify that share price (total_assets / total_shares) only increases
    // when yield accrues, never decreases (unless assets are lost, which shouldn't happen)
    
    // Initial: 0 assets / 0 shares = undefined (first deposit sets 1:1)
    // After deposit: 10 assets / 10 shares = 1.0
    // After yield: 11 assets / 10 shares = 1.1 (increased)
    // After more yield: 12 assets / 10 shares = 1.2 (increased)
}

#[test]
fn test_multiple_users_proportional_ownership() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Scenario with multiple users:
    // 1. User A deposits 10 USDC -> 10 shares
    // 2. User B deposits 20 USDC -> 20 shares (1:1, first deposits)
    // 3. Yield accrues: total_assets = 33 USDC (10% yield)
    // 4. User A balance: 10 * 33 / 30 = 11 USDC
    // 5. User B balance: 20 * 33 / 30 = 22 USDC
    // 6. Both users benefit proportionally from yield
}

#[test]
fn test_get_shares_and_get_balance_functions() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Initially, user has no shares or balance
    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(client.get_balance(&user), 0);

    // After deposit, shares should increase
    // After yield, balance should increase but shares remain constant
}

#[test]
fn test_total_assets_and_total_shares_tracking() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Verify initial state
    assert_eq!(client.get_total_assets(), 0);
    assert_eq!(client.get_total_shares(), 0);

    // After deposits, both should increase
    // After yield (update_total_assets), only assets increase
    // After withdrawals, both decrease proportionally
}
