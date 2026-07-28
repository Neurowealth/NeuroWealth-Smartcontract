//! Tests verifying that each contract operation emits the expected event with correct payload values

use super::utils::*;
use crate::{
    AgentUpdatedEvent, AssetsUpdatedEvent, BlendPoolConfiguredEvent, BlendSupplyEvent,
    BlendWithdrawEvent, CapsUpdatedEvent, DepositEvent, DepositLimitsUpdatedEvent,
    EmergencyPausedEvent, RebalanceEvent, TvlCapUpdatedEvent, UserDepositCapUpdatedEvent,
    VaultInitializedEvent, VaultPausedEvent, VaultUnpausedEvent, WithdrawEvent,
    TOPIC_AGENT_UPDATED, TOPIC_ASSETS_UPDATED, TOPIC_BLEND_POOL_CONFIGURED, TOPIC_BLEND_SUPPLY,
    TOPIC_BLEND_WITHDRAW, TOPIC_CAPS_UPDATED, TOPIC_DEPOSIT, TOPIC_DEPOSIT_LIMITS_UPDATED,
    TOPIC_EMERGENCY_PAUSED, TOPIC_INIT, TOPIC_PAUSED, TOPIC_REBALANCE, TOPIC_TVL_CAP_UPDATED,
    TOPIC_UNPAUSED, TOPIC_USER_CAP_UPDATED, TOPIC_WITHDRAW,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, BytesN, Env, TryFromVal};

#[test]
fn test_initialize_emits_init_event_with_correct_payload() {
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
    let expected_tvl_cap = 100_000_000_000_i128;
    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    let init_events = find_events_by_topic(env.events().all(), &env, TOPIC_INIT);
    assert_eq!(
        init_events.len(),
        1,
        "Exactly one init event should be emitted"
    );

    let (_, _, data) = &init_events[0];
    let event =
        VaultInitializedEvent::try_from_val(&env, data).expect("Should be a VaultInitializedEvent");

    assert_eq!(
        event.agent, agent,
        "Event agent should match initialized agent"
    );
    assert_eq!(
        event.usdc_token, usdc_token,
        "Event usdc_token should match initialized token"
    );
    assert_eq!(
        event.tvl_cap, expected_tvl_cap,
        "Event tvl_cap should match default cap"
    );
}

#[test]
fn test_set_blend_pool_emits_configured_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, old_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let new_pool = env.register_contract(None, MockBlendPool);

    client.set_blend_pool(&owner, &old_pool);
    client.set_blend_pool(&owner, &new_pool);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_BLEND_POOL_CONFIGURED);
    assert_eq!(
        events.len(),
        2,
        "Each set_blend_pool call should emit a BlendPoolConfiguredEvent"
    );

    let (_, _, data) = &events[1];
    let event = BlendPoolConfiguredEvent::try_from_val(&env, data)
        .expect("Should be a BlendPoolConfiguredEvent");

    assert_eq!(
        event.old_pool,
        Some(old_pool),
        "Event old_pool should match the previously configured pool"
    );
    assert_eq!(
        event.new_pool, new_pool,
        "Event new_pool should match the replacement pool"
    );
    assert_eq!(event.owner, owner, "Event owner should match the caller");
}

#[test]
fn test_deposit_emits_deposit_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let deposit_events = find_events_by_topic(env.events().all(), &env, TOPIC_DEPOSIT);
    assert!(!deposit_events.is_empty(), "Deposit should emit an event");

    let (_, _, data) = &deposit_events[0];
    let event = DepositEvent::try_from_val(&env, data).expect("Should be a DepositEvent");

    assert_eq!(event.user, user, "Event user should match depositor");
    assert_eq!(
        event.amount, deposit_amount,
        "Event amount should match deposited amount"
    );
    assert_eq!(
        event.shares, deposit_amount,
        "First deposit should mint 1:1 shares"
    );
}

#[test]
fn test_deposit_multiple_users_events_correct() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let amount1 = 10_000_000_i128;
    let amount2 = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user1, amount1);
    mint_and_deposit(&env, &client, &usdc_token, &user2, amount2);

    let deposit_events = find_events_by_topic(env.events().all(), &env, TOPIC_DEPOSIT);
    assert_eq!(deposit_events.len(), 2, "Should emit two deposit events");

    let (_, _, data1) = &deposit_events[0];
    let event1 = DepositEvent::try_from_val(&env, data1).expect("Should be a DepositEvent");
    assert_eq!(event1.user, user1);
    assert_eq!(event1.amount, amount1);
    assert_eq!(event1.shares, amount1);

    let (_, _, data2) = &deposit_events[1];
    let event2 = DepositEvent::try_from_val(&env, data2).expect("Should be a DepositEvent");
    assert_eq!(event2.user, user2);
    assert_eq!(event2.amount, amount2);
    assert_eq!(event2.shares, amount2);
}

#[test]
fn test_withdraw_emits_withdraw_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let withdraw_amount = 3_000_000_i128;
    client.withdraw(&user, &withdraw_amount, &None);

    let withdraw_events = find_events_by_topic(env.events().all(), &env, TOPIC_WITHDRAW);
    assert!(!withdraw_events.is_empty(), "Withdraw should emit an event");

    let (_, _, data) = &withdraw_events[0];
    let event = WithdrawEvent::try_from_val(&env, data).expect("Should be a WithdrawEvent");

    assert_eq!(event.user, user, "Event user should match withdrawer");
    assert_eq!(
        event.amount, withdraw_amount,
        "Event amount should match withdrawn amount"
    );
    assert_eq!(
        event.shares, withdraw_amount,
        "At 1:1 price, shares burned should equal amount"
    );
}

#[test]
fn test_withdraw_all_emits_withdraw_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let withdrawn = client.withdraw_all(&user, &None);

    assert_eq!(withdrawn, deposit_amount, "Should withdraw full balance");

    let withdraw_events = find_events_by_topic(env.events().all(), &env, TOPIC_WITHDRAW);
    let last_event_data = &withdraw_events.last().unwrap().2;
    let event =
        WithdrawEvent::try_from_val(&env, last_event_data).expect("Should be a WithdrawEvent");

    assert_eq!(event.user, user, "Event user should match withdrawer");
    assert_eq!(
        event.amount, deposit_amount,
        "Event amount should match full balance"
    );
    assert_eq!(event.shares, deposit_amount, "Should burn all shares");
}

#[test]
fn test_pause_emits_paused_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    let pause_events = find_events_by_topic(env.events().all(), &env, TOPIC_PAUSED);
    assert!(!pause_events.is_empty(), "Pause should emit an event");

    let (_, _, data) = &pause_events[0];
    let event = VaultPausedEvent::try_from_val(&env, data).expect("Should be a VaultPausedEvent");
    assert_eq!(event.owner, owner, "Event owner should match pauser");
}

#[test]
fn test_unpause_emits_unpaused_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    client.unpause(&owner);

    let unpause_events = find_events_by_topic(env.events().all(), &env, TOPIC_UNPAUSED);
    assert!(!unpause_events.is_empty(), "Unpause should emit an event");

    let (_, _, data) = &unpause_events[0];
    let event =
        VaultUnpausedEvent::try_from_val(&env, data).expect("Should be a VaultUnpausedEvent");
    assert_eq!(event.owner, owner, "Event owner should match unpauser");
}

#[test]
fn test_emergency_pause_emits_emergency_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.emergency_pause(&owner);

    let emergency_events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED);
    assert!(
        !emergency_events.is_empty(),
        "Emergency pause should emit an event"
    );

    let (_, _, data) = &emergency_events[0];
    let event =
        EmergencyPausedEvent::try_from_val(&env, data).expect("Should be an EmergencyPausedEvent");
    assert_eq!(
        event.owner, owner,
        "Event owner should match emergency pauser"
    );
}

#[test]
fn test_set_deposit_limits_emits_limits_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_min = 2_000_000_i128;
    let new_max = 20_000_000_000_i128;
    client.set_deposit_limits(&new_min, &new_max);

    let limits_events =
        find_events_by_topic(env.events().all(), &env, TOPIC_DEPOSIT_LIMITS_UPDATED);
    assert!(
        !limits_events.is_empty(),
        "set_deposit_limits should emit a DepositLimitsUpdatedEvent"
    );

    let (_, _, data) = &limits_events[0];
    let event = DepositLimitsUpdatedEvent::try_from_val(&env, data)
        .expect("Should be a DepositLimitsUpdatedEvent");
    assert_eq!(
        event.new_min, new_min,
        "Event new_min should match set value"
    );
    assert_eq!(
        event.new_max, new_max,
        "Event new_max should match set value"
    );
}

#[test]
fn test_set_tvl_cap_emits_tvl_cap_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_tvl_cap = 100_000_000_000_i128; // Default from initialize
    let new_tvl_cap = 200_000_000_000_i128;
    client.set_tvl_cap(&new_tvl_cap);

    let tvl_events = find_events_by_topic(env.events().all(), &env, TOPIC_TVL_CAP_UPDATED);
    assert!(!tvl_events.is_empty(), "set_tvl_cap should emit an event");

    let (_, _, data) = &tvl_events[0];
    let event =
        TvlCapUpdatedEvent::try_from_val(&env, data).expect("Should be a TvlCapUpdatedEvent");
    assert_eq!(event.old_cap, old_tvl_cap);
    assert_eq!(event.new_cap, new_tvl_cap);
}

#[test]
fn test_set_user_deposit_cap_emits_user_cap_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_user_cap = 10_000_000_000_i128; // Default from initialize
    let new_user_cap = 20_000_000_000_i128;
    client.set_user_deposit_cap(&new_user_cap);

    let user_events = find_events_by_topic(env.events().all(), &env, TOPIC_USER_CAP_UPDATED);
    assert!(
        !user_events.is_empty(),
        "set_user_deposit_cap should emit an event"
    );

    let (_, _, data) = &user_events[0];
    let event = UserDepositCapUpdatedEvent::try_from_val(&env, data)
        .expect("Should be a UserDepositCapUpdatedEvent");
    assert_eq!(event.old_cap, old_user_cap);
    assert_eq!(event.new_cap, new_user_cap);
}

#[test]
fn test_set_caps_emits_caps_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user_cap = 25_000_000_000_i128;
    let tvl_cap = 150_000_000_000_i128;
    client.set_caps(&user_cap, &tvl_cap);

    let caps_events = find_events_by_topic(env.events().all(), &env, TOPIC_CAPS_UPDATED);
    assert!(!caps_events.is_empty(), "set_caps should emit a caps event");

    let (_, _, data) = &caps_events[0];
    let event = CapsUpdatedEvent::try_from_val(&env, data).expect("Should be a CapsUpdatedEvent");
    assert_eq!(
        event.new_user_cap, user_cap,
        "Event new_user_cap should match set value"
    );
    assert_eq!(
        event.new_tvl_cap, tvl_cap,
        "Event new_tvl_cap should match set value"
    );
}

#[test]
fn test_update_agent_emits_agent_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    let agent_events = find_events_by_topic(env.events().all(), &env, TOPIC_AGENT_UPDATED);
    assert!(
        !agent_events.is_empty(),
        "update_agent should emit an event"
    );

    let (_, _, data) = &agent_events[0];
    let event =
        AgentUpdatedEvent::try_from_val(&env, data).expect("Should be an AgentUpdatedEvent");
    assert_eq!(
        event.old_agent, old_agent,
        "Event old_agent should match previous agent"
    );
    assert_eq!(
        event.new_agent, new_agent,
        "Event new_agent should match new agent"
    );
}

#[test]
fn test_update_total_assets_emits_assets_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let old_total = deposit_amount;
    let yield_amount = 5_000_000_i128;
    let new_total = old_total + yield_amount;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &new_total, &false, &0);

    let assets_events = find_events_by_topic(env.events().all(), &env, TOPIC_ASSETS_UPDATED);
    assert!(
        !assets_events.is_empty(),
        "update_total_assets should emit an event"
    );

    let (_, _, data) = &assets_events[0];
    let event =
        AssetsUpdatedEvent::try_from_val(&env, data).expect("Should be an AssetsUpdatedEvent");
    assert_eq!(
        event.old_total, old_total,
        "Event old_total should match previous total"
    );
    assert_eq!(
        event.new_total, new_total,
        "Event new_total should match new total"
    );
}

#[test]
fn test_rebalance_emits_rebalance_event_with_correct_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let expected_apy = 850_i128;
    client.rebalance(&symbol_short!("none"), &expected_apy, &0_i128);

    let rebalance_events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE);
    assert!(
        !rebalance_events.is_empty(),
        "rebalance should emit an event"
    );

    let (_, _, data) = &rebalance_events[0];
    let event = RebalanceEvent::try_from_val(&env, data).expect("Should be a RebalanceEvent");
    assert_eq!(
        event.protocol,
        symbol_short!("none"),
        "Event protocol should match rebalance target"
    );
    assert_eq!(
        event.expected_apy, expected_apy,
        "Event expected_apy should match provided APY"
    );
    assert_eq!(
        event.status,
        symbol_short!("noop"),
        "Event status should be noop when no funds move"
    );
    assert_eq!(
        event.amount_attempted, 0,
        "Event amount_attempted should be 0"
    );
    assert_eq!(event.amount_moved, 0, "Event amount_moved should be 0");
    assert_eq!(
        event.amount_supplied, 0,
        "Event amount_supplied should be 0"
    );
    assert_eq!(
        event.amount_withdrawn, 0,
        "Event amount_withdrawn should be 0"
    );
}

#[test]
fn test_rebalance_with_blend_emits_correct_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    let expected_apy = 1200_i128;
    client.rebalance(&symbol_short!("blend"), &expected_apy, &0_i128);

    let rebalance_events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE);
    let last_event_data = &rebalance_events.last().unwrap().2;
    let event =
        RebalanceEvent::try_from_val(&env, last_event_data).expect("Should be a RebalanceEvent");
    assert_eq!(
        event.protocol,
        symbol_short!("blend"),
        "Event protocol should be blend"
    );
    assert_eq!(
        event.expected_apy, expected_apy,
        "Event expected_apy should match provided APY"
    );
    assert_eq!(
        event.status,
        symbol_short!("success"),
        "Event status should be success"
    );
    assert_eq!(
        event.amount_attempted, 10_000_000_i128,
        "Event amount_attempted should match vault balance"
    );
    assert_eq!(
        event.amount_moved, 10_000_000_i128,
        "Event amount_moved should match supplied balance"
    );
    assert_eq!(
        event.amount_supplied, 10_000_000_i128,
        "Event amount_supplied should match supplied balance"
    );
    assert_eq!(
        event.amount_withdrawn, 0,
        "Event amount_withdrawn should be 0"
    );
}

#[test]
fn test_rebalance_with_blend_partial_fill_emits_correct_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    // Limit supply to 3M
    blend_client.set_max_supply_limit(&3_000_000_i128);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    let expected_apy = 1200_i128;
    client.rebalance(&symbol_short!("blend"), &expected_apy, &0_i128);

    let rebalance_events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE);
    let last_event_data = &rebalance_events.last().unwrap().2;
    let event =
        RebalanceEvent::try_from_val(&env, last_event_data).expect("Should be a RebalanceEvent");
    assert_eq!(
        event.protocol,
        symbol_short!("blend"),
        "Event protocol should be blend"
    );
    assert_eq!(
        event.status,
        symbol_short!("partial"),
        "Event status should be partial"
    );
    assert_eq!(
        event.amount_attempted, 10_000_000_i128,
        "Event amount_attempted should be 10M"
    );
    assert_eq!(
        event.amount_moved, 3_000_000_i128,
        "Event amount_moved should be 3M"
    );
    assert_eq!(
        event.amount_supplied, 3_000_000_i128,
        "Event amount_supplied should be 3M"
    );
    assert_eq!(
        event.amount_withdrawn, 0,
        "Event amount_withdrawn should be 0"
    );
}

#[test]
fn test_rebalance_with_blend_failed_fill_emits_correct_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    // Limit supply to -1 (simulates complete failure/0 returned)
    blend_client.set_max_supply_limit(&-1_i128);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    let expected_apy = 1200_i128;
    client.rebalance(&symbol_short!("blend"), &expected_apy, &0_i128);

    let rebalance_events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE);
    let last_event_data = &rebalance_events.last().unwrap().2;
    let event =
        RebalanceEvent::try_from_val(&env, last_event_data).expect("Should be a RebalanceEvent");
    assert_eq!(
        event.protocol,
        symbol_short!("blend"),
        "Event protocol should be blend"
    );
    assert_eq!(
        event.status,
        symbol_short!("failed"),
        "Event status should be failed"
    );
    assert_eq!(
        event.amount_attempted, 10_000_000_i128,
        "Event amount_attempted should be 10M"
    );
    assert_eq!(event.amount_moved, 0, "Event amount_moved should be 0");
    assert_eq!(
        event.amount_supplied, 0,
        "Event amount_supplied should be 0"
    );
    assert_eq!(
        event.amount_withdrawn, 0,
        "Event amount_withdrawn should be 0"
    );
}

#[test]
fn test_all_events_have_correct_topics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_deposit_limits(&1_000_000_i128, &10_000_000_000_i128);
    client.set_caps(&10_000_000_i128, &100_000_000_i128);
    client.rebalance(&symbol_short!("none"), &500_i128, &0_i128);
    client.pause(&owner);
    client.unpause(&owner);
    client.emergency_pause(&owner);

    let init_events = find_events_by_topic(env.events().all(), &env, TOPIC_INIT);
    let limits_events =
        find_events_by_topic(env.events().all(), &env, TOPIC_DEPOSIT_LIMITS_UPDATED);
    let caps_events = find_events_by_topic(env.events().all(), &env, TOPIC_CAPS_UPDATED);
    let rebalance_events = find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE);
    let paused_events = find_events_by_topic(env.events().all(), &env, TOPIC_PAUSED);
    let unpaused_events = find_events_by_topic(env.events().all(), &env, TOPIC_UNPAUSED);
    let emerg_events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED);

    assert!(!init_events.is_empty(), "Should have init events");
    assert!(!limits_events.is_empty(), "Should have limits events");
    assert!(!caps_events.is_empty(), "Should have caps events");
    assert!(!rebalance_events.is_empty(), "Should have rebalance events");
    assert!(!paused_events.is_empty(), "Should have paused events");
    assert!(!unpaused_events.is_empty(), "Should have unpaused events");
    assert!(
        !emerg_events.is_empty(),
        "Should have emergency paused events"
    );

    for (addr, topics, _) in env.events().all().iter() {
        assert_eq!(
            addr, contract_id,
            "All events should be from vault contract"
        );
        assert!(
            !topics.is_empty(),
            "Each event should have at least one topic"
        );
    }
}

#[test]
fn test_withdraw_all_partial_liquidity_emits_burned_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let sink = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Get post-deposit state for accurate calculations
    let shares_before = client.get_shares(&user);
    let _total_shares_before = client.get_total_shares();
    let _total_assets_before = client.get_total_assets();

    client.rebalance(&symbol_short!("blend"), &900_i128, &0_i128);

    // After rebalance: 90% in blend (9M), 10% in vault (1M)
    // Vault balance is 1,000,000

    // Drain most pool liquidity to force partial fulfillment in withdraw_all.
    // Pool had 9M, drain 8M, leaving 1M in pool
    token_client.transfer(&blend_pool, &sink, &8_000_000_i128);

    // Pre-withdrawal checks
    let vault_balance_before = token_client.balance(&contract_id);
    let blend_balance = token_client.balance(&blend_pool);

    // Expected: vault has 1M, blend pool has 1M
    // User entitled to 10M (full conversion of shares)
    // Available = vault_balance + blend_withdrawal
    // Blend can only return min(needed, pool_balance) where needed = 9M, pool_balance = 1M
    // So blend returns 1M
    // Total available = 1M (vault) + 1M (from blend) = 2M

    let entitled_amount = client.convert_to_assets(&shares_before);
    let expected_available_usdc = vault_balance_before + blend_balance; // 1M + 1M = 2M

    // When available < entitled, shares_to_burn = convert_to_shares(available)
    // shares_to_burn = available * total_shares / total_assets
    // shares_to_burn = 2M * 10M / 10M = 2M (assuming 1:1 ratio)
    let expected_shares_to_burn = client.convert_to_shares(&expected_available_usdc);

    let withdrawn = client.withdraw_all(&user, &None);
    let shares_after = client.get_shares(&user);

    // Verify we got partial liquidity
    assert!(withdrawn < entitled_amount, "Should be partial withdrawal");
    assert!(withdrawn > 0, "Should withdraw some amount");

    // Verify shares burned matches expected calculation (not full shares)
    let actual_shares_burned = shares_before - shares_after;
    assert_eq!(
        actual_shares_burned, expected_shares_to_burn,
        "Actual burned shares should match calculated expected shares"
    );
    assert!(
        actual_shares_burned < shares_before,
        "Should not burn all user shares in partial liquidity scenario"
    );

    let withdraw_events = find_events_by_topic(env.events().all(), &env, TOPIC_WITHDRAW);
    let last_event_data = &withdraw_events.last().unwrap().2;
    let event =
        WithdrawEvent::try_from_val(&env, last_event_data).expect("Should be a WithdrawEvent");

    // CRITICAL: Event must emit shares_to_burn (actual burned), NOT full user_shares
    assert_eq!(
        event.shares, actual_shares_burned,
        "Event shares must equal actual burned shares"
    );
    assert_ne!(
        event.shares, shares_before,
        "Event must NOT emit full user shares in partial liquidity path"
    );
    assert_eq!(
        event.shares, expected_shares_to_burn,
        "Event shares must match independently calculated expected shares"
    );
    assert_eq!(
        event.amount, withdrawn,
        "Event amount must match actual withdrawn amount"
    );
}

#[test]
fn test_blend_supply_event_reports_actual_supplied_with_shortfall() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Configure pool with supply shortfall limit (pool can only accept 3M)
    blend_client.set_max_supply_limit(&3_000_000_i128);

    client.set_blend_pool(&owner, &blend_pool);

    // Deposit 10M USDC to vault
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Rebalance 90% to blend - vault tries to supply 9M, but pool only accepts 3M
    client.rebalance(&symbol_short!("blend"), &900_i128, &0_i128);

    // Verify actual supplied was less than requested
    let pool_supplied = blend_client.supplied(&usdc_token);
    assert_eq!(
        pool_supplied, 3_000_000_i128,
        "Pool should only have 3M due to shortfall"
    );

    // Find blend supply events
    let supply_events = find_events_by_topic(env.events().all(), &env, TOPIC_BLEND_SUPPLY);
    assert!(!supply_events.is_empty(), "Should have blend supply events");

    // Get the last supply event
    let (_, _, data) = supply_events.last().unwrap();
    let event = BlendSupplyEvent::try_from_val(&env, data).expect("Should be BlendSupplyEvent");

    // CRITICAL: Event must report actual supplied (3M), NOT requested (9M)
    assert_eq!(
        event.amount_actual, 3_000_000_i128,
        "Event must report actual supplied amount, not requested amount"
    );
    assert!(
        event.amount_actual < 9_000_000_i128,
        "Actual supplied should be less than requested due to shortfall"
    );
    assert!(
        event.success,
        "Supply should be marked as successful even with partial fill"
    );

    // Verify vault balance: started with 10M, pool took 3M, vault should have 7M
    let vault_balance = token_client.balance(&contract_id);
    assert_eq!(
        vault_balance, 7_000_000_i128,
        "Vault should have 7M after partial supply"
    );
}

#[test]
fn test_blend_withdraw_event_reports_actual_withdrawn_with_shortfall() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    // Deposit 10M USDC to vault
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Rebalance to blend - 100% of vault balance (10M) goes to pool
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    // Verify pool has 10M (entire vault balance was supplied)
    assert_eq!(
        blend_client.supplied(&usdc_token),
        10_000_000_i128,
        "Pool should have 10M"
    );

    // Drain pool liquidity to force withdraw shortfall
    // Pool has 10M, drain 9.5M, leaving 500K
    let sink = Address::generate(&env);
    token_client.transfer(&blend_pool, &sink, &9_500_000_i128);

    let pool_balance_before = token_client.balance(&blend_pool);
    assert_eq!(
        pool_balance_before, 500_000_i128,
        "Pool should have 500K after drain"
    );

    // Verify vault has 0 balance (everything went to pool)
    let vault_balance_before = token_client.balance(&contract_id);
    assert_eq!(vault_balance_before, 0_i128, "Vault should have 0");

    // Record user balance before withdraw
    let user_balance_before = token_client.balance(&user);

    // User withdraws 2M - vault has 0, needs 2M from pool, but pool only has 500K
    // So expected withdraw: 500K (from pool only, since vault is empty)
    let requested_withdraw = 2_000_000_i128;
    let expected_actual = 500_000_i128; // Only what pool can give

    client.withdraw(&user, &requested_withdraw, &None);

    // Calculate actual withdrawn from user's balance change
    let user_balance_after = token_client.balance(&user);
    let withdrawn = user_balance_after.saturating_sub(user_balance_before);

    // Verify actual withdrawn is less than requested due to pool shortfall
    assert_eq!(
        withdrawn, expected_actual,
        "Should only withdraw what pool has (500K)"
    );
    assert!(
        withdrawn < requested_withdraw,
        "Actual withdrawn should be less than requested"
    );

    // Find blend withdraw events (pool was asked for 2M but only gave 500K)
    let blend_events = find_events_by_topic(env.events().all(), &env, TOPIC_BLEND_WITHDRAW);
    assert!(
        !blend_events.is_empty(),
        "Should have blend withdraw events"
    );

    // Get the last blend withdraw event
    let (_, _, data) = blend_events.last().unwrap();
    let event = BlendWithdrawEvent::try_from_val(&env, data).expect("Should be BlendWithdrawEvent");

    // CRITICAL: Event must report actual received (500K), NOT requested (2M)
    let expected_pool_withdrawn = 500_000_i128;
    assert_eq!(
        event.amount_actual, expected_pool_withdrawn,
        "Event must report actual received from pool, not requested amount"
    );
    assert!(
        event.amount_actual < requested_withdraw,
        "Actual received should be less than requested due to pool shortfall"
    );
    assert!(
        event.success,
        "Withdraw should be marked as successful even with partial fill"
    );
}
